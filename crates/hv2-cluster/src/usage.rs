//! Usage: how long sandboxes ran, and with how much, added up from the
//! lifecycle events a cluster keeps.
//!
//! `GET /usage?from=&to=&by=` answers with the running time, vCPU-seconds and
//! memory GiB-seconds of the caller's sandboxes in a window, in total and by
//! sandbox, team, template or owner.
//!
//! # How it is counted
//!
//! A sandbox runs from the event that starts it (created, forked, resumed)
//! to the event that stops it (paused, deleted, expired, lost). Each start
//! event carries what it runs with, in `hm_usage`; each stop event says
//! whether it was running when it stopped, so deleting a paused sandbox
//! stops nothing. A sandbox still running is counted to the moment asked.
//! Time paused is not counted: a paused sandbox holds a snapshot on disk,
//! not a processor or memory.
//!
//! # Kept for good
//!
//! The events are a bounded tail ([`crate::store::EVENT_TAIL`]). So that
//! usage outlives them, a store also keeps a **ledger**: as each event is
//! published, [`step`] folds it into the sandbox's open stretch, and a
//! stretch it closes is kept, with no limit. Usage since the ledger began
//! is read from it; usage before that, from the events, as far back as they
//! go. The answer's `coveredFrom` is the earliest either knows.
//!
//! # What it cannot know
//!
//! Before the ledger began, usage is only as old as the oldest event kept.
//! A sandbox already running when the ledger began, or whose start fell off
//! the tail, is counted from there, and the answer says it is `partial`. A
//! sandbox on a node that dies is counted as running until the node comes
//! back and records it lost.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::{rfc3339, ClusterEvent, SandboxRecord};
use crate::ownership::TeamId;

/// Event kinds that start a sandbox running. A resize starts it again at its
/// new size, which ends the stretch at the old one.
const STARTS: [&str; 4] = [
    "sandbox-created",
    "sandbox-forked",
    "sandbox-resumed",
    "sandbox-resized",
];
/// Event kinds that stop one.
const STOPS: [&str; 4] = [
    "sandbox-paused",
    "sandbox-deleted",
    "sandbox-expired",
    "sandbox-lost",
];

/// What an event of `kind` about `record` adds to the ledger, if anything:
/// for a start, what the sandbox runs with; for a stop, that too, and
/// whether it was running when it stopped.
#[must_use]
pub fn entry(kind: &str, record: &SandboxRecord) -> Option<Value> {
    let running_until_now = if STARTS.contains(&kind) {
        true
    } else if kind == "sandbox-paused" {
        // The record is already marked paused; it was running until now.
        true
    } else if STOPS.contains(&kind) {
        !record.paused
    } else {
        return None;
    };
    let mut entry = json!({
        "vcpuCount": record.cpu_count,
        "memoryMB": record.memory_mb,
        "running": running_until_now,
    });
    if let Some(owner) = &record.owner_id {
        entry["ownerId"] = json!(owner);
    }
    Some(entry)
}

/// How usage is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum By {
    #[default]
    Sandbox,
    Team,
    Template,
    Owner,
}

/// One stretch of running.
#[derive(Debug, Clone, PartialEq)]
struct Stretch {
    sandbox: String,
    team: Option<TeamId>,
    template: Option<String>,
    owner: Option<String>,
    vcpus: u64,
    memory_mb: u64,
    from_ms: u64,
    to_ms: u64,
    /// Its start was not among the events kept, so it is counted from the
    /// oldest one: it ran at least this long.
    partial: bool,
}

/// What a sandbox is doing, as the events read so far say.
struct Open {
    team: Option<TeamId>,
    template: Option<String>,
    owner: Option<String>,
    vcpus: u64,
    memory_mb: u64,
    since_ms: u64,
}

fn resources(event: &ClusterEvent) -> Option<(u64, u64, Option<String>, bool)> {
    let usage = event.data.as_ref()?.get("hm_usage")?;
    Some((
        usage.get("vcpuCount")?.as_u64()?,
        usage.get("memoryMB")?.as_u64()?,
        usage
            .get("ownerId")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage.get("running")?.as_bool()?,
    ))
}

/// A stretch of running kept in a store as it happens, so that usage
/// outlives the event tail. `to_ms` is `None` while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kept {
    pub sandbox: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub vcpus: u64,
    pub memory_mb: u64,
    pub from_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_ms: Option<u64>,
    /// Running since before the ledger began: counted from then.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// What a store's ledger holds: when it began, and the stretches in it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// When the first event was folded into it; `None` until then.
    pub since_ms: Option<u64>,
    /// Every stretch still running, and every closed one asked for.
    pub stretches: Vec<Kept>,
}

/// What `event` changes in the ledger for its sandbox, whose stretch open
/// before it is `open`: the stretch open after it, if any, and a stretch it
/// closed, if any. `None` when it changes nothing. `since_ms` is when the
/// ledger began, which a stretch whose start it never saw is counted from.
///
/// The same rules as the events are read by: a start ends what was running,
/// and a stop of something running whose start was not seen is `partial`.
#[must_use]
pub fn step(
    open: Option<&Kept>,
    event: &ClusterEvent,
    since_ms: u64,
) -> Option<(Option<Kept>, Option<Kept>)> {
    let sandbox = event.sandbox_id.as_deref()?;
    let kind = event.kind.as_str();
    let close = |was: &Kept| Kept {
        to_ms: Some(event.at_ms.max(was.from_ms)),
        ..was.clone()
    };
    if STARTS.contains(&kind) {
        let closed = open.map(close);
        let opened = resources(event).map(|(vcpus, memory_mb, owner, _)| Kept {
            sandbox: sandbox.to_string(),
            team: event.team_id.clone(),
            template: event.template_id.clone(),
            owner,
            vcpus,
            memory_mb,
            from_ms: event.at_ms,
            to_ms: None,
            partial: false,
        });
        (closed.is_some() || opened.is_some()).then_some((opened, closed))
    } else if STOPS.contains(&kind) {
        if let Some(was) = open {
            return Some((None, Some(close(was))));
        }
        match resources(event) {
            Some((vcpus, memory_mb, owner, true)) => Some((
                None,
                Some(Kept {
                    sandbox: sandbox.to_string(),
                    team: event.team_id.clone(),
                    template: event.template_id.clone(),
                    owner,
                    vcpus,
                    memory_mb,
                    from_ms: since_ms.min(event.at_ms),
                    to_ms: Some(event.at_ms),
                    partial: true,
                }),
            )),
            _ => None,
        }
    } else {
        None
    }
}

impl Kept {
    fn stretch(&self, now_ms: u64) -> Stretch {
        Stretch {
            sandbox: self.sandbox.clone(),
            team: self.team.clone(),
            template: self.template.clone(),
            owner: self.owner.clone(),
            vcpus: self.vcpus,
            memory_mb: self.memory_mb,
            from_ms: self.from_ms,
            to_ms: self.to_ms.unwrap_or(now_ms).max(self.from_ms),
            partial: self.partial,
        }
    }
}

/// The stretches `events` (oldest first) describe, up to `now_ms`.
fn stretches(events: &[&ClusterEvent], now_ms: u64) -> Vec<Stretch> {
    let covered_from = events.first().map_or(now_ms, |e| e.at_ms);
    let mut open: HashMap<&str, Open> = HashMap::new();
    let mut done = Vec::new();
    let close = |sandbox: &str, open: Open, to_ms: u64, partial: bool| Stretch {
        sandbox: sandbox.to_string(),
        team: open.team,
        template: open.template,
        owner: open.owner,
        vcpus: open.vcpus,
        memory_mb: open.memory_mb,
        from_ms: open.since_ms,
        to_ms: to_ms.max(open.since_ms),
        partial,
    };
    for event in events {
        let Some(sandbox) = event.sandbox_id.as_deref() else {
            continue;
        };
        let kind = event.kind.as_str();
        if STARTS.contains(&kind) {
            // A start while it was already running ends what was running:
            // nothing runs twice.
            if let Some(was) = open.remove(sandbox) {
                done.push(close(sandbox, was, event.at_ms, false));
            }
            if let Some((vcpus, memory_mb, owner, _)) = resources(event) {
                open.insert(
                    sandbox,
                    Open {
                        team: event.team_id.clone(),
                        template: event.template_id.clone(),
                        owner,
                        vcpus,
                        memory_mb,
                        since_ms: event.at_ms,
                    },
                );
            }
        } else if STOPS.contains(&kind) {
            if let Some(was) = open.remove(sandbox) {
                done.push(close(sandbox, was, event.at_ms, false));
            } else if let Some((vcpus, memory_mb, owner, true)) = resources(event) {
                // Running since before the oldest event kept.
                let started = Open {
                    team: event.team_id.clone(),
                    template: event.template_id.clone(),
                    owner,
                    vcpus,
                    memory_mb,
                    since_ms: covered_from,
                };
                done.push(close(sandbox, started, event.at_ms, true));
            }
        }
    }
    for (sandbox, still) in open {
        done.push(close(sandbox, still, now_ms, false));
    }
    done
}

/// Running time, vCPU-seconds and GiB-seconds of `stretches` within
/// `[from_ms, to_ms]`, in total and by `by`.
fn tally(stretches: &[Stretch], from_ms: u64, to_ms: u64, by: By) -> (Value, Vec<Value>) {
    #[derive(Default)]
    struct Sum {
        sandboxes: std::collections::BTreeSet<String>,
        ms: u64,
        vcpu_ms: u128,
        mib_ms: u128,
        partial: bool,
    }
    impl Sum {
        fn add(&mut self, stretch: &Stretch, ms: u64) {
            self.sandboxes.insert(stretch.sandbox.clone());
            self.ms += ms;
            self.vcpu_ms += u128::from(stretch.vcpus) * u128::from(ms);
            self.mib_ms += u128::from(stretch.memory_mb) * u128::from(ms);
            self.partial |= stretch.partial;
        }
        fn json(&self) -> Value {
            json!({
                "sandboxes": self.sandboxes.len(),
                "runningSeconds": self.ms as f64 / 1e3,
                "vcpuSeconds": self.vcpu_ms as f64 / 1e3,
                "memoryGiBSeconds": self.mib_ms as f64 / 1024.0 / 1e3,
                "partial": self.partial,
            })
        }
    }
    let mut total = Sum::default();
    let mut groups: BTreeMap<Option<String>, Sum> = BTreeMap::new();
    for stretch in stretches {
        let ms = stretch
            .to_ms
            .min(to_ms)
            .saturating_sub(stretch.from_ms.max(from_ms));
        if ms == 0 {
            continue;
        }
        total.add(stretch, ms);
        let key = match by {
            By::Sandbox => Some(stretch.sandbox.clone()),
            By::Team => stretch.team.as_ref().map(|t| t.as_str().to_string()),
            By::Template => stretch.template.clone(),
            By::Owner => stretch.owner.clone(),
        };
        groups.entry(key).or_default().add(stretch, ms);
    }
    let groups = groups
        .into_iter()
        .map(|(key, sum)| {
            let mut group = sum.json();
            group["key"] = json!(key);
            group
        })
        .collect();
    (total.json(), groups)
}

/// The answer to `GET /usage`: `events` newest first, as a store gives
/// them, every team's; only the stretches of a team `sees` admits count.
///
/// Every team's events are read because some close a stretch and name no
/// team: a sandbox lost with its node is recorded without its record.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn report(
    events: &[ClusterEvent],
    truncated: bool,
    ledger: Option<&Ledger>,
    from_ms: u64,
    to_ms: u64,
    now_ms: u64,
    by: By,
    sees: impl Fn(Option<&TeamId>) -> bool,
) -> Value {
    let oldest_first: Vec<&ClusterEvent> = events.iter().rev().collect();
    let events_from = oldest_first.first().map(|e| e.at_ms);
    let from_events = stretches(&oldest_first, now_ms);
    let (all, covered_from, truncated) = match ledger.and_then(|l| l.since_ms.map(|s| (l, s))) {
        // The ledger from when it began; the events only for before that.
        Some((ledger, since)) => {
            let mut all: Vec<Stretch> = from_events
                .into_iter()
                .filter(|s| s.from_ms < since)
                .map(|s| Stretch {
                    to_ms: s.to_ms.min(since),
                    ..s
                })
                .collect();
            all.extend(ledger.stretches.iter().map(|k| k.stretch(now_ms)));
            let covered = events_from.map_or(since, |e| e.min(since));
            // Dropped events matter only before the ledger began.
            (
                all,
                Some(covered),
                truncated && events_from.is_some_and(|e| e < since),
            )
        }
        None => (from_events, events_from, truncated),
    };
    let all: Vec<Stretch> = all
        .into_iter()
        .filter(|stretch| sees(stretch.team.as_ref()))
        .collect();
    let (total, groups) = tally(&all, from_ms, to_ms.min(now_ms), by);
    json!({
        "from": rfc3339(from_ms),
        "to": rfc3339(to_ms),
        "by": format!("{by:?}").to_lowercase(),
        // Usage before this is not known: the events were not kept.
        "coveredFrom": covered_from.map(rfc3339),
        // Whether events had been dropped from the tail when this was read,
        // so the window may begin before what is known.
        "truncated": truncated,
        "total": total,
        "groups": groups,
    })
}

/// A time as a query gives it: RFC 3339, or milliseconds since the epoch.
///
/// # Errors
///
/// Neither.
pub fn parse_time(text: &str) -> Result<u64, String> {
    if let Ok(ms) = text.trim().parse::<u64>() {
        return Ok(ms);
    }
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .map_err(|e| format!("{text:?}: RFC 3339 or milliseconds since the epoch ({e})"))
        .and_then(|t| {
            u64::try_from(t.timestamp_millis()).map_err(|_| format!("{text:?} is before 1970"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn everyone(_: Option<&TeamId>) -> bool {
        true
    }

    fn record(paused: bool) -> SandboxRecord {
        serde_json::from_value(json!({
            "sandbox_id": "s1", "owner_id": "key-a", "team_id": "team-a", "node_id": "n",
            "template_id": "base", "started_at_ms": 0, "end_at_ms": 0, "cpu_count": 2,
            "memory_mb": 1024, "envd_version": "", "descriptor": {}, "paused": paused,
        }))
        .unwrap()
    }

    fn event(
        kind: &str,
        sandbox: &str,
        at_ms: u64,
        team: &str,
        usage: Option<Value>,
    ) -> ClusterEvent {
        let mut event = ClusterEvent::new(kind, "n", Some(sandbox));
        event.at_ms = at_ms;
        event.team_id = Some(TeamId::parse(team).unwrap());
        event.template_id = Some("base".into());
        event.data = usage.map(|u| json!({ "hm_usage": u }));
        event
    }

    fn running(vcpus: u64, mib: u64, running: bool) -> Option<Value> {
        Some(json!({"vcpuCount": vcpus, "memoryMB": mib, "running": running, "ownerId": "key-a"}))
    }

    /// Newest first, as a store gives them.
    fn newest_first(mut events: Vec<ClusterEvent>) -> Vec<ClusterEvent> {
        events.reverse();
        events
    }

    #[test]
    fn a_ledger_entry_says_what_it_runs_with_and_whether_it_was_running() {
        let entry = entry("sandbox-created", &record(false)).unwrap();
        assert_eq!(
            entry,
            json!({"vcpuCount": 2, "memoryMB": 1024, "running": true, "ownerId": "key-a"})
        );
        assert_eq!(entry_of("sandbox-paused", true)["running"], true);
        assert_eq!(entry_of("sandbox-deleted", false)["running"], true);
        // Deleted while paused: it was not running.
        assert_eq!(entry_of("sandbox-deleted", true)["running"], false);
        assert!(super::entry("sandbox-updated", &record(false)).is_none());
        assert!(super::entry("node-joined", &record(false)).is_none());
    }

    fn entry_of(kind: &str, paused: bool) -> Value {
        entry(kind, &record(paused)).unwrap()
    }

    #[test]
    fn running_is_counted_from_start_to_stop_and_paused_time_is_not() {
        let events = newest_first(vec![
            event(
                "sandbox-created",
                "s1",
                1_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-paused",
                "s1",
                11_000,
                "team-a",
                running(2, 1024, true),
            ),
            // Ten seconds paused, then resumed for five, then deleted.
            event(
                "sandbox-resumed",
                "s1",
                21_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-deleted",
                "s1",
                26_000,
                "team-a",
                running(2, 1024, true),
            ),
            // Another, deleted while paused: the delete stops nothing more.
            event(
                "sandbox-created",
                "s2",
                2_000,
                "team-b",
                running(1, 512, true),
            ),
            event(
                "sandbox-paused",
                "s2",
                4_000,
                "team-b",
                running(1, 512, true),
            ),
            event(
                "sandbox-deleted",
                "s2",
                30_000,
                "team-b",
                running(1, 512, false),
            ),
        ]);
        let mut sorted = events.clone();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.at_ms));
        let answer = report(
            &sorted,
            false,
            None,
            0,
            100_000,
            100_000,
            By::Sandbox,
            everyone,
        );
        assert_eq!(answer["total"]["sandboxes"], 2);
        // s1: 10 s + 5 s; s2: 2 s.
        assert_eq!(answer["total"]["runningSeconds"], 17.0);
        assert_eq!(answer["total"]["vcpuSeconds"], 32.0);
        assert_eq!(answer["total"]["memoryGiBSeconds"], 16.0);
        assert_eq!(answer["total"]["partial"], false);
        let groups = answer["groups"].as_array().unwrap();
        assert_eq!(groups[0]["key"], "s1");
        assert_eq!(groups[0]["runningSeconds"], 15.0);
        assert_eq!(groups[1]["key"], "s2");
        assert_eq!(groups[1]["runningSeconds"], 2.0);
        assert_eq!(answer["coveredFrom"], rfc3339(1_000));
        let _ = events;
    }

    #[test]
    fn a_window_cuts_stretches_and_one_still_running_counts_to_now() {
        let events = newest_first(vec![
            event("sandbox-created", "s1", 0, "team-a", running(4, 2048, true)),
            event(
                "sandbox-deleted",
                "s1",
                60_000,
                "team-a",
                running(4, 2048, true),
            ),
            event(
                "sandbox-created",
                "s2",
                50_000,
                "team-a",
                running(1, 1024, true),
            ),
        ]);
        // 30 s to 90 s, asked at 70 s: s1 for its last 30 s, s2 from 50 s to now.
        let answer = report(
            &events,
            false,
            None,
            30_000,
            90_000,
            70_000,
            By::Team,
            everyone,
        );
        assert_eq!(answer["total"]["runningSeconds"], 50.0);
        assert_eq!(answer["total"]["vcpuSeconds"], 4.0 * 30.0 + 20.0);
        let groups = answer["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["key"], "team-a");
        assert_eq!(groups[0]["sandboxes"], 2);
        // Nothing in a window before it all.
        let none = report(&events, false, None, 0, 0, 70_000, By::Sandbox, everyone);
        assert_eq!(none["total"]["sandboxes"], 0);
        assert_eq!(none["groups"], json!([]));
    }

    #[test]
    fn a_stop_whose_start_was_not_kept_counts_from_the_oldest_event_and_says_so() {
        let events = newest_first(vec![
            event(
                "sandbox-created",
                "s2",
                10_000,
                "team-a",
                running(1, 1024, true),
            ),
            event(
                "sandbox-deleted",
                "s1",
                40_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-deleted",
                "s2",
                20_000,
                "team-a",
                running(1, 1024, true),
            ),
            // A stop of one that was paused, start unknown: nothing.
            event(
                "sandbox-deleted",
                "s3",
                30_000,
                "team-a",
                running(8, 8192, false),
            ),
        ]);
        let mut sorted = events;
        sorted.sort_by_key(|e| std::cmp::Reverse(e.at_ms));
        let answer = report(
            &sorted,
            true,
            None,
            0,
            100_000,
            100_000,
            By::Sandbox,
            everyone,
        );
        let groups = answer["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(groups[0]["key"], "s1");
        assert_eq!(groups[0]["runningSeconds"], 30.0);
        assert_eq!(groups[0]["partial"], true);
        assert_eq!(groups[1]["partial"], false);
        assert_eq!(answer["total"]["partial"], true);
        assert_eq!(answer["truncated"], true);
    }

    #[test]
    fn a_lost_sandbox_stops_and_one_started_twice_does_not_run_twice() {
        let mut lost = ClusterEvent::new("sandbox-lost", "n", Some("s1"));
        lost.at_ms = 9_000;
        let mut sorted = vec![
            event(
                "sandbox-created",
                "s1",
                1_000,
                "team-a",
                running(1, 1024, true),
            ),
            event(
                "sandbox-created",
                "s1",
                3_000,
                "team-a",
                running(1, 1024, true),
            ),
            lost,
        ];
        sorted.sort_by_key(|e| std::cmp::Reverse(e.at_ms));
        let team_a = TeamId::parse("team-a").unwrap();
        let only_a = |team: Option<&TeamId>| team == Some(&team_a);
        // The lost event names no team, and still ends team A's stretch.
        let answer = report(&sorted, false, None, 0, 100_000, 100_000, By::Owner, only_a);
        assert_eq!(answer["total"]["runningSeconds"], 8.0);
        assert_eq!(answer["groups"][0]["key"], "key-a");
        let only_b = |team: Option<&TeamId>| team.is_some_and(|t| t.as_str() == "team-b");
        let answer = report(&sorted, false, None, 0, 100_000, 100_000, By::Owner, only_b);
        assert_eq!(answer["total"]["sandboxes"], 0);
    }

    #[test]
    fn a_resize_ends_the_old_size_and_counts_the_new_one() {
        let mut sorted = vec![
            event("sandbox-created", "s1", 0, "team-a", running(1, 1024, true)),
            event(
                "sandbox-resized",
                "s1",
                10_000,
                "team-a",
                running(4, 4096, true),
            ),
            event(
                "sandbox-deleted",
                "s1",
                15_000,
                "team-a",
                running(4, 4096, true),
            ),
        ];
        sorted.sort_by_key(|e| std::cmp::Reverse(e.at_ms));
        let answer = report(
            &sorted,
            false,
            None,
            0,
            100_000,
            100_000,
            By::Sandbox,
            everyone,
        );
        // 10 s at one vCPU and 1 GiB, then 5 s at four and 4 GiB.
        assert_eq!(answer["total"]["runningSeconds"], 15.0);
        assert_eq!(answer["total"]["vcpuSeconds"], 30.0);
        assert_eq!(answer["total"]["memoryGiBSeconds"], 30.0);
        assert!(super::entry("sandbox-resized", &record(false)).is_some());
    }

    #[test]
    fn times_are_rfc3339_or_milliseconds() {
        assert_eq!(parse_time("1700000000000"), Ok(1_700_000_000_000));
        assert_eq!(parse_time("2023-11-14T22:13:20Z"), Ok(1_700_000_000_000));
        assert_eq!(
            parse_time("2023-11-14T23:13:20.5+01:00"),
            Ok(1_700_000_000_500)
        );
        assert!(parse_time("yesterday").is_err());
        assert!(parse_time("1969-12-31T00:00:00Z").is_err());
    }

    /// Fold `events` (oldest first) as a store does, into a ledger.
    fn fold(events: &[ClusterEvent]) -> Ledger {
        let mut since = None;
        let mut open: HashMap<String, Kept> = HashMap::new();
        let mut closed = Vec::new();
        for event in events {
            let begun = since.unwrap_or(event.at_ms);
            let Some(sandbox) = event.sandbox_id.clone() else {
                continue;
            };
            if let Some((opened, done)) = step(open.get(&sandbox), event, begun) {
                since = Some(begun);
                match opened {
                    Some(k) => {
                        open.insert(sandbox, k);
                    }
                    None => {
                        open.remove(&sandbox);
                    }
                }
                closed.extend(done);
            }
        }
        closed.extend(open.into_values());
        Ledger {
            since_ms: since,
            stretches: closed,
        }
    }

    fn running_seconds(answer: &Value) -> f64 {
        answer["total"]["runningSeconds"].as_f64().unwrap()
    }

    #[test]
    fn the_ledger_counts_what_the_events_do() {
        // Created at 2 vCPUs, paused, resumed, resized to 4, deleted.
        let events = vec![
            event(
                "sandbox-created",
                "s1",
                10_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-paused",
                "s1",
                20_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-resumed",
                "s1",
                30_000,
                "team-a",
                running(2, 1024, true),
            ),
            event(
                "sandbox-resized",
                "s1",
                40_000,
                "team-a",
                running(4, 2048, true),
            ),
            event(
                "sandbox-deleted",
                "s1",
                50_000,
                "team-a",
                running(4, 2048, true),
            ),
        ];
        let newest_first: Vec<ClusterEvent> = events.iter().rev().cloned().collect();
        let ledger = fold(&events);
        assert_eq!(ledger.since_ms, Some(10_000));
        let from_events = report(
            &newest_first,
            false,
            None,
            0,
            60_000,
            60_000,
            By::Sandbox,
            everyone,
        );
        let from_ledger = report(
            &[],
            false,
            Some(&ledger),
            0,
            60_000,
            60_000,
            By::Sandbox,
            everyone,
        );
        assert_eq!(from_events["total"], from_ledger["total"]);
        // 10 s at 2 vCPUs, 10 s at 2 after the resume, 10 s at 4.
        assert_eq!(running_seconds(&from_ledger), 30.0);
        assert_eq!(from_ledger["total"]["vcpuSeconds"].as_f64(), Some(80.0));
    }

    #[test]
    fn a_stretch_running_when_the_ledger_began_is_counted_from_then() {
        let stop = event(
            "sandbox-deleted",
            "s1",
            9_000,
            "team-a",
            running(2, 1024, true),
        );
        let (opened, closed) = step(None, &stop, 5_000).unwrap();
        assert!(opened.is_none());
        let closed = closed.unwrap();
        assert_eq!(
            (closed.from_ms, closed.to_ms, closed.partial),
            (5_000, Some(9_000), true)
        );
        // A paused sandbox deleted ran nothing.
        let deleted = event(
            "sandbox-deleted",
            "s2",
            9_000,
            "team-a",
            running(2, 1024, false),
        );
        assert!(step(None, &deleted, 5_000).is_none());
    }

    #[test]
    fn the_events_before_the_ledger_and_the_ledger_after_are_not_counted_twice() {
        // Created before the ledger began, deleted after: the events say
        // 1 s to 9 s, the ledger 5 s to 9 s.
        let created = event(
            "sandbox-created",
            "s1",
            1_000,
            "team-a",
            running(1, 512, true),
        );
        let deleted = event(
            "sandbox-deleted",
            "s1",
            9_000,
            "team-a",
            running(1, 512, true),
        );
        let ledger = fold(std::slice::from_ref(&deleted));
        assert_eq!(ledger.since_ms, Some(9_000));
        let ledger = Ledger {
            since_ms: Some(5_000),
            stretches: vec![Kept {
                from_ms: 5_000,
                ..ledger.stretches[0].clone()
            }],
        };
        let answer = report(
            &[deleted, created],
            false,
            Some(&ledger),
            0,
            20_000,
            20_000,
            By::Sandbox,
            everyone,
        );
        assert_eq!(running_seconds(&answer), 8.0);
        assert_eq!(answer["coveredFrom"], rfc3339(1_000));
    }

    #[tokio::test]
    async fn usage_outlives_the_event_tail() {
        use crate::store::{ClusterStore, MemoryStore, EVENT_TAIL};
        let store = MemoryStore::new();
        store
            .publish(&event(
                "sandbox-created",
                "s1",
                10_000,
                "team-a",
                running(2, 1024, true),
            ))
            .await
            .unwrap();
        store
            .publish(&event(
                "sandbox-deleted",
                "s1",
                20_000,
                "team-a",
                running(2, 1024, true),
            ))
            .await
            .unwrap();
        // As many events again as are kept: the two above fall off.
        for _ in 0..EVENT_TAIL {
            let mut other = ClusterEvent::new("node-joined", "n", None);
            other.at_ms = 30_000;
            store.publish(&other).await.unwrap();
        }
        let events = store.events(EVENT_TAIL).await.unwrap();
        assert!(events.iter().all(|e| e.sandbox_id.is_none()));
        let ledger = store.usage_ledger(0).await.unwrap();
        let kept = report(
            &events,
            true,
            ledger.as_ref(),
            0,
            40_000,
            40_000,
            By::Sandbox,
            everyone,
        );
        assert_eq!(running_seconds(&kept), 10.0);
        assert_eq!(kept["truncated"], false);
        let lost = report(
            &events,
            true,
            None,
            0,
            40_000,
            40_000,
            By::Sandbox,
            everyone,
        );
        assert_eq!(running_seconds(&lost), 0.0);
    }
}
