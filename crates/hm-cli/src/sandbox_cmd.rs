//! `hm sandbox run`: run a host program under [`hv2_sandbox::ProcessSandbox`],
//! with its output streamed live and its exit code passed back.
//!
//! Before the program starts, a report on stderr says, for every control the
//! run asked for, whether this host enforces it or why not. By default a
//! control this host cannot enforce is dropped and reported (best effort);
//! `--strict` refuses the run instead.
//!
//! Exit codes follow `timeout(1)` and the shells: the program's own code;
//! 124 when the wall-clock deadline killed it; 128+N for a signal; 125 when
//! the run was refused or could not be confined; 127 when the program could
//! not be started; 130 when interrupted.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use clap::{Args, ValueEnum};
use serde_json::json;

use hv2_sandbox::{
    Control, FilesystemPolicy, NetworkPolicy, OutputStream, ProcessSandbox, RunIo, Sandbox,
    SandboxCommand, SandboxError, SandboxSpec,
};

/// How the enforcement report is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Report {
    /// Human-readable, on stderr.
    Text,
    /// One JSON object, on stderr.
    Json,
    /// No report.
    None,
}

/// What the program may reach on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Net {
    /// No network (loopback only, where the host can isolate it).
    Deny,
    /// The host's network.
    Host,
}

/// `hm sandbox run` arguments.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Memory ceiling, e.g. 512M, 4G.
    #[arg(long, value_parser = parse_size)]
    pub memory: Option<u64>,
    /// CPU-time ceiling, in seconds.
    #[arg(long)]
    pub cpu_time: Option<u64>,
    /// Wall-clock deadline, in seconds; the whole process tree is killed at it.
    #[arg(long)]
    pub wall_clock: Option<u64>,
    /// Ceiling on processes and threads.
    #[arg(long)]
    pub max_processes: Option<u32>,
    /// Network access.
    #[arg(long, value_enum, default_value = "deny")]
    pub net: Net,
    /// Filesystem view: `host`, or `isolated:ROOT` to make ROOT the program's `/`.
    #[arg(long, default_value = "host")]
    pub fs: String,
    /// A host path mounted read-only at the same path inside `--fs isolated:ROOT`.
    #[arg(long = "ro", value_name = "PATH")]
    pub read_only: Vec<PathBuf>,
    /// Working directory.
    #[arg(long)]
    pub workdir: Option<PathBuf>,
    /// Set a variable: K=V. Repeatable.
    #[arg(long = "env", value_name = "K=V")]
    pub env: Vec<String>,
    /// Pass a host variable through by name. Repeatable.
    #[arg(long = "pass-env", value_name = "NAME")]
    pub pass_env: Vec<String>,
    /// Start from an empty environment instead of PATH, HOME, TEMP and the
    /// rest a program needs to run.
    #[arg(long)]
    pub clean_env: bool,
    /// Hide the host's processes from the program.
    #[arg(long)]
    pub isolate_processes: bool,
    /// Bar the program from gaining privileges.
    #[arg(long)]
    pub no_new_privileges: bool,
    /// Refuse to run if any asked-for control cannot be enforced here.
    #[arg(long)]
    pub strict: bool,
    /// How to report what is enforced.
    #[arg(long, value_enum, default_value = "text")]
    pub report: Report,
    /// The program and its arguments, after `--`.
    #[arg(last = true, required = true, value_name = "CMD")]
    pub command: Vec<String>,
}

/// `512`, `64K`, `512M`, `4G` (binary units; a trailing `B` or `iB` is fine).
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let t = if lower.ends_with("ib") {
        &t[..t.len() - 2]
    } else if lower.ends_with('b') {
        &t[..t.len() - 1]
    } else {
        t
    };
    let (digits, shift) = match t.chars().last() {
        Some('K' | 'k') => (&t[..t.len() - 1], 10),
        Some('M' | 'm') => (&t[..t.len() - 1], 20),
        Some('G' | 'g') => (&t[..t.len() - 1], 30),
        Some('T' | 't') => (&t[..t.len() - 1], 40),
        _ => (t, 0),
    };
    let n: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("{text:?} is not a size like 512M or 4G"))?;
    n.checked_shl(shift)
        .filter(|v| v >> shift == n)
        .ok_or_else(|| format!("{text:?} is too large"))
}

/// The spec `args` asks for.
pub fn spec_of(args: &RunArgs) -> Result<SandboxSpec> {
    let filesystem = match args.fs.as_str() {
        "host" => {
            if !args.read_only.is_empty() {
                bail!("--ro mounts into an isolated filesystem; use --fs isolated:ROOT");
            }
            FilesystemPolicy::Host
        }
        other => match other.strip_prefix("isolated:") {
            Some(root) if !root.is_empty() => FilesystemPolicy::Isolated {
                root: PathBuf::from(root),
                read_only: args.read_only.clone(),
            },
            _ => bail!("--fs is `host` or `isolated:ROOT`, not {other:?}"),
        },
    };
    Ok(SandboxSpec {
        memory_bytes: args.memory,
        max_processes: args.max_processes,
        cpu_time: args.cpu_time.map(Duration::from_secs),
        wall_clock: args.wall_clock.map(Duration::from_secs),
        network: match args.net {
            Net::Deny => NetworkPolicy::Denied,
            Net::Host => NetworkPolicy::Host,
        },
        filesystem,
        isolate_processes: args.isolate_processes,
        no_new_privileges: args.no_new_privileges,
        best_effort: !args.strict,
    })
}

/// The environment `args` gives the program.
pub fn env_of(
    args: &RunArgs,
    host: impl Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    if !args.clean_env {
        for name in hv2_sandbox::HOST_BASE_ENV {
            if let Some(v) = host(name) {
                env.insert((*name).to_string(), v);
            }
        }
    }
    for name in &args.pass_env {
        match host(name) {
            Some(v) => {
                env.insert(name.clone(), v);
            }
            None => bail!("--pass-env {name}: not set here"),
        }
    }
    for pair in &args.env {
        let (k, v) = pair
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| anyhow!("--env {pair:?}: expected K=V"))?;
        env.insert(k.to_string(), v.to_string());
    }
    Ok(env)
}

/// What the run asked for against what this host enforces, as a report.
pub fn report_of(sandbox: &ProcessSandbox, spec: &SandboxSpec) -> serde_json::Value {
    let controls = sandbox.controls();
    let rows: Vec<serde_json::Value> = spec
        .required()
        .into_iter()
        .map(|c: Control| {
            if controls.enforces(c) {
                json!({ "control": c.to_string(), "enforced": true })
            } else {
                json!({
                    "control": c.to_string(),
                    "enforced": false,
                    "reason": controls.reason(c).unwrap_or("not available on this host"),
                })
            }
        })
        .collect();
    json!({
        "backend": sandbox.name(),
        "os": std::env::consts::OS,
        "mode": if spec.best_effort { "best-effort" } else { "strict" },
        "controls": rows,
    })
}

fn print_report(report: &serde_json::Value, how: Report) {
    let mut err = std::io::stderr().lock();
    match how {
        Report::None => {}
        Report::Json => {
            let _ = writeln!(err, "{report}");
        }
        Report::Text => {
            let _ = writeln!(
                err,
                "hm sandbox: {} backend on {}, {}",
                report["backend"].as_str().unwrap_or("?"),
                report["os"].as_str().unwrap_or("?"),
                report["mode"].as_str().unwrap_or("?")
            );
            for row in report["controls"].as_array().into_iter().flatten() {
                let name = row["control"].as_str().unwrap_or("?");
                if row["enforced"].as_bool() == Some(true) {
                    let _ = writeln!(err, "  enforced     {name}");
                } else {
                    let _ = writeln!(
                        err,
                        "  NOT ENFORCED {name}: {}",
                        row["reason"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
}

/// Run it: report, stream, and return the exit code to leave with.
pub async fn run(args: RunArgs) -> Result<i32> {
    let spec = spec_of(&args)?;
    let env = env_of(&args, |k| std::env::var(k).ok())?;
    let (program, rest) = args
        .command
        .split_first()
        .ok_or_else(|| anyhow!("no program to run"))?;
    let mut command = SandboxCommand::new(program.clone()).args(rest.iter().cloned());
    command.env = env;
    command.working_dir = args.workdir.clone();

    let sandbox = ProcessSandbox::new();
    print_report(&report_of(&sandbox, &spec), args.report);

    let cancel = Arc::new(AtomicBool::new(false));
    let io = RunIo {
        on_output: Some(Arc::new(|stream, bytes: &[u8]| match stream {
            OutputStream::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            }
            OutputStream::Stderr => {
                let mut err = std::io::stderr().lock();
                let _ = err.write_all(bytes);
                let _ = err.flush();
            }
        })),
        cancel: Some(Arc::clone(&cancel)),
    };

    let mut task = tokio::task::spawn_blocking(move || sandbox.run_with(&command, &spec, &io));
    let mut interrupted = false;
    let result = loop {
        tokio::select! {
            joined = &mut task => break joined?,
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                cancel.store(true, Ordering::SeqCst);
            }
        }
    };

    Ok(match result {
        _ if interrupted => 130,
        Ok(output) => {
            if !output.unenforced.is_empty() && args.report != Report::None {
                let names: Vec<String> =
                    output.unenforced.iter().map(ToString::to_string).collect();
                eprintln!("hm sandbox: ran without {}", names.join(", "));
            }
            match (output.killed_by, output.exit_code, output.signal) {
                (Some(Control::WallClock), _, _) => {
                    eprintln!("hm sandbox: killed at the wall-clock deadline");
                    124
                }
                (Some(control), _, _) => {
                    eprintln!("hm sandbox: killed by the {control}");
                    125
                }
                (None, Some(code), _) => code,
                (None, None, Some(signal)) => 128 + signal,
                (None, None, None) => 1,
            }
        }
        Err(SandboxError::Spawn { program, source }) => {
            eprintln!("hm sandbox: could not start {program}: {source}");
            127
        }
        Err(SandboxError::Unsupported { controls }) => {
            eprintln!(
                "hm sandbox: --strict, and this host cannot enforce: {}. Drop --strict to run                  without them, or drop the flags that ask for them.",
                controls.join("; ")
            );
            125
        }
        Err(e) => {
            eprintln!("hm sandbox: {e}");
            125
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct T {
        #[command(flatten)]
        args: RunArgs,
    }

    fn parse(argv: &[&str]) -> RunArgs {
        T::try_parse_from(std::iter::once("t").chain(argv.iter().copied()))
            .unwrap()
            .args
    }

    #[test]
    fn sizes_parse_in_binary_units() {
        assert_eq!(parse_size("512"), Ok(512));
        assert_eq!(parse_size("64K"), Ok(64 << 10));
        assert_eq!(parse_size("512M"), Ok(512 << 20));
        assert_eq!(parse_size("4G"), Ok(4 << 30));
        assert_eq!(parse_size("4GiB"), Ok(4 << 30));
        assert_eq!(parse_size("2gb"), Ok(2 << 30));
        assert!(parse_size("lots").is_err());
        assert!(parse_size("99999999999T").is_err());
    }

    #[test]
    fn flags_become_the_spec() {
        let a = parse(&[
            "--memory",
            "4G",
            "--cpu-time",
            "60",
            "--wall-clock",
            "90",
            "--max-processes",
            "64",
            "--net",
            "host",
            "--strict",
            "--",
            "prog",
            "arg",
        ]);
        let s = spec_of(&a).unwrap();
        assert_eq!(s.memory_bytes, Some(4 << 30));
        assert_eq!(s.cpu_time, Some(Duration::from_secs(60)));
        assert_eq!(s.wall_clock, Some(Duration::from_secs(90)));
        assert_eq!(s.max_processes, Some(64));
        assert_eq!(s.network, NetworkPolicy::Host);
        assert!(!s.best_effort);
        assert_eq!(a.command, ["prog", "arg"]);
    }

    #[test]
    fn the_defaults_deny_the_network_and_run_best_effort() {
        let s = spec_of(&parse(&["--", "prog"])).unwrap();
        assert_eq!(s.network, NetworkPolicy::Denied);
        assert_eq!(s.filesystem, FilesystemPolicy::Host);
        assert!(s.best_effort);
    }

    #[test]
    fn isolated_filesystems_take_a_root_and_read_only_mounts() {
        let s = spec_of(&parse(&[
            "--fs",
            "isolated:/srv/root",
            "--ro",
            "/usr",
            "--",
            "p",
        ]))
        .unwrap();
        assert_eq!(
            s.filesystem,
            FilesystemPolicy::Isolated {
                root: "/srv/root".into(),
                read_only: vec!["/usr".into()]
            }
        );
        assert!(spec_of(&parse(&["--fs", "isolated:", "--", "p"])).is_err());
        assert!(spec_of(&parse(&["--fs", "chroot", "--", "p"])).is_err());
        assert!(spec_of(&parse(&["--ro", "/usr", "--", "p"])).is_err());
    }

    #[test]
    fn the_environment_is_a_base_plus_what_is_named_and_nothing_else() {
        let host = |k: &str| match k {
            "PATH" => Some("/bin".to_string()),
            "SECRET_TOKEN" => Some("t".to_string()),
            "AWS_SECRET_ACCESS_KEY" => Some("k".to_string()),
            _ => None,
        };
        let env = env_of(
            &parse(&["--env", "A=1=2", "--pass-env", "SECRET_TOKEN", "--", "p"]),
            host,
        )
        .unwrap();
        assert_eq!(env.get("PATH").map(String::as_str), Some("/bin"));
        assert_eq!(env.get("A").map(String::as_str), Some("1=2"));
        assert_eq!(env.get("SECRET_TOKEN").map(String::as_str), Some("t"));
        assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));

        let clean = env_of(&parse(&["--clean-env", "--", "p"]), host).unwrap();
        assert!(clean.is_empty());
        assert!(env_of(&parse(&["--env", "=x", "--", "p"]), host).is_err());
        assert!(env_of(&parse(&["--pass-env", "NOPE", "--", "p"]), host).is_err());
    }
}
