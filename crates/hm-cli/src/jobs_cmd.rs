//! `hm jobs`: a durable queue of sandboxed host jobs (see `hv2_jobs`).
//!
//! The store is a directory: `--store DIR`, or `HM_JOBS_DIR`, or
//! `~/.hypermachine/jobs`. Every command and every worker that names the same
//! directory shares one queue, across processes and restarts.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use hv2_jobs::worker::WorkerConfig;
use hv2_jobs::{JobSpec, JobState, Phase, Store};

/// Where the queue lives.
#[derive(Debug, Args)]
pub struct StoreArgs {
    /// The store directory [default: $HM_JOBS_DIR or ~/.hypermachine/jobs]
    #[arg(long, global = true)]
    pub store: Option<PathBuf>,
}

impl StoreArgs {
    fn open(&self) -> Result<Store> {
        let root = self.store.clone().unwrap_or_else(Store::default_root);
        Store::open(&root).with_context(|| format!("opening the job store {}", root.display()))
    }
}

/// `hm jobs` commands.
#[derive(Debug, Subcommand)]
pub enum JobsCommand {
    /// Persist interval schedules and occurrences; does not execute jobs
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
    /// Queue a job from a spec file (JSON); prints its ID
    Submit {
        /// The spec, or `-` for standard input
        spec: PathBuf,
    },
    /// List jobs, oldest first
    List {
        /// Only jobs in this state: queued, running, succeeded, failed, cancelled
        #[arg(long)]
        state: Option<String>,
        /// Print JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one job's state (and spec, with --json)
    Status {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Print a job's output
    Logs {
        id: String,
        /// Keep printing as it is written, until the job ends
        #[arg(short, long)]
        follow: bool,
        /// Its stderr instead of its stdout
        #[arg(long)]
        stderr: bool,
    },
    /// Ask a job to stop: at once if queued; gracefully first if its spec says how
    Cancel { id: String },
    /// Run jobs from the queue until interrupted
    Worker {
        /// Labels this worker offers, comma-separated (e.g. gpu,cpu)
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,
        /// Jobs run at once
        #[arg(long, default_value = "1")]
        concurrency: usize,
        /// Seconds without a heartbeat before a running job is taken as lost
        #[arg(long, default_value = "30")]
        lease_secs: u64,
    },
    /// Serve the REST mirror, /api/v1/jobs, over this store
    Serve {
        /// Address to listen on
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: std::net::SocketAddr,
        /// Bearer token every request must carry [env: HM_JOBS_TOKEN]; required
        /// to listen anywhere but loopback, since a job runs a program
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Validate an operator connection profile; makes no network request
    ProfileCheck { file: PathBuf, name: String },
    /// List schedule names in lexical order as a JSON page
    List {
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Stop future occurrence publication; preserves committed history
    Cancel { id: String },
    /// Publish due occurrences repeatedly; does not execute jobs
    Watch {
        id: String,
        #[arg(long, default_value = "100")]
        limit: usize,
        #[arg(long, default_value = "1000")]
        poll_ms: u64,
        /// Stop after this many polling ticks (otherwise until Ctrl+C)
        #[arg(long)]
        ticks: Option<u64>,
    },
    /// Create an immutable interval schedule from JSON (use - for stdin)
    Create { id: String, spec: PathBuf },
    /// Print the schedule and occurrence-publication progress as JSON
    Status { id: String },
    /// Publish one bounded due batch; does not enqueue or execute jobs
    Publish {
        id: String,
        /// Unix milliseconds to plan through [default: current wall clock]
        #[arg(long)]
        now_ms: Option<u64>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Read a JSON page of committed occurrences; does not claim jobs
    Occurrences {
        id: String,
        /// Exclusive scheduled-time cursor in Unix milliseconds
        #[arg(long)]
        after_ms: Option<u64>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
}

/// Run a `hm jobs` command.
pub async fn run(store: &StoreArgs, command: JobsCommand) -> Result<i32> {
    let s = store.open()?;
    match command {
        JobsCommand::Schedule { command } => {
            let value = match command {
                ScheduleCommand::ProfileCheck { file, name } => {
                    crate::jobs_profile::validate_connection_profile(&file, &name)?;
                    serde_json::json!({"profile": name, "configuration_valid": true})
                }
                ScheduleCommand::List { after, limit } => {
                    serde_json::to_value(s.interval_schedule_ids(after.as_deref(), limit)?)?
                }
                ScheduleCommand::Cancel { id } => {
                    s.cancel_interval_schedule(&id)?;
                    serde_json::json!({"id": id, "cancelled": true})
                }
                ScheduleCommand::Watch {
                    id,
                    limit,
                    poll_ms,
                    ticks,
                } => {
                    if !(1..=1024).contains(&limit)
                        || !(1..=60_000).contains(&poll_ms)
                        || ticks == Some(0)
                    {
                        bail!("watch requires limit 1-1024, poll-ms 1-60000 and positive ticks");
                    }
                    s.interval_schedule(&id)?;
                    let shutdown = tokio::signal::ctrl_c();
                    tokio::pin!(shutdown);
                    let mut count = 0_u64;
                    loop {
                        if s.interval_schedule_cancelled(&id)? {
                            return Ok(0);
                        }
                        let (store, name) = (s.clone(), id.clone());
                        let mut task = tokio::task::spawn_blocking(move || {
                            store.materialize_interval(&name, hv2_jobs::now_ms(), limit)
                        });
                        let result = tokio::select! {
                            result = &mut task => result?,
                            signal = &mut shutdown => {
                                signal?;
                                // Finish an accepted filesystem operation before exit.
                                // Its records remain recoverable even without stdout.
                                task.await??;
                                return Ok(0);
                            }
                        };
                        match result {
                            Ok(records) => {
                                println!("{}", serde_json::to_string(&records)?);
                                std::io::stdout().flush()?;
                            }
                            Err(hv2_jobs::JobError::Conflict(_)) => {
                                eprintln!("schedule progress changed; retrying on next tick");
                            }
                            Err(error) => return Err(error.into()),
                        }
                        count = count
                            .checked_add(1)
                            .context("schedule tick count overflow")?;
                        if ticks.is_some_and(|ticks| count >= ticks) {
                            return Ok(0);
                        }
                        tokio::select! {
                            () = tokio::time::sleep(Duration::from_millis(poll_ms)) => {},
                            signal = &mut shutdown => { signal?; return Ok(0); }
                        }
                    }
                }
                ScheduleCommand::Create { id, spec } => {
                    let text = if spec.as_os_str() == "-" {
                        let mut text = String::new();
                        std::io::stdin().read_to_string(&mut text)?;
                        text
                    } else {
                        std::fs::read_to_string(&spec)
                            .with_context(|| format!("reading {}", spec.display()))?
                    };
                    let schedule =
                        serde_json::from_str(&text).context("the interval schedule spec")?;
                    s.create_interval_schedule(&id, &schedule)?;
                    serde_json::json!({"id": id})
                }
                ScheduleCommand::Status { id } => serde_json::json!({
                    "id": id, "schedule": s.interval_schedule(&id)?,
                    "cancelled": s.interval_schedule_cancelled(&id)?,
                    "publication_through_ms": s.interval_progress(&id)?
                }),
                ScheduleCommand::Publish { id, now_ms, limit } => serde_json::to_value(
                    s.materialize_interval(&id, now_ms.unwrap_or_else(hv2_jobs::now_ms), limit)?,
                )?,
                ScheduleCommand::Occurrences {
                    id,
                    after_ms,
                    limit,
                } => serde_json::to_value(s.committed_interval_occurrences(&id, after_ms, limit)?)?,
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(0)
        }
        JobsCommand::Submit { spec } => {
            let text = if spec.as_os_str() == "-" {
                let mut t = String::new();
                std::io::stdin().read_to_string(&mut t)?;
                t
            } else {
                std::fs::read_to_string(&spec)
                    .with_context(|| format!("reading {}", spec.display()))?
            };
            let spec: JobSpec = serde_json::from_str(&text).context("the job spec")?;
            println!("{}", s.submit(&spec)?);
            Ok(0)
        }
        JobsCommand::List { state, json } => {
            if let Some(want) = &state {
                if !["queued", "running", "succeeded", "failed", "cancelled"]
                    .contains(&want.as_str())
                {
                    bail!(
                        "--state is queued, running, succeeded, failed or cancelled, not {want:?}"
                    );
                }
            }
            let all: Vec<JobState> = s
                .list()?
                .into_iter()
                .filter(|j| state.as_deref().is_none_or(|w| j.state.as_str() == w))
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&all)?);
            } else {
                println!(
                    "{:<32} {:<10} {:>4} {:>5}  NAME",
                    "ID", "STATE", "TRY", "EXIT"
                );
                for j in &all {
                    let name = s
                        .spec(&j.id)
                        .ok()
                        .and_then(|sp| sp.name)
                        .unwrap_or_default();
                    println!(
                        "{:<32} {:<10} {:>4} {:>5}  {}",
                        j.id,
                        j.state.as_str(),
                        j.attempts,
                        j.exit_code.map_or_else(|| "-".into(), |c| c.to_string()),
                        name
                    );
                }
            }
            Ok(0)
        }
        JobsCommand::Status { id, json } => {
            let state = s.state(&id)?;
            if json {
                let spec = s.spec(&id)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({ "state": state, "spec": spec })
                    )?
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&state)?);
            }
            Ok(0)
        }
        JobsCommand::Logs { id, follow, stderr } => {
            s.state(&id)?;
            let path = s.log_path(&id, if stderr { "stderr" } else { "stdout" });
            let mut file = std::fs::File::open(&path)?;
            let mut out = std::io::stdout().lock();
            let mut pos = 0u64;
            loop {
                let ended = !follow || s.state(&id).map_or(true, |st| st.state.is_final());
                file.seek(SeekFrom::Start(pos))?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                pos += buf.len() as u64;
                out.write_all(&buf)?;
                out.flush()?;
                if ended {
                    return Ok(0);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        JobsCommand::Cancel { id } => {
            let state = s.cancel(&id)?;
            println!(
                "{}",
                if state.state == Phase::Cancelled {
                    format!("{id} cancelled")
                } else {
                    format!("{id}: cancel asked for; its worker is stopping it")
                }
            );
            Ok(0)
        }
        JobsCommand::Worker {
            labels,
            concurrency,
            lease_secs,
        } => {
            let mut s = s;
            s.lease = Duration::from_secs(lease_secs.max(3 * hv2_jobs::HEARTBEAT.as_secs()));
            let config = WorkerConfig::named_for_this_process(labels, concurrency);
            eprintln!(
                "hm jobs worker {}: store {}, labels [{}], {} at a time. Ctrl-C stops taking jobs \
                 and waits for the ones running; a second Ctrl-C exits at once.",
                config.name,
                s.root().display(),
                config.labels.join(","),
                config.concurrency
            );
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    eprintln!("hm jobs worker: finishing the jobs in hand");
                    flag.store(true, Ordering::SeqCst);
                    if tokio::signal::ctrl_c().await.is_ok() {
                        std::process::exit(130);
                    }
                }
            });
            tokio::task::spawn_blocking(move || hv2_jobs::worker::run(&s, &config, &stop)).await?;
            Ok(0)
        }
        JobsCommand::Serve { addr, token } => {
            let token = token.or_else(|| std::env::var("HM_JOBS_TOKEN").ok());
            if !addr.ip().is_loopback() && token.is_none() {
                bail!(
                    "refusing to serve jobs on {addr} without --token: anyone who reaches it could \
                     run programs on the workers"
                );
            }
            let listener = tokio::net::TcpListener::bind(addr).await?;
            eprintln!("hm jobs: serving /api/v1/jobs on http://{addr}");
            axum::serve(listener, hv2_jobs::http::router(s, token)).await?;
            Ok(0)
        }
    }
}
