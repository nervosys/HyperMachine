//! A guest's processes, listed from outside: `GET /machines/{name}/processes`
//! and `GET /sandboxes/{id}/processes`, as `ps` would list them inside.
//!
//! Every process the guest's kernel has, not only those started through the
//! API, read from its `/proc` through the guest agent's directory and file
//! operations, so a guest with no shell lists as an Ubuntu one does. An agent
//! that predates those operations lists through one POSIX shell instead
//! (`is_unsupported_op`), and then so is the shell left out of the listing,
//! with what it runs. Kernel threads are left out unless `?all=true`.
//!
//! Each process: `pid`, `ppid`, `uid`, `state` (`R`, `S`, `D`, `Z`, `T` and
//! the rest, as `ps` shows them), `command` (its name, as the kernel keeps
//! it), `args` (its command line, empty for a zombie), `threads`, `rssKiB`,
//! `cpuSeconds` (user and system time so far) and `startedSecondsAgo`.

use std::time::Duration;

use hv2_agent::AgentVM;
use serde::Deserialize;
use serde_json::{json, Value};

/// How long listing may take.
const LIST_TIMEOUT: Duration = Duration::from_secs(20);

/// The most processes a listing returns.
const MAX_PROCESSES: usize = 4096;

/// The kernel's clock ticks a second, which `/proc/<pid>/stat` counts in.
/// 100 on every Linux this project boots (`CONFIG_HZ` does not change it).
const TICKS: f64 = 100.0;

/// The guest's uptime, this shell's PID, then for each process its PID, its
/// `stat`, its command line with NULs made \001, and its real UID: every
/// field ended by NUL. A process that ends while it is read is skipped.
const LIST: &str = r#"read up _ < /proc/uptime
printf '%s\0%s\0' "$up" "$$"
for d in /proc/[0-9]*; do
  p=${d#/proc/}
  s=$(cat "$d/stat" 2>/dev/null) || continue
  c=$(tr '\000' '\001' < "$d/cmdline" 2>/dev/null)
  u=$(sed -n 's/^Uid:[[:space:]]*\([0-9]*\).*/\1/p' "$d/status" 2>/dev/null)
  printf '%s\0%s\0%s\0%s\0' "$p" "$s" "$c" "$u"
done"#;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Listing {
    /// Kernel threads too.
    #[serde(default)]
    all: bool,
}

/// One process, from its fields in the listing; `None` if they do not
/// parse, as for a process that ended while it was read.
fn process(pid: &str, stat: &str, cmdline: &str, uid: &str, uptime: f64) -> Option<Value> {
    // The name is between the first `(` and the last `)`, and may hold
    // either, or spaces.
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let command = stat.get(open + 1..close)?;
    let rest: Vec<&str> = stat.get(close + 1..)?.split_whitespace().collect();
    // `rest[0]` is field 3, state; so field n is `rest[n - 3]`.
    let field = |n: usize| rest.get(n - 3).copied();
    let number = |n: usize| field(n).and_then(|f| f.parse::<u64>().ok());
    let ticks = number(14)? + number(15)?;
    let started = number(22)? as f64 / TICKS;
    let page_kib = 4;
    let args = cmdline.trim_end_matches('\u{1}').replace('\u{1}', " ");
    Some(json!({
        "pid": pid.parse::<u32>().ok()?,
        "ppid": number(4)?,
        "uid": uid.parse::<u32>().ok(),
        "state": field(3)?,
        "command": command,
        "args": args,
        "threads": number(20)?,
        "rssKiB": number(24)? * page_kib,
        "cpuSeconds": ticks as f64 / TICKS,
        "startedSecondsAgo": ((uptime - started).max(0.0) * 100.0).round() / 100.0,
    }))
}

/// Whether a listed process is a kernel thread: the kernel's thread daemon,
/// PID 2, and everything it started. They have no command line.
fn kernel_thread(process: &Value) -> bool {
    process["pid"] == 2 || process["ppid"] == 2
}

/// The listing's output as processes, without the listing shell and what
/// it ran, and without kernel threads unless `all`.
fn parse(out: &str, all: bool) -> Result<Vec<Value>, String> {
    let fields: Vec<&str> = out.split('\0').collect();
    let (Some(uptime), Some(shell)) = (fields.first(), fields.get(1)) else {
        return Err("the guest's listing was empty".into());
    };
    let uptime: f64 = uptime
        .parse()
        .map_err(|_| format!("the guest's uptime {uptime:?}"))?;
    let shell: u64 = shell
        .parse()
        .map_err(|_| format!("the listing shell's PID {shell:?}"))?;
    let mut listed: Vec<Value> = fields[2..]
        .chunks(4)
        .filter(|chunk| chunk.len() == 4)
        .filter_map(|chunk| process(chunk[0], chunk[1], chunk[2], chunk[3], uptime))
        .filter(|p| p["pid"] != shell && p["ppid"] != shell)
        .filter(|p| all || !kernel_thread(p))
        .collect();
    listed.sort_by_key(|p| p["pid"].as_u64());
    listed.truncate(MAX_PROCESSES);
    Ok(listed)
}

/// The real UID in a `/proc/<pid>/status`, as the `sed` of [`LIST`] finds it.
fn status_uid(status: &str) -> String {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .filter(|uid| uid.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or("")
        .to_string()
}

/// A guest file as text, lossily; `None` if it could not be read (a process
/// that ended while it was read).
async fn read_text(vm: &AgentVM, path: &str) -> Option<String> {
    let bytes = vm
        .read_file_in_guest(path, 1024 * 1024, LIST_TIMEOUT)
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The listing through the agent's filesystem operations: no shell, `cat`,
/// `tr` or `sed` in the guest. `Err(None)` is an agent that predates them
/// (the caller may use a shell); `Err(Some(_))` a failure.
async fn list_by_agent(vm: &AgentVM, all: bool) -> Result<Vec<Value>, Option<String>> {
    let (entries, _) = match vm.list_dir_in_guest("/proc", 0, LIST_TIMEOUT).await {
        Ok(listed) => listed,
        Err(e) if hv2_agent::is_unsupported_op(&e) => return Err(None),
        Err(e) => return Err(Some(format!("listing the guest's processes: {e}"))),
    };
    let uptime = read_text(vm, "/proc/uptime")
        .await
        .and_then(|up| {
            up.split_whitespace()
                .next()
                .and_then(|n| n.parse::<f64>().ok())
        })
        .ok_or_else(|| Some("the guest's uptime could not be read".to_string()))?;
    let mut listed = Vec::new();
    for entry in entries {
        if entry.name.is_empty() || !entry.name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = format!("/proc/{}", entry.name);
        let (stat_at, cmdline_at, status_at) = (
            format!("{dir}/stat"),
            format!("{dir}/cmdline"),
            format!("{dir}/status"),
        );
        let (stat, cmdline, status) = tokio::join!(
            read_text(vm, &stat_at),
            read_text(vm, &cmdline_at),
            read_text(vm, &status_at),
        );
        // A process without a `stat` ended while it was read.
        let Some(stat) = stat else { continue };
        let cmdline = cmdline.unwrap_or_default().replace('\0', "\u{1}");
        let uid = status.map(|s| status_uid(&s)).unwrap_or_default();
        if let Some(p) = process(&entry.name, stat.trim_end(), &cmdline, &uid, uptime) {
            if all || !kernel_thread(&p) {
                listed.push(p);
            }
        }
    }
    listed.sort_by_key(|p| p["pid"].as_u64());
    listed.truncate(MAX_PROCESSES);
    Ok(listed)
}

/// The guest's processes, through its agent: its filesystem operations, or,
/// for an agent that predates them, one shell.
pub(crate) async fn list(vm: &AgentVM, query: &Listing) -> Result<Value, String> {
    match list_by_agent(vm, query.all).await {
        Ok(processes) => {
            tracing::debug!("listing processes: using agent op");
            return Ok(json!({ "processes": processes }));
        }
        Err(Some(e)) => return Err(e),
        Err(None) => tracing::debug!("listing processes: falling back to shell"),
    }
    let ran = vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".to_string(), LIST.to_string()],
            LIST_TIMEOUT,
        )
        .await
        .map_err(|e| format!("listing the guest's processes: {e}"))?;
    if ran.exit_code != Some(0) {
        return Err(format!(
            "listing the guest's processes ended with {:?}: {}",
            ran.exit_code,
            ran.stderr.trim()
        ));
    }
    let processes = parse(&ran.stdout, query.all)?;
    Ok(json!({ "processes": processes }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(pid: u32, name: &str, state: &str, ppid: u32) -> String {
        // Fields 3 to 24 after the name; 14 and 15 are 150 and 50 ticks, 20
        // is 3 threads, 22 starts at tick 1000, 24 is 256 pages.
        format!(
            "{pid} ({name}) {state} {ppid} 1 1 0 -1 4194304 100 0 0 0 150 50 0 0 20 0 3 0 1000 \
             12345678 256 18446744073709551615"
        )
    }

    fn listing(uptime: &str, shell: u32, procs: &[(u32, String, &str, &str)]) -> String {
        let mut out = format!("{uptime}\0{shell}\0");
        for (pid, stat, cmdline, uid) in procs {
            out.push_str(&format!("{pid}\0{stat}\0{cmdline}\0{uid}\0"));
        }
        out
    }

    #[test]
    fn a_process_is_read_from_its_stat() {
        let out = listing(
            "60.00",
            900,
            &[(
                42,
                stat(42, "my (odd) name", "S", 1),
                "python3\u{1}-m\u{1}http.server\u{1}",
                "1000",
            )],
        );
        let listed = parse(&out, false).unwrap();
        assert_eq!(listed.len(), 1);
        let p = &listed[0];
        assert_eq!(p["pid"], 42);
        assert_eq!(p["ppid"], 1);
        assert_eq!(p["uid"], 1000);
        assert_eq!(p["state"], "S");
        assert_eq!(p["command"], "my (odd) name");
        assert_eq!(p["args"], "python3 -m http.server");
        assert_eq!(p["threads"], 3);
        assert_eq!(p["rssKiB"], 1024);
        assert_eq!(p["cpuSeconds"], 2.0);
        assert_eq!(p["startedSecondsAgo"], 50.0);
    }

    #[test]
    fn the_uid_is_the_real_one_from_status() {
        assert_eq!(
            status_uid("Name:\tx\nUid:\t1000\t0\t0\t0\nGid:\t5\n"),
            "1000"
        );
        assert_eq!(status_uid("Name:\tx\n"), "");
        assert_eq!(status_uid("Uid:\tabc\n"), "");
    }

    #[test]
    fn a_nul_separated_cmdline_reads_as_the_shell_listing_gave_it() {
        let stat = stat(7, "sleep", "S", 1);
        let cmdline = "sleep\u{0}300\u{0}".replace('\0', "\u{1}");
        let p = process("7", &stat, &cmdline, "0", 5.0).unwrap();
        assert_eq!(p["args"], "sleep 300");
    }

    #[test]
    fn kernel_threads_and_the_listing_are_left_out() {
        let out = listing(
            "10.5",
            900,
            &[
                (1, stat(1, "init", "S", 0), "/sbin/init\u{1}", "0"),
                (2, stat(2, "kthreadd", "S", 0), "", "0"),
                (7, stat(7, "kworker/0:1", "I", 2), "", "0"),
                (900, stat(900, "sh", "S", 1), "/bin/sh\u{1}-c\u{1}", "0"),
                (901, stat(901, "cat", "R", 900), "cat\u{1}", "0"),
            ],
        );
        let pids = |all| -> Vec<u64> {
            parse(&out, all)
                .unwrap()
                .iter()
                .map(|p| p["pid"].as_u64().unwrap())
                .collect()
        };
        assert_eq!(pids(false), vec![1]);
        assert_eq!(pids(true), vec![1, 2, 7]);
    }

    #[test]
    fn a_process_that_ended_while_read_is_skipped() {
        let out = format!(
            "{}5\0garbage\0\0\0",
            listing("1.0", 900, &[(1, stat(1, "init", "S", 0), "", "0")])
        );
        assert_eq!(parse(&out, false).unwrap().len(), 1);
        assert!(parse("", false).is_err());
        assert!(parse("abc\0900\0", false).is_err());
    }
}
