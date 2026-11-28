//! A node's shields: signed rules an operator lays over running sandboxes'
//! networks without restarting them, as the node (`hv2-sandboxd`:
//! `shields.rs`, over the `hv2-shield` crate) serves them. The control plane
//! does not route `/shields`. Modelled as data: `hv2-api` depends on neither
//! crate.
//!
//! The routes split by who may use them. Reading (`list_shields`,
//! `list_shield_hits`, `list_shield_events`) and proposing
//! (`propose_shield`, `withdraw_shield_proposal`) change nothing in force and
//! are for an agent. Everything that puts a shield in force, changes its
//! mode, removes it, changes who is trusted to sign, or signs for the node
//! (`load_shield`, `apply_shield_directive`, `reload_shield_keyring`,
//! `attest_shields`) is the node operator's: it needs a signature from a key
//! in the node's keyring, or the keyring file only the operator writes. The
//! descriptions say which each is.

use super::spec::{body_rejection, capability, fail, ok, path_param, Spec};
use super::{Capability, HyperMachineOntology, Operation, OperationResponse};
use serde_json::{json, Value};

/// A shield as the node keeps it.
fn entry_schema() -> Value {
    json!({ "type": "object", "properties": {
        "digest": { "type": "string", "description": "SHA-256 of `specJson`, 64 lowercase hex digits; it names the shield everywhere" },
        "specJson": { "type": "string", "description": "the shield's JSON exactly as proposed or signed" },
        "signed": { "type": ["object", "null"], "description": "the SignedShield it was loaded from; absent for a proposal",
            "properties": {
                "spec": { "type": "string" },
                "signer": { "type": "string", "description": "the signer's key ID" },
                "signature": { "type": "string" } } },
        "state": { "type": "string", "enum": ["proposed", "monitor", "enforce", "removed", "expired", "revoked"] },
        "proposedBy": { "type": ["string", "null"], "description": "what the proposer called itself; not checked" },
        "loadedBy": { "type": ["string", "null"] },
        "loadedAt": { "type": ["integer", "null"], "description": "unix seconds" },
        "enforcedBy": { "type": ["string", "null"], "description": "the key ID of the directive that put it in force" },
        "expiresAt": { "type": "integer", "description": "unix seconds: its own expiry, or 30 days after it was loaded, whichever is sooner" },
        "changedAt": { "type": "integer", "description": "unix seconds" } } })
}

/// One change in the measured log.
fn event_schema() -> Value {
    json!({ "type": "object", "properties": {
        "seq": { "type": "integer", "description": "from 1, with no gaps" },
        "at": { "type": "integer", "description": "unix seconds" },
        "kind": { "type": "string", "enum": ["load", "enforce", "monitor", "unload", "expire", "revoke"] },
        "shield": { "type": "string", "description": "the shield's name" },
        "digest": { "type": "string" },
        "signer": { "type": "string", "description": "the signer's key ID" },
        "by": { "type": ["string", "null"], "description": "who made the change; absent for one the node made itself (expiry)" } } })
}

/// The shield JSON a proposal carries, and a signed shield is made over.
fn shield_spec_schema() -> Value {
    json!({ "type": "object", "required": ["name", "expiresAt"], "additionalProperties": false, "properties": {
        "name": { "type": "string", "description": "`[a-z0-9][a-z0-9-]*`, at most 63 characters; one shield of a name is in force on a node at a time" },
        "description": { "type": "string" },
        "advisory": { "type": "string" },
        "scope": { "type": "object", "description": "`{\"kind\":\"all\"}` (the default) or `{\"kind\":\"sandboxes\",\"ids\":[...]}`" },
        "syscalls": { "type": "array", "description": "rules on trappable system calls; at most 64 rules in all; argument index at most 5" },
        "network": { "type": "array", "description": "rules by `cidr`, `name` (`host` or `*.domain`), `ports` {from,to} and `protocol`; a rule with several fields matches only when all match" },
        "hooks": { "type": "array", "description": "rules `{\"hook\": <name>, \"errno\": 1-4095}` refusing a kernel operation at a Linux security (BPF LSM) hook, whatever system call reaches it; the hook is one of hv2_shield_bpf::HOOKS (such as `socket_create`, `sb_mount`, `bpf`, `userns_create`); at most 64; enforced only where a kernel runs BPF LSM: see docs/SHIELDS.md" },
        "expiresAt": { "type": "integer", "description": "unix seconds; must be in the future" } } })
}

impl HyperMachineOntology {
    /// The node's shield routes. The control plane does not serve them.
    pub(super) fn build_shield_node_operations() -> Vec<Operation> {
        let access = || Self::sandbox_access_responses(true, false);
        let team_refused = || -> OperationResponse {
            fail(
                403,
                "`shields are the node operator's: a team's request cannot read or change \
                 them`: the request carries the team header the control plane sets for a \
                 request made for a team (`hv2_cluster::ownership::TEAM_HEADER`). Shields \
                 cover every tenant's sandboxes and what they record names other tenants' \
                 sandboxes and destinations, so no team's request reaches any shield route, \
                 read or write. A caller that reaches the node directly with the cluster \
                 token and no team header is not refused here",
            )
        };
        let not_saved = || -> OperationResponse {
            fail(
                500,
                "`applied, but not saved, so a restart would undo it`: the change is in \
                 force in memory and the node could not write its registry file. Do not \
                 retry blindly: read `GET /shields` for what is in force, fix the \
                 directory, and know that a restart before the next successful save will \
                 forget the change (and, for an enforce or remove, restore the old state)",
            )
        };
        let digest = || {
            path_param(
                "digest",
                "The shield's digest: the SHA-256 of its JSON, 64 lowercase hex digits",
                json!({ "type": "string", "pattern": "^[0-9a-f]{64}$" }),
            )
        };
        let signed_directive = json!({ "type": "object",
            "required": ["directive", "signer", "signature"],
            "properties": {
                "directive": { "type": "string", "description": "the JSON text of {\"action\": \"enforce\"|\"monitor\"|\"remove\", \"shield\": <64 lowercase hex digest>, \"issuedAt\": <unix seconds>}; these exact bytes are signed" },
                "signer": { "type": "string", "description": "the key ID of the signer, which must be in the node's keyring" },
                "signature": { "type": "string", "description": "ML-DSA-87 signature over `directive`, hex" } } });

        let specs = vec![
            Spec::new("list_shields", "List The Node's Shields", "GET", "/shields")
                .about(
                    "Every shield the node holds, in every state (proposed, monitor, enforce, \
                     removed, expired, revoked), with the node's attestation key, the \
                     register of its measured log, whether the registry is persistent, and \
                     the policy it enforces. A shield is a signed rule that refuses \
                     something a running sandbox might do, loaded and removed with no \
                     restart. Here a sandbox is a microVM, so what a shield refuses is its \
                     network: every sandbox's gateway asks the node's shields on each DNS \
                     question and connection. The answer is the node's whole picture: \
                     `nodeKey` and `nodePublicKey` (the key that signs attestations), \
                     `register` (hex, the hash chain's head), `persistent` (false when the \
                     node has no --shield-dir: shields are then in memory with an empty \
                     keyring, so none can ever be loaded and a restart forgets proposals), \
                     `policy.minMonitorSecs` (--shield-min-monitor: how long a shield must \
                     have been monitored before it may be enforced) and \
                     `policy.distinctEnforcer` (--shield-two-keys: the key that enforces a \
                     shield must differ from the one that signed it), and `syscallGuests` \
                     (empty unless the node runs with --shield-guests; then, by sandbox or \
                     machine id, whether its guest agent took the syscall rules: \
                     `applied`, `unsupported` with the guest's reason, such as an agent \
                     older than the operation or a kernel without seccomp user-notify, or \
                     `unreachable`). Read-only, safe for \
                     an agent. Not a promise about the guests: see the warnings on \
                     `load_shield`. The control plane does not route `/shields`, and a \
                     request carrying a team header is 403.",
                )
                .answers(ok(
                    200,
                    "The shields and the node's shield state.",
                    Some(json!({ "type": "object", "properties": {
                        "nodeKey": { "type": "string", "description": "the attestation key's ID" },
                        "nodePublicKey": { "type": "string", "description": "hex" },
                        "register": { "type": "string", "description": "hex head of the measured log" },
                        "persistent": { "type": "boolean" },
                        "policy": { "type": "object", "properties": {
                            "minMonitorSecs": { "type": "integer" },
                            "distinctEnforcer": { "type": "boolean" } } },
                        "syscallGuests": { "type": "object", "description": "by sandbox or machine id: {\"status\": \"applied\"|\"unsupported\"|\"unreachable\", \"reason\"?}; empty without --shield-guests" },
                        "shields": { "type": "array", "items": entry_schema() } } })),
                ))
                .answers(team_refused())
                .answers_all(access())
                .repeatable(),
            Spec::new("load_shield", "Load A Signed Shield (Operator)", "POST", "/shields")
                .about(
                    "OPERATOR ONLY. Load a shield the operator has signed, in MONITOR mode: \
                     it records what it matches and refuses nothing until a separate signed \
                     directive enforces it. The body is a SignedShield: `spec` is the \
                     shield's JSON as a string (its SHA-256 is the digest that names it, and \
                     these exact bytes are what `signature` covers), `signer` the signing \
                     key's ID and `signature` the ML-DSA-87 (FIPS 204) signature, hex. The \
                     node loads it only if the signer is in its keyring \
                     (`<shield-dir>/keyring.json`, which only the operator writes) and not \
                     revoked, the signature verifies, the shield has not expired, no other \
                     shield of its name is in force, and the node holds fewer than 256 \
                     active. A shield may stay in force at most 30 days from when it was \
                     loaded whatever its `expiresAt` says. The cluster token alone is not \
                     authority to do this: without a keyring key the answer is 403. Because \
                     a loaded shield applies to sandboxes already running, a wrong or broad \
                     one is a change to every tenant's network even in monitor mode (a \
                     name rule makes the gateway read each connection's TLS SNI or HTTP Host \
                     before relaying, which delays and can refuse connections that never \
                     finish naming something), so review the digest first. An agent should \
                     propose (`propose_shield`) and leave loading to an operator, who signs \
                     offline with `hm shield sign`. Warnings that hold for every loaded \
                     shield: a shield refuses only trappable system calls (a fixed set of \
                     about 56 rarely made calls) and network destinations; with --shield-guests the node also sends the syscall rules to each \
                     guest's agent, which traps them in the programs IT starts (exec, \
                     start, commands) and their children; a program started before the \
                     first push, or not by the agent (init, a daemon the guest launched \
                     itself), is not trapped, a guest's root can kill the agent and so \
                     end its trapping, and four hot calls (io_uring_enter, splice, tee, \
                     vmsplice) are trapped only in programs started while a rule names \
                     them; without the flag a system-call rule does nothing to a \
                     microVM sandbox, whose guest calls go to its own kernel; \
                     a name rule refuses DNS questions \
                     and the names that connections send, and does NOT block an address the \
                     name resolves to (an address serves other names too; a connection \
                     by address alone needs a CIDR rule); a connection refused by the name \
                     it sends has already been dialled, though none of the guest's bytes went \
                     up it; pointer arguments are never read. Not idempotent: loading the \
                     same shield again is 409.",
                )
                .takes(
                    json!({ "type": "object", "required": ["spec", "signer", "signature"], "properties": {
                        "spec": { "type": "string", "description": "the shield's JSON text (see `propose_shield` for its fields); its SHA-256 is the digest" },
                        "signer": { "type": "string", "description": "the signer's key ID, from the node's keyring" },
                        "signature": { "type": "string", "description": "ML-DSA-87 signature over `spec`, hex" } } }),
                    true,
                )
                .answers(ok(
                    200,
                    "Loaded in monitor mode: the measured log's `load` event.",
                    Some(json!({ "type": "object", "properties": { "event": event_schema() } })),
                ))
                .answers(fail(
                    400,
                    "the shield's JSON is invalid (an unknown field, a bad name, an argument \
                     index above 5, more than 64 rules, a malformed CIDR or name), has \
                     already expired, or the signer or signature is not well-formed",
                ))
                .answers(fail(
                    403,
                    "the signature is not one the node trusts: the signer is not in its \
                     keyring, is revoked, or the signature does not verify; or the request \
                     carries a team header (shields are the node operator's)",
                ))
                .answers(fail(
                    409,
                    "the shield is already here, another shield of its name is in force, or \
                     the node holds the most it allows (256 active)",
                ))
                .answers(not_saved())
                .answers(body_rejection())
                .answers_all(access()),
            Spec::new(
                "propose_shield",
                "Propose A Shield For An Operator To Review",
                "POST",
                "/shields/proposals",
            )
            .about(
                "Put a shield in front of the operator for review. A proposal DOES NOTHING: \
                 it is not loaded, not monitored, refuses and records nothing, and no \
                 sandbox is touched; it exists so a person or an agent that has found a \
                 hole (a kernel advisory reachable through a system call, a command-and-\
                 control domain) can hand the operator the exact bytes to sign. This is the \
                 route an agent uses. The body is `spec`, the shield's JSON as a STRING \
                 (the digest that names it is the SHA-256 of exactly these bytes, so \
                 re-serialising it changes the digest) and an optional `proposedBy`. The \
                 JSON is checked strictly: `name` (`[a-z0-9][a-z0-9-]*`, at most 63), \
                 optional `description` and `advisory`, `scope` (`{\"kind\":\"all\"}` or \
                 `{\"kind\":\"sandboxes\",\"ids\":[...]}`), `syscalls`, `network`, \
                 `hooks` and `expiresAt` (unix seconds, in the future); an unknown field, an \
                 argument index above 5, a hook not in the list, or more than 64 rules of a \
                 kind is 400. A hook rule refuses a kernel operation at a Linux security \
                 hook for every process in scope, with no argument tests, and is enforced \
                 only by a kernel running BPF LSM: in a microVM sandbox's guest, on a node \
                 run with --shield-guests and a guest kernel built with it, and never by the \
                 stock guest kernel. Network rules match by CIDR, \
                 by name (`host` or `*.domain`), by port range and by protocol, all fields \
                 of a rule having to match; write a CIDR rule as well as a name rule when \
                 an address must be blocked, because a name rule does not block the \
                 addresses the name resolves to. A syscall rule can name only a trappable \
                 call, and reaches a microVM sandbox's guest only on a node run with \
                 --shield-guests, through its agent. At most 256 proposals \
                 wait at once (409). `proposedBy` is free text the caller chooses, cut to \
                 128 characters, and it is NOT checked against any identity: do not treat it \
                 as who proposed, and never as approval. Anyone holding the cluster token \
                 (or anyone, on a node with none) can propose, and a proposal sits in the \
                 operator's list until someone loads or withdraws it, so a flood of \
                 proposals is a nuisance bounded by the 256 limit. Not idempotent: the same \
                 bytes again are 409.",
            )
            .takes(
                json!({ "type": "object", "required": ["spec"], "additionalProperties": false, "properties": {
                    "spec": { "type": "string", "description": "the shield's JSON as a string, byte for byte what will be signed", "contentMediaType": "application/json", "contentSchema": shield_spec_schema() },
                    "proposedBy": { "type": "string", "description": "who proposes it, as they describe themselves; not checked; at most 128 characters" } } }),
                true,
            )
            .answers(ok(
                201,
                "The proposal, as an entry in state `proposed`; it does nothing until an \
                 operator signs and loads it.",
                Some(entry_schema()),
            ))
            .answers(fail(
                400,
                "the shield's JSON is invalid, or has already expired",
            ))
            .answers(fail(
                409,
                "the shield (the same bytes) is already here in some state, or 256 proposals \
                 are already waiting",
            ))
            .answers(fail(
                500,
                "the proposal was recorded in memory and the registry file could not be \
                 written; a restart would forget it",
            ))
            .answers(team_refused())
            .answers(body_rejection())
            .answers_all(access()),
            Spec::new(
                "withdraw_shield_proposal",
                "Withdraw A Shield Proposal",
                "DELETE",
                "/shields/proposals/{digest}",
            )
            .about(
                "Drop a proposal that was never loaded, by its digest. Only a proposal: a \
                 shield that is loaded (monitored or enforced) is removed by a signed \
                 `remove` directive (`apply_shield_directive`), never by this route, and \
                 asking for one is 409. This is deliberate, so that holding the cluster \
                 token cannot lift a protection. Anyone who can propose can also withdraw \
                 any proposal, including one another caller made: the proposer is not \
                 recorded in a checked way. The second call is 404. The removal is not a \
                 measured event, since a proposal was never in force.",
            )
            .with(digest())
            .answers(ok(204, "Withdrawn.", None))
            .answers(fail(
                404,
                "`no shield <digest>`: the node holds no shield of that digest",
            ))
            .answers(fail(
                409,
                "`a loaded shield is removed by a signed directive`: the shield is loaded \
                 (or was); only a proposal can be withdrawn here",
            ))
            .answers(fail(
                500,
                "the proposal was withdrawn in memory and the registry file could not be \
                 written; a restart would bring it back",
            ))
            .answers(team_refused())
            .answers_all(access()),
            Spec::new(
                "apply_shield_directive",
                "Enforce, Monitor Or Remove A Loaded Shield (Operator)",
                "POST",
                "/shields/directives",
            )
            .about(
                "OPERATOR ONLY. Change a loaded shield by a signed directive: `enforce` \
                 (it now REFUSES what it matches in every sandbox it covers, with no \
                 restart), `monitor` (back to recording only), or `remove` (out of force \
                 for good). The body is a SignedDirective: `directive` is the JSON text \
                 `{\"action\": \"enforce\"|\"monitor\"|\"remove\", \"shield\": <the \
                 64-digit digest>, \"issuedAt\": <unix seconds>}`, which is what the \
                 signature covers, with the signer's key ID and the signature. The node \
                 checks that the signer is in its keyring and not revoked and the signature \
                 verifies; that the directive is no older than 600 seconds and not more \
                 than 60 seconds in the future; that this exact directive has not been \
                 applied before (it remembers across restarts, so replaying a captured \
                 directive is 409); that, with --shield-min-monitor SECS, the shield has \
                 been monitored that long before it is enforced (409 before then); and \
                 that, with --shield-two-keys, the key that enforces differs from the key \
                 that signed the shield (409). Enforcing refuses real traffic of every \
                 tenant the shield's scope covers: monitor first, read `list_shield_hits` \
                 for what it WOULD have refused, and only then enforce. A shield leaves \
                 force by itself when it expires. The answer is the measured log's event, \
                 or null when the directive changed nothing. Not for an agent: an agent \
                 does not hold a keyring key, and no cluster token substitutes for one.",
            )
            .takes(signed_directive, true)
            .answers(ok(
                200,
                "Applied; the measured event, or null when the shield was already in that \
                 state.",
                Some(json!({ "type": "object", "properties": {
                    "event": { "oneOf": [event_schema(), { "type": "null" }] } } })),
            ))
            .answers(fail(
                400,
                "the directive is not well-formed: not JSON of that shape, an action other \
                 than the three, a digest that is not 64 lowercase hex digits, or a signer \
                 or signature that is not well-formed",
            ))
            .answers(fail(
                403,
                "the signature is not one the node trusts: the signer is not in its \
                 keyring, is revoked, or the signature does not verify; or the request \
                 carries a team header",
            ))
            .answers(fail(404, "`no shield <digest>`: the node holds no such shield"))
            .answers(fail(
                409,
                "the directive is stale (older than 600 seconds) or from the future (more \
                 than 60 seconds), was already applied, the shield is not in a state that \
                 action applies to (proposed, removed, expired or revoked), it has not been \
                 monitored for --shield-min-monitor seconds, or --shield-two-keys is set \
                 and the enforcing key is the shield's signer",
            ))
            .answers(not_saved())
            .answers(body_rejection())
            .answers_all(access()),
            Spec::new(
                "reload_shield_keyring",
                "Re-Read The Shield Keyring (Operator)",
                "POST",
                "/shields/keyring",
            )
            .about(
                "OPERATOR ONLY. Make the node read `<shield-dir>/keyring.json` again: the \
                 trusted signers, revoked signers and revoked shields. The operator edits \
                 that file; the node does not change it, and this route sends no body. \
                 Anything the new keyring no longer vouches for is RETIRED at once and the \
                 retirement is measured (a `revoke` event): a shield whose signer was \
                 removed or revoked, or which is itself revoked, leaves force in every \
                 sandbox. Adding a key adds a person who can put rules over every tenant's \
                 network, so the keyring file is the node's real trust root and its \
                 permissions matter; the cluster token only lets a caller ask for the \
                 re-read. A keyring that does not parse is 400 and the node keeps the old \
                 one. The answer is the number of signers now trusted and the events of \
                 what was retired. Repeating it with an unchanged file changes nothing.",
            )
            .answers(ok(
                200,
                "The keyring was read.",
                Some(json!({ "type": "object", "properties": {
                    "signers": { "type": "integer", "description": "trusted signers now" },
                    "retired": { "type": "array", "items": event_schema() } } })),
            ))
            .answers(fail(
                400,
                "the keyring file is unreadable or does not parse; the node keeps the \
                 keyring it had",
            ))
            .answers(fail(
                403,
                "the request carries a team header (shields are the node operator's)",
            ))
            .answers(fail(
                409,
                "`this node keeps no shield directory (--shield-dir), so it has no keyring \
                 file`",
            ))
            .answers(fail(
                500,
                "the keyring was applied in memory and the registry file could not be \
                 written",
            ))
            .answers_all(access())
            .repeatable(),
            Spec::new(
                "list_shield_hits",
                "List What Shields Matched",
                "GET",
                "/shields/hits",
            )
            .about(
                "What the shields matched, across the node's sandboxes: a hit is REFUSED \
                 when its shield is enforced and only RECORDED when it is monitored (so \
                 monitor-mode hits are what enforcing would have refused). Each hit has a \
                 `seq` from 1 that is never reused, so a gap is a hit let go; `shield` and \
                 `digest`; the `sandbox` ID; `what` (for a network hit, such as \
                 `tcp 203.0.113.9:443 evil.example`); and `enforced`. `tally` counts \
                 `monitored` and `refused` per shield digest, `recorded` is every hit \
                 made and `dropped` the number not in `hits`. The log keeps the FIRST 256 \
                 hits and the LATEST 1024 and lets the middle go, and it is NOT persisted: \
                 a node restart empties it (the tally too). So the log is evidence of what \
                 happened since the node started, not a complete audit, and a quiet shield \
                 after a restart proves nothing. It names other tenants' sandboxes and \
                 destinations, which is why a request with a team header is 403. Read-only.",
            )
            .answers(ok(
                200,
                "The hits.",
                Some(json!({ "type": "object", "properties": {
                    "hits": { "type": "array", "items": { "type": "object", "properties": {
                        "seq": { "type": "integer" },
                        "at": { "type": "integer", "description": "unix seconds" },
                        "shield": { "type": "string" },
                        "digest": { "type": "string" },
                        "sandbox": { "type": "string" },
                        "what": { "type": "string" },
                        "enforced": { "type": "boolean", "description": "true: refused; false: only recorded" } } } },
                    "recorded": { "type": "integer" },
                    "dropped": { "type": "integer" },
                    "tally": { "type": "object", "additionalProperties": { "type": "object", "properties": {
                        "monitored": { "type": "integer" },
                        "refused": { "type": "integer" } } } } } })),
            ))
            .answers(team_refused())
            .answers_all(access())
            .repeatable(),
            Spec::new(
                "list_shield_events",
                "Read The Shields' Measured Log",
                "GET",
                "/shields/events",
            )
            .about(
                "The measured log: every load, enforce, monitor, unload, expire and revoke, \
                 in order, with the `register` that is the head of the hash chain over \
                 them. A verifier replays the events and checks that the register matches \
                 the one in an attestation (`attest_shields`). The log is only ever \
                 appended to and is NEVER compacted, so it grows with every change for the \
                 life of the registry file (a long-lived node accumulates it; there is no \
                 route to trim it). A change the node could not save is in memory but may \
                 be missing after a restart. It is the node's own record: whoever controls \
                 the host can rewrite the file (a rewrite that does not replay stops the \
                 node starting, but a consistent rewrite does not). Read-only; 403 with a \
                 team header.",
            )
            .answers(ok(
                200,
                "The register and the events.",
                Some(json!({ "type": "object", "properties": {
                    "register": { "type": "string", "description": "hex" },
                    "events": { "type": "array", "items": event_schema() } } })),
            ))
            .answers(team_refused())
            .answers_all(access())
            .repeatable(),
            Spec::new(
                "attest_shields",
                "Attest To What Has Been In Force (Operator)",
                "POST",
                "/shields/attestation",
            )
            .about(
                "Have the node sign its register over a verifier's nonce: a statement of \
                 what shields have been in force here. The body is `nonce`, hex, 16 to 64 \
                 bytes, which the verifier must make fresh and unpredictable; the answer \
                 is `nodeKey`, the nonce, the `register`, how many `events` it covers, the \
                 time `at`, and the ML-DSA-87 `signature`. Verify it by fetching the node's \
                 public key (`list_shields` `nodePublicKey`) out of band, replaying \
                 `list_shield_events` to the register and checking the signature and the \
                 nonce. The attestation key is a FILE on the node (`<shield-dir>/node.key`, \
                 created at first start with mode 0600), NOT a TPM: anyone who controls \
                 the host can sign anything, so an attestation shows what the node SAYS \
                 was in force and that its log is consistent, not that the host was \
                 honest. TPM quotes are not implemented. Without --shield-dir the key is \
                 generated at each start. It changes nothing, but it is the operator's \
                 (not an agent's) route: a signed statement from the node's key is \
                 evidence others rely on. A nonce that is not hex is 400.",
            )
            .takes(
                json!({ "type": "object", "required": ["nonce"], "additionalProperties": false, "properties": {
                    "nonce": { "type": "string", "description": "hex, 16 to 64 bytes (32 to 128 hex digits)" } } }),
                true,
            )
            .answers(ok(
                200,
                "The signed attestation.",
                Some(json!({ "type": "object", "properties": {
                    "nodeKey": { "type": "string" },
                    "nonce": { "type": "string" },
                    "register": { "type": "string" },
                    "events": { "type": "integer" },
                    "at": { "type": "integer" },
                    "signature": { "type": "string" } } })),
            ))
            .answers(fail(
                400,
                "`the nonce is hex` (it is not), or it is shorter than 16 or longer than 64 \
                 bytes",
            ))
            .answers(team_refused())
            .answers(body_rejection())
            .answers_all(access()),
        ];
        specs.into_iter().map(|spec| spec.build(true)).collect()
    }

    /// The capability over the node's shield routes. The control plane
    /// serves none.
    pub(super) fn build_shield_capabilities() -> Vec<Capability> {
        vec![capability(
            "shields",
            "Shields (node daemon)",
            "Signed, expiring rules an operator lays over a node's running sandboxes' \
             networks with no restart, first in monitor mode and then enforced by a \
             separate signed directive, every change measured in a hash-chained log the \
             node signs over a verifier's nonce. An agent may read (`list_shields`, \
             `list_shield_hits`, `list_shield_events`) and propose (`propose_shield`, \
             `withdraw_shield_proposal`), which put nothing in force. Loading, directing, \
             re-reading the keyring and attesting (`load_shield`, \
             `apply_shield_directive`, `reload_shield_keyring`, `attest_shields`) are the \
             operator's and need a key in the node's keyring or its keyring file. The \
             cluster token alone does not put a shield in force, and a request made for a \
             team is refused (403). Limits: in microVM sandboxes system-call rules \
             reach only the programs the guest agent starts, and only with \
             --shield-guests (a guest's root can end that by killing the agent), a name \
             rule does not block the \
             addresses a name resolves to, the node's attestation key is a file and not a \
             TPM, hits are not persisted, and the measured log is never compacted. See \
             docs/SHIELDS.md and crates/hv2-sandboxd/src/shields.rs",
            &[
                "list_shields",
                "load_shield",
                "propose_shield",
                "withdraw_shield_proposal",
                "apply_shield_directive",
                "reload_shield_keyring",
                "list_shield_hits",
                "list_shield_events",
                "attest_shields",
            ],
            true,
            &["a node started with --shield-dir, for anything to be loaded"],
            &["cluster token", "a keyring key (operator routes)"],
        )]
    }
}
