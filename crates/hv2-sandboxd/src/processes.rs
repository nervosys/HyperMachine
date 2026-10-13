//! A guest's processes, listed from outside: `GET /machines/{name}/processes`
//! and `GET /sandboxes/{id}/processes`, as `ps` would list them inside.
//!
//! Every process the guest's kernel has, not only those started through the
//! API, read from its `/proc` by one shell through the guest agent. Only
//! `/proc` and a POSIX shell are used, so a guest of busybox lists as an
//! Ubuntu one does. Kernel threads are left out unless `?all=true`, and so
//! is the shell doing the listing, with what it runs.
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

/// The guest's processes, through its agent.
pub(crate) async fn list(vm: &AgentVM, query: &Listing) -> Result<Value, String> {
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
