# BOAT_PARITY.md read against docs/benchmarks (2026-11-15)

A read of every row of the matrix (31 rows from "What a sandbox is" to "Who runs it"),
the 3 figures rows, the "Where boat is ahead" items 1 to 10, the "Where HyperMachine has
something" list (7 bullets), "What is boat's alone" and "Found while checking". Nothing was
run: no VM, no build. Method: every relative link was checked for existence and every
`#anchor` against the target file's headings (script, no failures); the numbers and
run counts in each linked README were read against the row; claims with no link were
searched for in `docs/benchmarks/` by keyword.

## Links

All relative links in BOAT_PARITY.md resolve (files and anchors). No evidence README
was edited. Two older READMEs are stale in a way the rows already account for:
`2026-10-06/block-disk-kvm` says a sandbox with a disk cannot be checkpointed or
paused (the matrix links DISKS.md for the later change); `2026-10-08/disk-fork-kvm` lists
"more than two forks" as not shown (see Fork below).

## Per row

Confirmed means every number, run count and "not shown" in the row matched the report.

| Row | Result |
|---|---|
| What a sandbox is | Confirmed (machines-kvm: 1 GiB ext4, reboot, daemon kill, power loss and start latency not shown; warm-pool-kvm small initramfs). |
| Sizes | Confirmed (sandbox-sizes 2 vCPU/1024 MiB; sized-templates 0.019/0.025 s against 0.248 s, 1 to 2 samples; sized-checkpoint; repeats 3 of 3, 6 cases; the 2026-11-14 repeats README records that sized snapshots are now accepted, matching the row). |
| Changing size later | Confirmed (resize 7 cases x 3; fork-size; block-disk 16 MiB to 1 TiB is from DISKS.md). |
| Creating one | Confirmed (warm-pool 100 creates, 18.89/0.65/11.61 ms; boat-gaps 503 in 2.5, 3.8, 2.5 ms). Softened: "there is no request option that says so when the node queues" became "(read from the code, not tested)" because boat-gaps lists it NOT VERIFIED. |
| What stopping keeps | Confirmed (standby 40 rounds, 57 us median; disk-pause not timed). Corrected: "Resume on another node is NOT VERIFIED" was too broad; sandbox-history-kvm paused on node a and resumed on node b. Now says only the command history was carried; memory and processes on another node NOT VERIFIED. Link added. |
| Fork | Mismatch fixed: "up to eight copies with a disk, checked on KVM" overstated; disk-fork-kvm ran two forks and a count of 9 (refused 400); "more than two forks, up to the limit of eight" is listed as not shown. Now: eight is the code's limit, a count of 9 is refused, only two forks of a disk were run. fork-busy 560 of 560, 40 at a time, 2 vCPU/2048 MiB confirmed; repeats 7 cases x 3, 0.284 to 0.464 s confirmed. |
| Automatic snapshots | Confirmed (60.2 s first run; 61.2 s in all three repeats; 6 checks; interval of 5 s refused; node-wide flag, disk, other node NOT VERIFIED). |
| Named snapshots | Confirmed (teams-kvm red-snap; sized-checkpoint; repeats 5 cases x 3). |
| Reading a snapshot from outside | Confirmed (checkpoint-files 0.061 to 0.132 s; checkpoint-archive stopwatch note; machine-snapshot-archive). |
| Sharing a snapshot | Confirmed (snapshot-share-kvm: refusal without acknowledgement, share, revoke). |
| systemd, and Docker | Confirmed against machine-docker (serial console), machine-agent, systemd-sandbox, docker-sandbox (13 cases incl. compose build and two projects, 3 runs), hm-run/build/registry/compose, hm-multistage (5 cases, 3 runs), hm-from-stage (7 cases, 3 of 3), compose-network (6 cases, 3 of 3, one node), compose-ports (TCP only; team-scoped key refused), standalone-network (9 cases; the first three runs failed case 7 and the fixed build passed 3 of 3, as the row says). |
| /dev/kvm in the guest | Confirmed, including the nested numbers: 0.6 s host direct; 1.12 s guest time host-direct trace; 60 to 243 s (2026-10-27) and 270 s (2026-10-24 reports a console stalled at 17 to 34 s guest time for 20, 60 or 270 s probes); exit handling 6x to 37x. |
| Installed for you | Confirmed (TOOLS_IMAGE.md 1536 MiB; tools-image-kvm; machine-agents-kvm). |
| Commands and files from outside | Confirmed (machine-agent/runs/files; reserved-create 20 checks; boat-gaps terminal and stdin: pty 100x30, exit 7, stdin closed; the README says a hand-written Connect client, not the E2B SDK, which the row says). |
| Output of past commands | Confirmed (machine-runs-durable lost run; sandbox-history pause and other-node). |
| The guest's processes | Confirmed (guest-processes-kvm). |
| An HTTPS address for a port | Link added: sandbox custom domains and private URLs had no link; private-web README (24 functional cases, custom-domain access). Machine proxy and login reports confirmed. |
| A port from the caller's machine | Confirmed (reverse-port, machine-reverse-port, machine-reverse-cluster). "Into it, TCP and UDP" is covered by the udp forwarding reports; not re-opened. |
| A public address of its own | Confirmed (native-gateway: two runs, 14 checks each, 1 MiB, 65,507 bytes). |
| A desktop in a browser | Confirmed (one run; RFB greeting; no browser drew it). |
| Computer use | Confirmed (agents connect to the server; no key used, no agent used it). |
| Coding agents run for you | Confirmed (conversations on stand-ins; real agents installed, no key). |
| Environments | Corrected: "kept across a reboot and into a fork" for the Lit repositories is not in lit-repositories-kvm (it names a reboot as not shown, and runs no fork). Rewritten to the clone into a sandbox, with reboot and fork marked NOT VERIFIED for repositories. git-repositories (redirect refused) and git-private (5 cases) confirmed. "Setup steps run when a template is built" is code only, no kept report found. |
| Secrets | Confirmed (secret-registration 12 checks, 17 requests; secret-substitution 7; boat-gaps envVars). |
| Network policy | Confirmed (machine-network 5 checks, debug then 3 of 3 release; network-fresh-daemon 16; boat-gaps PUT network). |
| Networks between sandboxes | Confirmed (28 checks, same host). |
| Scoped API keys | Confirmed (api-key-reload 16 checks; key-presets 25 cases x 3; roles). Presets exist, key-creation API does not. |
| Webhooks | Mostly confirmed (machine-webhooks verifies `hv2-signature` HMAC over timestamp.body). Marked: rotation keeping the old secret signing has only unit tests (events.rs), no KVM report. |
| Telemetry | Confirmed (otlp-export http/json; team-telemetry; otelcol logs and traces accepted, content not read back). |
| Teams | Confirmed (teams-kvm; repeats 5 x 3; team-roles-kvm 33 of 33 cases, three runs). |
| How much was used | Confirmed (usage-kvm stopwatch, paused 4.0 s excluded, no API keys, no cluster; usage-ledger Redis 8.0.2). |
| Client libraries | Link added for "E2B's own, unmodified": E2B SDK 2.51.0 ran on KVM in history-output-redaction-kvm and sandbox-history-kvm. The Python and TypeScript KVM checks have five cases and no prompts case, so "prompts" is now marked not exercised. |
| A command line | Confirmed (hm-cli-kvm 16 cases x 3; hm-cli-json-kvm 16 x 3; JSON on every success except `exec` and `checkpoint cat`; still no `--json`). Nothing stale. |
| GPU | Marked: code only, no kept report. |
| Who runs it | Not a capability claim about evidence (DEPLOYMENT_GUIDE.md link only). |

## Prose

- Header: "the latest is 2026-11-11" was stale; now 2026-11-15, and a sentence points here.
- Where boat is ahead, items 1 to 10: item 1 now links machine-ubuntu-kvm for "boots Ubuntu 24.04 with systemd ... turns itself off" (it already linked docker, agent, files). Item 4 and 5 and 6, 7 (git-private, git-in-sandbox, team-git), 8 (otelcol), 10 (tools-image-ci, 179 MiB image) confirmed against their links. Item 9 (SDKs "tested as installed") has no kept report; marked "no kept report in docs/benchmarks/".
- Where HyperMachine has something: warm pool confirmed; egress policy and secrets now linked (boat-gaps, secret-registration); workload identity marked NOT VERIFIED (no kept report); shared volumes linked to hm-run-volume-kvm (volume outlives a run; two sandboxes at once NOT VERIFIED) and private networks linked; hv2-sandbox on three platforms marked (no kept report, macOS only in CI); "run by its user" is not a testable claim.
- Figures: node-web-tooling 12.56 median over 12 runs, 9.33 to 15.26, host 42 to 100%, E2B 11.24, Blaxel 19.8 confirmed; warm pool and standby 3.30 ms, 96% CPU confirmed.
- Found while checking: the disk-module note ("a checkpoint by name has worked since 2026-10-23, starting from a snapshot is still refused") is consistent with DISKS.md.

## Counts

Rows read: 31 matrix rows, 3 figures rows, 10 + 7 prose items.
Confirmed with no change: 21 matrix rows and all 3 figures rows.
Fixed mismatch (overstated against the report): 2 (Fork eight copies, Environments reboot/fork).
Stale fixed: 2 (header date; resume on another node, which later evidence partly shows).
Link added: 6 (HTTPS address, Client libraries, egress and secrets, volumes, Ubuntu systemd, resume on another node).
Marked NOT VERIFIED or no kept report: 6 (GPU, workload identity, hv2-sandbox, SDK install, webhook rotation, Lit reboot/fork).
Softened: 2 (503 request option, SDK prompts).
Broken links fixed: 0 (none found).

## Not resolved

- Sandbox "Into it, TCP and UDP" forwarding, the `fifty ports` figure (boat's), the 1 to 100 fork range, template setup steps, and the sized API limits (256 MiB to 64 GiB, 32 vCPUs) are claims about code; the first four are marked NOT VERIFIED or code-only in their rows or here, the last is what the row says the API accepts.
- The `Setup steps run when a template is built` clause has no marker in the row; no report was found.
- A 2026-11-14 nested KVM change, `net-pause-standby-kvm`, and 2026-11-13 reports (agent-ops, machine-noshell, template-user-owner, and others) are not linked from BOAT_PARITY.md; they were not needed to confirm or refute any row, and no row was found stale against them.

## Follow-up after the audit (same day)

- "Into it, TCP and UDP" (sandbox forwarding): the row now links `2026-10-01/tcp-tunnel.md` (TCP) and
  `2026-10-03/udp-kvm` (UDP on KVM, node level, five checks). It is no longer listed as code-only.
- "setup steps run when a template is built": no kept report was found; the row now says so in place.
- The sized API limits (256 MiB to 64 GiB, 32 vCPUs) were already qualified in the row ("only one size ...
  was checked on KVM"), so they needed no marker.
With these, every claim in the matrix is either linked to evidence or marked in its own row.

## Follow-up 2 (same day)

- Template setup steps: the row "A named set of variables, files and repositories" now links
  `../setup-steps-kvm/README.md` (new driver `tools/check-setup-steps-kvm.py`, 3 runs of 4 cases on KVM) in place
  of "no kept report". The same row also links `2026-11-13/build-noshell-kvm`, `template-user-owner-kvm`,
  `agent-ops-timing-kvm` and `agent-ops-batch-kvm`.
- Creating one: `2026-11-13/restore-loop-kvm`.
- What stopping keeps: `2026-11-14/net-pause-standby-kvm` and `2026-11-13/disk-mount-agent-kvm`.
- Reading a snapshot from outside: `2026-11-13/agent-ops-archive-kvm`.
- Commands and files from outside: `2026-11-13/machine-noshell-kvm`.
- The guest's processes, listed from outside: `2026-11-13/agent-ops-kvm`.
- What a sandbox may reach on the network: `2026-11-11/urltrace-decision-log-kvm`.
- Fit no row, not linked: `2026-11-11/urltrace-kvm`, `2026-11-11/urltrace-noshell-kvm`,
  `2026-11-12/urltrace-noshell-kvm` (URL tracing, `docs/URL_TRACING.md`, has no matrix row);
  `2026-11-13/machine-network-kvm` and `2026-11-13/machine-runs-kvm` (reports and logs only, no README; cited by
  `machine-noshell-kvm` as regression runs); `2026-11-11/{host-direct-trace,nested-*}` were already linked.
