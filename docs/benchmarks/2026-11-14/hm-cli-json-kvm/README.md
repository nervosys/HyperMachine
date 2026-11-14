# `hm sandbox vm` prints JSON on every success: re-check on KVM

Three runs of the tightened `tools/check-hm-cli-kvm.py` (16 cases, 79 `hm`
invocations a run), each against its own `hv2-sandboxd` on KVM, driving only the
rebuilt `hm`. This follows [the first check](../hm-cli-kvm/README.md), whose
findings 1 to 3 this change fixes. All 3 runs passed all 16 cases, and the daemon
log of each has `platform: Kvm` (10) and no `platform: Tcg` (0). A functional check,
not a benchmark.

## Inputs

| | SHA-256 (identical in all 3 runs) |
|---|---|
| `hm` (rebuilt, release, rustc 1.100.0, `-D warnings`) | `400d81cdcaffb983b86dd7ee1aea222a3299edead66e5c8d98d5cdf59617f123` |
| `hv2-sandboxd` (the same binary as the first check) | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| kernel (bzImage) | `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297` |
| initrd (busybox + agent) | `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485` |
| driver (as read from the Windows checkout, CRLF line endings) | `8efb7792e6720b015d26ce5a27cf51e3652038228e61f5402a930083a044bdeb` |

## What changed in `hm`

- Every success prints one JSON document, except `exec` and `checkpoint cat`
  (raw bytes, by design). A command whose API reply is empty prints a
  confirmation: `delete` `{"deleted": ID, "kind": "sandbox"}`; `disk|network|volume|machine delete`
  the same with their kind; `checkpoint delete` `{"deleted": NAME, "kind": "checkpoint", "sandboxID": ID}`;
  `pause` `{"sandboxID": ID, "status": "paused"}`; `network disconnect`
  `{"network": N, "sandboxID": ID, "status": "disconnected"}`; anything else empty
  `{"status": "ok"}`. A reply with a body is printed as before.
- `disk inspect` and `disk delete` take a disk's ID or its name; an unknown or
  ambiguous name is an error naming it.
- An API error with an empty body names the method and path; a 404 adds that the
  daemon may not serve the route and a control plane may be needed.
- `delete` is still not idempotent (second delete exits 1, with the API's message).

## What the check now asserts (all 3 runs)

- `pause`, `delete` (every sandbox), `checkpoint delete`, `network disconnect`
  (twice), `network delete`, `disk delete` (by name, and by ID for a second disk)
  each print exactly the object above, parsed as JSON.
- `disk inspect clidata` equals `disk inspect <diskID>`; `disk inspect no-such-disk`
  exits 1 naming `no-such-disk`; `disk delete` by name is refused (exit 1) while a
  sandbox holds the disk, and works by name when it is free.
- `alias bind` on the standalone daemon exits 1 and its message contains
  `PUT /sandboxes/<id>/names/cli-alias` and `control plane`.
- The 15 earlier cases still pass unchanged in substance.

Observed in the runs: a disk delete refused while held says
`sandbox API returned 409 Conflict for DELETE /disks/<id> with no message`
(the daemon sends no body there).

## Unit tests (Windows, `cargo test -p hm-cli`)

New: the five empty-reply confirmations through the real binary, a reply with a
body printed unchanged, disk by ID and by unique name, unknown and ambiguous disk
names (no DELETE sent), and empty-body errors on both request paths. The volume
client test's expectation of empty stdout from `volume delete` was updated to the
new object.

## Not covered by these runs

`exec` and `checkpoint cat` raw output is unchanged and not claimed to be JSON.
Commands that need a control plane, a registry or a browser (as listed in the
first check) were not run, so the `{"status": "ok"}` fallback and the
`volume delete` / `machine delete` / conversation / snapshot confirmations are
covered by code and by the `volume` unit test only, not by KVM.

## NOT VERIFIED

- There is still no `--json` flag and errors are text on stderr, not JSON.
- The CLI against a daemon with API keys, TLS or mTLS.
- Whether a real control plane's reply to `alias bind` (success or failure) prints
  as intended.
- Windows and macOS hosts of the CLI against a daemon (unit tests ran on Windows
  against a fixture server only).
- Stability of the confirmation shapes beyond what these assertions read.
