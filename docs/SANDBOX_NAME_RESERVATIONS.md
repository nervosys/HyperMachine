# Atomic sandbox name reservations design

This is a proposed implementation contract for closing the named SSH gap. Atomic pending reservation primitives are implemented, but named creation enforcement is not implemented. Existing `hm.name` metadata and CLI ambiguity rejection remain the current behavior. The design requires shared ownership in MemoryStore and RedisStore, rather than a control-plane process lock.

## Current behavior and the creation race

The CLI accepts 1–64 ASCII letters, digits, hyphens, underscores or dots, excluding the dot segments `.` and `..`. Names are case sensitive. Creation writes `hm.name` metadata; lookup filters the v1 sandbox inventory and refuses zero or multiple exact matches. Metadata does not reserve a name.

`ControlPlane::create_inner` selects nodes and forwards the create body. The node assigns the sandbox ID. Transport errors currently permit trying another node. Therefore a named create cannot reserve ownership by sandbox ID in advance, and a timeout does not prove that the first node failed to create a VM. Reserving only after a successful response can leave duplicate live named VMs. A process-local lock also fails across replicas.

## Proposed ownership contract

A reservation has a validated name, a random opaque operation token, a state and, after binding, a sandbox ID. Names and reservation tokens are separate from authentication credentials. Store operations are atomic: reserve an unused name; bind a pending reservation only with its matching token and an existing sandbox record; release only the matching owner; resolve only a bound reservation whose sandbox still exists. Names use the CLI grammar and remain case sensitive, with no silent normalization.

Pending reservations have no automatic expiry that can authorize another creator. A timeout or control-plane crash leaves the outcome uncertain: allowing the name to expire could permit a second VM while the first still exists. A later recovery operation must establish the original outcome before releasing or binding the reservation. This trades availability of an uncertain name for prevention of duplicate ownership. Explicit operator reconciliation is required until creation has a durable idempotency protocol.

All named creation routes must reserve before forwarding. The operation token must reach the node as trusted cluster context and identify the resulting cluster record; it must not become guest environment data or a client credential. A successful node response binds the existing reservation before the API reports success. A definitive refusal that guarantees no VM exists may release the reservation or retry another node with the same ownership token. Transport failure, unreadable success response or store failure after creation retains pending ownership and must not blindly retry a different node. Unnamed creation retains its current protocol.

A client retry without an explicit idempotency key receives a conflict while the reservation is pending or bound. Do not infer that two requests carrying the same human-readable name are the same operation. A future idempotent-create interface must authenticate and scope the key, persist its result, and ensure retries recover the same VM.

## Shared storage and lifecycle rules

MemoryStore must update reservations and sandbox records under the same mutex used for sandbox deletion. RedisStore must use atomic scripts for conditional reservation, binding and deletion; compare the full operation token or bound sandbox owner before removing a name. Maintain an index of names owned by each sandbox so deletion can release them in the same transaction that removes the record. An old delete or reconciliation attempt must not release a name newly assigned to another sandbox. Reuse the ownership principles of custom-domain claims while keeping names in separate storage and API types.

Pause, resume and heartbeat updates retain the existing bound name. A heartbeat may update sandbox metadata but must not transfer ownership. Fork and snapshot-derived creation must not inherit a reserved name automatically; the caller must request a new name, or the resulting VM is unnamed. Reaping a sandbox releases its bound names through the same deletion transaction. Pending reservations cannot be reaped merely because no sandbox ID has yet been recorded.

Direct node creation with `hm.name` and legacy metadata must not bypass ownership. The rollout must distinguish authoritative reservation records from advisory metadata. Before enforcing reserved names for existing VMs, inventory current metadata and resolve duplicate names explicitly. Never pick the first duplicate as owner. During migration, keep the existing scan-and-refuse lookup available for legacy names; a reserved lookup must not silently ignore a conflicting legacy VM. A shared migration gate or an atomic store operation covering legacy conflicts is required before switching creation to enforced reservations.

## Interfaces and authorization

Introduce dedicated authenticated reservation and name-resolution interfaces instead of exposing internal tokens through inventory. Management belongs to sandbox capability scope; inventory scope must not gain guest connection credentials or reservation mutation rights. Errors should distinguish invalid name (400), existing or uncertain ownership (409), missing bound target (404) and unavailable store (503), without returning tokens. The CLI should prefer the authoritative lookup after rollout and retain its refusal of ambiguous legacy metadata until migration is complete.

## Required verification before feature completion

| Requirement | Evidence needed |
|---|---|
| Shared atomic creation ownership | Concurrent creators through separate control-plane instances produce one winner in both memory and Redis contracts |
| Unknown creation outcome | A node that creates but loses its response leaves the name unavailable; a second node does not create another VM |
| Recovery identity | Wrong-token bind and release fail; restart can reconcile the original operation without assigning another VM |
| Deletion and reuse | Delete releases bound ownership; a delayed old delete cannot remove the replacement owner's reservation |
| Pause and resume | Name resolves the same VM before and after pause, cross-node resume and control-plane restart |
| Fork and templates | Copies do not inherit exclusive names; requested replacement names are reserved independently |
| Migration | Duplicate legacy metadata stays ambiguous; reserved creation cannot race a legacy collision |
| Authorization | Missing, expired and inventory-only keys cannot mutate reservations or obtain guest connection credentials |
| Shipped CLI behavior | Named SSH reaches the sole reserved guest through the authenticated KVM/TLS fixture, with no duplicate or stale-owner fallback |
| Cost | Matched name-create and resolve measurements on the same guest, store and concurrency, including conflicts, errors and cleanup |

The feature comparison must continue to mark atomic name reservations absent until these interfaces and invariants are implemented and verified. A store primitive or a successful single-process lookup alone does not complete the feature.

## Implemented store foundation

Validated name and reservation types reject invalid stored identities, preserve case-sensitive names and omit operation tokens from Debug output. MemoryStore and RedisStore now support atomic pending reservation, same-token replay, reservation lookup and conditional pending release. Competing operation tokens cannot acquire the same name; a delayed release from the old owner cannot remove its replacement. Bound records cannot be inserted through the pending reservation operation. The shared contract passed in memory and against an owned Redis server on Linux, with all 34 library tests passing. Windows library tests and strict Clippy on both platforms also passed.

Atomic binding now requires an existing sandbox and matching reservation token, permits same-target replay and refuses transfer. Sandbox deletion removes bound ownership in the same memory mutex or Redis script; an old sandbox deletion cannot remove a name reused by a new sandbox. The shared contract verified missing-target and wrong-token rejection, replay, refusal to release a bound record as pending, deletion cleanup and safe name reuse against memory and live Redis.

Authenticated resolution now exists at `GET /sandbox-names/{name}` for admin and sandbox-scoped keys; inventory keys are refused. It returns only `name` and `sandboxID`, with no reservation token or guest connection credentials. Invalid names return 400, unknown or deleted names 404, and pending ownership or an observed conflicting legacy metadata name 409. Store failures return 503 with generic errors. The lookup checks current records but cannot prevent legacy metadata changing after that check; creation enforcement and migration remain necessary.

Node operation identity, legacy migration, reservation management routes and CLI use remain unimplemented. These primitives alone do not close the named SSH feature gap.
