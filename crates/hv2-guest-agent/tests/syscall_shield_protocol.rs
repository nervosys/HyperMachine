//! The syscall-shield and hook-shield operations on the wire: round trips,
//! defaults that keep a newer peer readable, and what an agent that predates
//! them does.

use hv2_guest_agent::{
    decode, encode, GuestArgMatch, GuestArgOp, GuestHit, GuestHookHit, GuestHookRule,
    GuestSyscallAction, GuestSyscallRule, OpResult, Operation, Request, Response, PROTOCOL_VERSION,
};
use serde::Deserialize;

fn rule() -> GuestSyscallRule {
    GuestSyscallRule {
        shield: "cve-2026-xxxx-unshare".into(),
        digest: "ab".repeat(32),
        syscall: "unshare".into(),
        args: vec![GuestArgMatch {
            index: 0,
            op: GuestArgOp::AnyBits,
            value: 0x1000_0000,
            mask: u64::MAX,
        }],
        action: GuestSyscallAction::Errno { errno: 1 },
        enforce: true,
        expires_at: 4_102_444_800,
    }
}

#[test]
fn set_syscall_shields_round_trips_with_the_names_the_host_and_agent_agree_on() {
    let request = Request {
        id: 9,
        version: PROTOCOL_VERSION,
        op: Operation::SetSyscallShields {
            rules: vec![
                rule(),
                GuestSyscallRule {
                    action: GuestSyscallAction::Kill,
                    args: vec![],
                    enforce: false,
                    expires_at: 0,
                    ..rule()
                },
            ],
        },
    };
    let bytes = encode(&request).unwrap();
    let (back, used) = decode::<Request>(&bytes).unwrap().unwrap();
    assert_eq!(used, bytes.len());
    assert_eq!(back, request);

    let json: serde_json::Value = serde_json::to_value(&request.op).unwrap();
    assert_eq!(json["kind"], "set_syscall_shields");
    assert_eq!(json["rules"][0]["args"][0]["op"], "any_bits");
    assert_eq!(json["rules"][0]["action"]["kind"], "errno");
    assert_eq!(json["rules"][1]["action"]["kind"], "kill");
    assert!(
        json["rules"][1].get("expires_at").is_none(),
        "0 is not sent"
    );

    let hits = Operation::SyscallShieldHits;
    assert_eq!(
        serde_json::to_value(&hits).unwrap(),
        serde_json::json!({"kind": "syscall_shield_hits"})
    );
}

#[test]
fn the_hits_answer_round_trips() {
    let response = Response {
        id: 9,
        version: PROTOCOL_VERSION,
        result: OpResult::SyscallShieldHits {
            hits: vec![GuestHit {
                at: 1_800_000_000,
                shield: "s".into(),
                digest: "d".into(),
                syscall: "unshare".into(),
                args: [0x1000_0000, 0, 0, 0, u64::MAX, 6],
                enforced: true,
                pid: 4242,
            }],
            dropped: 17,
        },
    };
    let bytes = encode(&response).unwrap();
    let (back, _) = decode::<Response>(&bytes).unwrap().unwrap();
    assert_eq!(back, response, "u64::MAX survives: no float in the path");
}

#[test]
fn a_rule_sent_by_a_host_that_knows_fewer_fields_still_reads() {
    // No `args`, `enforce`, `expires_at`, and an argument with no `mask`.
    let json = r#"{"kind":"set_syscall_shields","rules":[
        {"shield":"s","digest":"d","syscall":"ptrace","action":{"kind":"kill"}},
        {"shield":"s","digest":"d","syscall":"unshare","action":{"kind":"errno","errno":1},
         "args":[{"index":0,"op":"eq","value":3}]}]}"#;
    let Operation::SetSyscallShields { rules } = serde_json::from_str(json).unwrap() else {
        panic!("not the operation");
    };
    assert!(!rules[0].enforce, "absent means monitored, never enforced");
    assert_eq!(rules[0].expires_at, 0);
    assert_eq!(rules[1].args[0].mask, u64::MAX);
}

/// What an agent built before these operations decodes: the operations it
/// knows, tagged the same way. (The real old `Operation` is this crate at an
/// older revision; its derive produces exactly this failure for an unknown
/// tag.)
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OldOperation {
    Ping,
    Stats,
}

#[derive(Debug, Deserialize)]
struct OldRequest {
    #[allow(dead_code)]
    id: u64,
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    op: OldOperation,
}

#[test]
fn an_old_agent_cannot_parse_the_new_operations_so_it_hangs_up_without_answering() {
    // agentd's `serve` does `decode::<Request>(&buf).map_err(..)?` and returns,
    // closing the connection: the host's client reads that as "closed before it
    // answered" -> `is_unsupported_op`. This test pins the first half: the
    // request is an *error* for an old reader, not a silently different op.
    for op in [
        Operation::SetSyscallShields {
            rules: vec![rule()],
        },
        Operation::SyscallShieldHits,
        Operation::SetHookShields {
            rules: vec![hook_rule()],
        },
        Operation::HookShieldHits,
    ] {
        let bytes = encode(&Request {
            id: 1,
            version: PROTOCOL_VERSION,
            op,
        })
        .unwrap();
        let old = decode::<OldRequest>(&bytes);
        assert!(old.is_err(), "an old agent must fail to parse, got {old:?}");
    }
    // And the operations an old agent knows still parse in a new one.
    let bytes = encode(&Request {
        id: 2,
        version: PROTOCOL_VERSION,
        op: Operation::Ping,
    })
    .unwrap();
    assert!(decode::<OldRequest>(&bytes).unwrap().is_some());
}

fn hook_rule() -> GuestHookRule {
    GuestHookRule {
        shield: "no-mount".into(),
        digest: "cd".repeat(32),
        hook: "sb_mount".into(),
        errno: 1,
        enforce: true,
        expires_at: 4_102_444_800,
    }
}

#[test]
fn set_hook_shields_round_trips_with_the_names_the_host_and_agent_agree_on() {
    let request = Request {
        id: 11,
        version: PROTOCOL_VERSION,
        op: Operation::SetHookShields {
            rules: vec![
                hook_rule(),
                GuestHookRule {
                    enforce: false,
                    expires_at: 0,
                    ..hook_rule()
                },
            ],
        },
    };
    let bytes = encode(&request).unwrap();
    let (back, used) = decode::<Request>(&bytes).unwrap().unwrap();
    assert_eq!(used, bytes.len());
    assert_eq!(back, request);

    let json: serde_json::Value = serde_json::to_value(&request.op).unwrap();
    assert_eq!(json["kind"], "set_hook_shields");
    assert_eq!(json["rules"][0]["hook"], "sb_mount");
    assert_eq!(json["rules"][0]["errno"], 1);
    assert!(
        json["rules"][1].get("expires_at").is_none(),
        "0 is not sent"
    );

    assert_eq!(
        serde_json::to_value(&Operation::HookShieldHits).unwrap(),
        serde_json::json!({"kind": "hook_shield_hits"})
    );
}

#[test]
fn the_hook_hits_answer_round_trips() {
    let response = Response {
        id: 11,
        version: PROTOCOL_VERSION,
        result: OpResult::HookShieldHits {
            hits: vec![GuestHookHit {
                at: 1_800_000_000,
                shield: "s".into(),
                digest: "d".into(),
                hook: "sb_mount".into(),
                enforced: true,
                count: u64::MAX,
            }],
            dropped: 17,
        },
    };
    let bytes = encode(&response).unwrap();
    let (back, _) = decode::<Response>(&bytes).unwrap().unwrap();
    assert_eq!(back, response, "u64::MAX survives: no float in the path");
}

#[test]
fn a_hook_rule_sent_by_a_host_that_knows_fewer_fields_still_reads() {
    // No `enforce`, no `expires_at`.
    let json = r#"{"kind":"set_hook_shields","rules":[
        {"shield":"s","digest":"d","hook":"sb_mount","errno":1}]}"#;
    let Operation::SetHookShields { rules } = serde_json::from_str(json).unwrap() else {
        panic!("not the operation");
    };
    assert!(!rules[0].enforce, "absent means monitored, never enforced");
    assert_eq!(rules[0].expires_at, 0);
}
