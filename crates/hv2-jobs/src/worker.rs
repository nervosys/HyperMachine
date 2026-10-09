//! A worker: claims jobs from a [`Store`] and runs them under
//! [`hv2_sandbox::ProcessSandbox`], one per slot.
//!
//! While a job runs, a watcher beside it renews the worker's lease and
//! watches for a cancel. A cancel with a [`GracefulStop`](crate::GracefulStop) creates the stop
//! file first and gives the program its grace period; then, or without one,
//! the whole process tree is killed. A worker that finds its lease taken --
//! it was too slow, and the job was requeued -- kills its run at once, so a
//! job never runs twice at the same time.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hv2_sandbox::{
    FilesystemPolicy, NetworkPolicy, OutputStream, ProcessSandbox, RunIo, Sandbox, SandboxCommand,
    SandboxError, SandboxSpec,
};

use crate::{now_ms, parse_size, JobSpec, JobState, Phase, Store, HEARTBEAT};

/// How a worker runs.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// How this worker names itself in job states.
    pub name: String,
    /// Labels it offers; it takes jobs whose labels are all among them.
    pub labels: Vec<String>,
    /// Jobs it runs at once.
    pub concurrency: usize,
    /// How long it waits between looks at an empty queue.
    pub poll: Duration,
}

impl WorkerConfig {
    /// A worker named after this host and process.
    pub fn named_for_this_process(labels: Vec<String>, concurrency: usize) -> Self {
        let host = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "host".into());
        Self {
            name: format!("{host}-{}", std::process::id()),
            labels,
            concurrency: concurrency.max(1),
            poll: Duration::from_secs(1),
        }
    }
}

/// Run until `stop` is set, then finish the jobs in hand and return.
pub fn run(store: &Store, config: &WorkerConfig, stop: &AtomicBool) {
    std::thread::scope(|scope| {
        for slot in 0..config.concurrency {
            let name = if config.concurrency == 1 {
                config.name.clone()
            } else {
                format!("{}/{slot}", config.name)
            };
            scope.spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match store.claim(&name, &config.labels) {
                        Ok(Some(claim)) => {
                            tracing::info!("{name}: running job {}", claim.id);
                            let state = run_job(store, &claim.id, &claim.spec, &claim.token);
                            tracing::info!("{name}: job {} {}", claim.id, state.state.as_str());
                        }
                        Ok(None) => std::thread::sleep(config.poll),
                        Err(e) => {
                            tracing::warn!("{name}: {e}");
                            std::thread::sleep(config.poll);
                        }
                    }
                }
            });
        }
    });
}

/// The sandbox spec a job's settings ask for.
pub fn sandbox_spec(spec: &JobSpec) -> std::result::Result<SandboxSpec, String> {
    let s = &spec.sandbox;
    let filesystem = match s.fs.as_deref().unwrap_or("host") {
        "host" => FilesystemPolicy::Host,
        other => FilesystemPolicy::Isolated {
            root: PathBuf::from(other.strip_prefix("isolated:").unwrap_or(other)),
            read_only: s.ro.clone(),
        },
    };
    Ok(SandboxSpec {
        memory_bytes: s.memory.as_deref().map(parse_size).transpose()?,
        max_processes: s.max_processes,
        cpu_time: s.cpu_time_secs.map(Duration::from_secs),
        wall_clock: s.wall_clock_secs.map(Duration::from_secs),
        network: if s.net.as_deref() == Some("host") {
            NetworkPolicy::Host
        } else {
            NetworkPolicy::Denied
        },
        filesystem,
        grants: hv2_sandbox::PathGrants::default(),
        confine_paths: false,
        isolate_processes: s.isolate_processes,
        no_new_privileges: s.no_new_privileges,
        best_effort: !s.strict,
    })
}

/// Run claimed job `id` to its end, and record the end.
pub fn run_job(store: &Store, id: &str, spec: &JobSpec, worker: &str) -> JobState {
    let fail = |message: String| {
        store
            .finish(id, |s| {
                s.state = Phase::Failed;
                s.message = Some(message.clone());
            })
            .unwrap_or_else(|_| lost_state(id))
    };
    let sandbox_spec = match sandbox_spec(spec) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let (program, args) = match spec.command.split_first() {
        Some(split) => split,
        None => return fail("command is empty".into()),
    };
    let mut command = SandboxCommand::new(program.clone()).args(args.iter().cloned());
    command.env = hv2_sandbox::host_base_env();
    command.env.extend(spec.env.clone());
    command.env.insert("HM_JOB_ID".into(), id.to_string());
    command.working_dir = spec.workdir.clone();

    let open = |stream: &str| {
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(store.log_path(id, stream))
    };
    let (stdout, stderr) = match (open("stdout"), open("stderr")) {
        (Ok(o), Ok(e)) => (Arc::new(Mutex::new(o)), Arc::new(Mutex::new(e))),
        (Err(e), _) | (_, Err(e)) => return fail(format!("opening its logs: {e}")),
    };
    let sink = {
        let (stdout, stderr) = (Arc::clone(&stdout), Arc::clone(&stderr));
        Arc::new(move |stream: OutputStream, bytes: &[u8]| {
            let file: &Mutex<File> = match stream {
                OutputStream::Stdout => &stdout,
                OutputStream::Stderr => &stderr,
            };
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(bytes);
            }
        })
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let lease_lost = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (store, id, worker) = (store.clone(), id.to_string(), worker.to_string());
        let (cancel, done, lease_lost, cancelled) = (
            Arc::clone(&cancel),
            Arc::clone(&done),
            Arc::clone(&lease_lost),
            Arc::clone(&cancelled),
        );
        let graceful = spec.graceful_stop.clone().map(|g| {
            let file = match &spec.workdir {
                Some(dir) if g.create_file.is_relative() => dir.join(&g.create_file),
                _ => g.create_file.clone(),
            };
            (file, Duration::from_secs(g.grace_secs))
        });
        std::thread::spawn(move || {
            watch(
                &store,
                &id,
                &worker,
                graceful,
                &cancel,
                &done,
                &lease_lost,
                &cancelled,
            );
        })
    };

    let io = RunIo {
        on_output: Some(sink),
        cancel: Some(Arc::clone(&cancel)),
    };
    let result = ProcessSandbox::new().run_with(&command, &sandbox_spec, &io);
    done.store(true, Ordering::SeqCst);
    let _ = watcher.join();

    if lease_lost.load(Ordering::SeqCst) {
        // The job is no longer ours; whoever took it records its fate.
        return store.state(id).unwrap_or_else(|_| lost_state(id));
    }
    let was_cancelled = cancelled.load(Ordering::SeqCst) || store.cancel_requested(id);
    let finished = store.finish(id, |s| match &result {
        Ok(output) => {
            s.exit_code = output.exit_code;
            s.signal = output.signal;
            s.killed_by = output.killed_by.map(|c| c.to_string());
            s.unenforced = output.unenforced.iter().map(ToString::to_string).collect();
            s.cancel_requested |= was_cancelled;
            // A graceful stop can end in exit 0; it is still a cancel.
            s.state = if was_cancelled {
                Phase::Cancelled
            } else if output.succeeded() {
                Phase::Succeeded
            } else {
                Phase::Failed
            };
        }
        Err(e) => {
            s.state = if was_cancelled {
                Phase::Cancelled
            } else {
                Phase::Failed
            };
            s.message = Some(match e {
                SandboxError::Unsupported { .. } => format!("{e} (the spec has sandbox.strict)"),
                other => other.to_string(),
            });
        }
    });
    finished.unwrap_or_else(|_| lost_state(id))
}

/// Renew the lease and watch for a cancel, until the run is `done`.
#[allow(clippy::too_many_arguments)]
fn watch(
    store: &Store,
    id: &str,
    worker: &str,
    graceful: Option<(PathBuf, Duration)>,
    cancel: &AtomicBool,
    done: &AtomicBool,
    lease_lost: &AtomicBool,
    cancelled: &AtomicBool,
) {
    let tick = Duration::from_millis(200);
    let mut last_beat = Instant::now();
    let mut stopping_since: Option<Instant> = None;
    while !done.load(Ordering::SeqCst) {
        std::thread::sleep(tick);
        if last_beat.elapsed() >= HEARTBEAT {
            last_beat = Instant::now();
            if store.heartbeat(id, worker).is_err() {
                tracing::warn!("{worker}: lost the lease on {id}; stopping it");
                lease_lost.store(true, Ordering::SeqCst);
                cancel.store(true, Ordering::SeqCst);
                return;
            }
        }
        if stopping_since.is_none() && store.cancel_requested(id) {
            cancelled.store(true, Ordering::SeqCst);
            match &graceful {
                Some((file, _)) => {
                    if let Err(e) = File::create(file) {
                        tracing::warn!("{worker}: creating {}: {e}; killing {id}", file.display());
                        cancel.store(true, Ordering::SeqCst);
                    }
                    stopping_since = Some(Instant::now());
                }
                None => cancel.store(true, Ordering::SeqCst),
            }
        }
        if let (Some(since), Some((_, grace))) = (stopping_since, &graceful) {
            if since.elapsed() >= *grace {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

fn lost_state(id: &str) -> JobState {
    JobState {
        id: id.to_string(),
        state: Phase::Failed,
        submitted_ms: 0,
        attempts: 0,
        worker: None,
        started_ms: None,
        heartbeat_ms: None,
        finished_ms: Some(now_ms()),
        exit_code: None,
        signal: None,
        killed_by: None,
        unenforced: Vec::new(),
        cancel_requested: false,
        message: Some("its state could not be read".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GracefulStop, SandboxSettings};
    use std::collections::BTreeMap;

    /// Under this variable, [`job_helper`] is the workload.
    const HELPER: &str = "HV2_JOBS_HELPER";

    /// The workload: `print` prints and exits; `fail` exits 3; `env` prints
    /// HM_JOB_ID; `wait STOPFILE` waits for the file, then exits 0; `sleep`
    /// sleeps a minute.
    #[test]
    fn job_helper() {
        let Ok(mode) = std::env::var(HELPER) else {
            return;
        };
        let arg = std::env::var("HV2_JOBS_ARG").unwrap_or_default();
        match mode.as_str() {
            "print" => {
                println!("hello from the job");
                eprintln!("and its stderr");
            }
            "fail" => std::process::exit(3),
            "env" => println!("{}", std::env::var("HM_JOB_ID").unwrap_or_default()),
            "wait" => {
                for _ in 0..600 {
                    if std::path::Path::new(&arg).exists() {
                        println!("stopping cleanly");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            _ => std::thread::sleep(Duration::from_secs(60)),
        }
    }

    fn helper_spec(mode: &str, arg: &str) -> JobSpec {
        let exe = std::env::current_exe().unwrap();
        let mut env = BTreeMap::new();
        env.insert(HELPER.to_string(), mode.to_string());
        env.insert("HV2_JOBS_ARG".to_string(), arg.to_string());
        JobSpec {
            not_before_ms: None,
            name: Some(mode.into()),
            command: vec![
                exe.to_string_lossy().into_owned(),
                "--exact".into(),
                "worker::tests::job_helper".into(),
                "--nocapture".into(),
            ],
            workdir: None,
            env,
            sandbox: SandboxSettings {
                net: Some("host".into()),
                ..SandboxSettings::default()
            },
            labels: Vec::new(),
            max_attempts: 1,
            graceful_stop: None,
        }
    }

    fn run_one(store: &Store, spec: &JobSpec) -> JobState {
        store.submit(spec).unwrap();
        let c = store.claim("t", &[]).unwrap().unwrap();
        run_job(store, &c.id, &c.spec, &c.token)
    }

    #[test]
    fn a_job_runs_and_its_output_and_exit_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let st = run_one(&store, &helper_spec("print", ""));
        assert_eq!(
            (st.state, st.exit_code),
            (Phase::Succeeded, Some(0)),
            "{st:?}"
        );
        let out = std::fs::read_to_string(store.log_path(&st.id, "stdout")).unwrap();
        let err = std::fs::read_to_string(store.log_path(&st.id, "stderr")).unwrap();
        assert!(out.contains("hello from the job"), "{out}");
        assert!(err.contains("and its stderr"), "{err}");
    }

    #[test]
    fn a_non_zero_exit_is_a_failure_with_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let st = run_one(&store, &helper_spec("fail", ""));
        assert_eq!((st.state, st.exit_code), (Phase::Failed, Some(3)), "{st:?}");
    }

    #[test]
    fn the_job_knows_its_own_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let st = run_one(&store, &helper_spec("env", ""));
        let out = std::fs::read_to_string(store.log_path(&st.id, "stdout")).unwrap();
        assert!(
            out.contains(&st.id),
            "HM_JOB_ID was {out:?}, want {}",
            st.id
        );
    }

    #[test]
    fn a_cancel_without_a_graceful_stop_kills_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.submit(&helper_spec("sleep", "")).unwrap();
        let c = store.claim("t", &[]).unwrap().unwrap();
        let (id, spec, token) = (c.id, c.spec, c.token);
        let s2 = store.clone();
        let id2 = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            s2.cancel(&id2).unwrap();
        });
        let started = Instant::now();
        let st = run_job(&store, &id, &spec, &token);
        assert_eq!(st.state, Phase::Cancelled, "{st:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    /// AWAKE's contract: the stop file appears, the program exits 0 on its
    /// own, and the job is recorded as cancelled, not succeeded.
    #[test]
    fn a_graceful_stop_creates_the_file_and_ends_as_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let stop_file = dir.path().join("STOP");
        let mut spec = helper_spec("wait", &stop_file.to_string_lossy());
        spec.graceful_stop = Some(GracefulStop {
            create_file: stop_file.clone(),
            grace_secs: 30,
        });
        store.submit(&spec).unwrap();
        let c = store.claim("t", &[]).unwrap().unwrap();
        let (id, spec, token) = (c.id, c.spec, c.token);
        let s2 = store.clone();
        let id2 = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            s2.cancel(&id2).unwrap();
        });
        let st = run_job(&store, &id, &spec, &token);
        assert!(stop_file.exists(), "the stop file was never created");
        assert_eq!(
            (st.state, st.exit_code),
            (Phase::Cancelled, Some(0)),
            "{st:?}"
        );
        let out = std::fs::read_to_string(store.log_path(&id, "stdout")).unwrap();
        assert!(
            out.contains("stopping cleanly"),
            "it was killed, not stopped: {out:?}"
        );
    }

    /// A program that ignores the stop file is killed when its grace ends.
    #[test]
    fn a_graceful_stop_that_is_ignored_ends_in_a_kill() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let mut spec = helper_spec("sleep", "");
        spec.graceful_stop = Some(GracefulStop {
            create_file: dir.path().join("IGNORED"),
            grace_secs: 1,
        });
        store.submit(&spec).unwrap();
        let c = store.claim("t", &[]).unwrap().unwrap();
        let (id, spec, token) = (c.id, c.spec, c.token);
        let s2 = store.clone();
        let id2 = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            s2.cancel(&id2).unwrap();
        });
        let started = Instant::now();
        let st = run_job(&store, &id, &spec, &token);
        assert_eq!(st.state, Phase::Cancelled, "{st:?}");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
    }

    /// Workers drain a queue between them, each job once.
    #[test]
    fn a_worker_pool_runs_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let ids: Vec<String> = (0..4)
            .map(|_| store.submit(&helper_spec("print", "")).unwrap())
            .collect();
        let stop = AtomicBool::new(false);
        let config = WorkerConfig {
            name: "pool".into(),
            labels: Vec::new(),
            concurrency: 2,
            poll: Duration::from_millis(50),
        };
        std::thread::scope(|s| {
            s.spawn(|| run(&store, &config, &stop));
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline
                && !ids
                    .iter()
                    .all(|id| store.state(id).unwrap().state.is_final())
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            stop.store(true, Ordering::SeqCst);
        });
        for id in &ids {
            let st = store.state(id).unwrap();
            assert_eq!((st.state, st.attempts), (Phase::Succeeded, 1), "{st:?}");
        }
    }
}
