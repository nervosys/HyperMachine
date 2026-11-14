# `hm sandbox vm` against a real daemon on KVM

Three runs of `tools/check-hm-cli-kvm.py`, each starting its own `hv2-sandboxd`
(512 MiB, 1 vCPU, a `--disk-dir`, the busybox-plus-agent template) and driving it
only through the real `hm` binary as a subprocess, 74 invocations a run. Every run
passed all 16 cases. It is a functional check; the seconds in each `report.json`
are the check's own stopwatch.

## Inputs

| | SHA-256 (identical in all 3 runs) |
|---|---|
| `hm` (built 2026-11-13 14:38, not rebuilt) | `f98c086c9c58ed9026eaade027ff7e3acd6ff9c9317d106ef96fbfe5e33ddd4e` |
| `hv2-sandboxd` | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| kernel (bzImage) | `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297` |
| initrd (busybox + agent) | `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485` |
| driver | `45ae8dc15e7b2809fcd4fa783890e6c9c85aec904e434719a448ca24779351d4` |

No commit since 14:38 on 2026-11-13 touches `crates/hm-cli` (the later ones change
`hv2-sandboxd` only, and the daemon is newer than `hm`), so `hm` was not rebuilt.

## Cases (pass in run 1 / 2 / 3)

| Case | 1 | 2 | 3 |
|---|---|---|---|
| `disk create / list / inspect` print JSON naming the disk | pass | pass | pass |
| `create --name --cpus 1 --memory-mb 512`; `list` and `inspect` show it; the guest has 1 CPU and about 463 MiB | pass | pass | pass |
| `exec`: stdout, stderr, the guest's exit code (7), argv quoting (`'`, `$(...)`, backticks stay literal), `--timeout 2` on `sleep 30` exits 124 | pass | pass | pass |
| `ps` and `history` print JSON; history holds the earlier exec | pass | pass | pass |
| `files upload` / `download` of a 256 KiB binary: guest SHA-256 equals the source's, the download is identical, a second download over it exits 1 | pass | pass | pass |
| `checkpoint save / list / ls / cat / restore / delete`: after restore the guest file is back to its saved content | pass | pass | pass |
| `pause`, `inspect` says `paused`, `resume`; a file in RAM is still there | pass | pass | pass |
| `fork --count 1`: the child has the parent's file, and writing it in the child leaves the parent's alone | pass | pass | pass |
| `create --name` shows in `list` metadata; `alias bind` on a standalone daemon exits 1 with 404 (see findings) | pass | pass | pass |
| `network create / list / connect (x2) / inspect / disconnect (x2) / delete` | pass | pass | pass |
| `standby` prints its status (`standby: true`); the next `exec` wakes it with state intact | pass | pass | pass |
| `delete` of the fork and the original leaves `/sandboxes` empty | pass | pass | pass |
| `create --disk NAME:/data`: data written survives the sandbox; `disk delete` is refused (exit 1) while held and works when free | pass | pass | pass |
| errors: `exec`, `inspect`, `pause`, `delete` on a deleted sandbox exit 1 with the API's 404 message; `create --cpus 0` exits 2 | pass | pass | pass |
| no sandbox left behind | pass | pass | pass |
| ran on KVM: daemon log has `platform: Kvm` (10) and no `platform: Tcg` (0) | pass | pass | pass |

At most 2 sandboxes were alive at once.

## Findings

None of these is a crash; they bear on the claim "JSON output on every command".

1. **There is no `--json` flag, and not every command prints JSON.** Commands that
   print a JSON document (pretty-printed): `create`, `list`, `inspect`, `resume`,
   `fork`, `standby`, `ps`, `history`, `disk *` (except delete), `network *`
   (except disconnect and delete), `checkpoint save/list/ls/restore`,
   `files upload/download`. Commands that print **nothing** on success (exit 0,
   empty stdout): `pause`, `delete`, `checkpoint delete`, `network disconnect`,
   `network delete`, `disk delete`. `exec` and `checkpoint cat` print the guest's
   raw bytes, by design. Errors are plain text on stderr (`Error: sandbox API
   returned 404 Not Found: {...}`), not JSON.
2. **`disk inspect NAME` and `disk delete NAME` give a bare 404.** They take the
   `diskID` (`disk-1f862cb7ad53c32a`), not the name given to `disk create`:
   `hm sandbox vm disk inspect clidata` prints
   `Error: sandbox API returned 404 Not Found` with no body and no hint.
3. **`alias bind` against a standalone daemon** prints
   `Error: sandbox API returned 404 Not Found` (empty body) and exits 1: reserved
   aliases are a control-plane route (`PUT /sandboxes/{id}/names/{name}`), so the
   daemon has none. The message does not say so.
4. **`delete` is not idempotent**: deleting an already-deleted sandbox exits 1
   (`no sandbox sbx-...`). `pause` on it likewise.
5. `exec`'s error body differs from `inspect`'s on the same missing sandbox
   (`{"error":"no sandbox ..."}` against `{"code":404,"message":"no sandbox ..."}`).

## Not covered

Subcommands not run: `usage`, `pending-registrations`, `reconcile-registration`,
`adopt-owner`, `domain bind/unbind`, `alias bind` success (all need a cluster or
control plane); `login` / `logout` (browser and SSO); `public-ports`,
`web-sharing`, `udp`, `tcp`, `tcp-stdio`, `reverse` (port tunnels and a public
edge, each with its own check elsewhere); `volume *` (separate checks
under `2026-10-03`); `build`, `run`, `compose` (their own KVM checks under
`2026-10-19` and later); `mcp`, `mcp-http`; `benchmark`; `machine *` (long-lived
machines, not sandboxes); `resize`. `hm sandbox vm` has no network-policy
update subcommand (`allowOut`/`denyOut` exist only on `machine create`), so none
was run.

## NOT VERIFIED

- The CLI against a daemon with API keys, TLS or mTLS (`--api-ca-cert`,
  `HV2_API_KEY`): this run used a plain-HTTP daemon with no keys.
- `exec` with output larger than a few KiB, `--timeout` beyond 2 s, or `files`
  beyond 256 KiB through this CLI.
- `resize`, `resume --cpus/--memory-mb`, `fork --count` above 1, `create
  --auto-checkpoint`.
- The Ubuntu guest (the template was busybox plus the agent) and any platform
  other than Linux/KVM; the CLI on Windows or macOS against a daemon.
- Whether a network member can reach another by alias (only membership and
  addresses were read back).
- That the JSON shapes are stable: the check reads fields (`sandboxID`, `state`,
  `diskID`, `members`, `standby`) but is not a schema test.
