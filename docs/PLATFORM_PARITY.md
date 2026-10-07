# HyperMachine against boxd and exe.dev

The goal: every capability [boxd](https://boxd.sh/) and [exe.dev](https://exe.dev/) offer, and more.
This page tracks it. Their feature lists were reviewed against their public docs on 2026-10-01, with a targeted
secret, storage and raw-port rechecks on 2026-10-03
(`docs.boxd.sh/llms-full.txt`, `exe.dev/docs/all`). HyperMachine's statuses come from its code,
not its docs. **Real** means wired to a checked binary; it does not imply a published
release or managed production validation. **Partial** says what is missing.

All three products give each user a real Linux VM behind a hardware boundary, rather than a
container. The comparison is about what surrounds the VM.

## The matrix

| Capability | boxd | exe.dev | HyperMachine |
|---|---|---|---|
| Create, list, delete over an API | yes | yes | **Real**: E2B's API (`hv2-sandboxd`, `hv2-control-plane`), so the unmodified E2B SDKs work |
| A CLI | yes | yes (over ssh) | **Yes**: `hm sandbox vm` manages lifecycle, commands, binary files and checkpoints; verified on a real KVM node and authenticated control plane/proxy |
| Fork a running VM, memory included | yes; ~160 ms provider example | `cp` exists; live-memory copying not established | **Real**: `POST /sandboxes/{id}/fork`, 1-100 copies |
| Named snapshots, and new VMs from them | yes | not documented | **Real**: snapshots become templates |
| Checkpoint and restore in place | yes, 10 per VM | not documented | **Real**: 10 per sandbox; memory and disk; same ID, token and URL; a failed restore changes nothing |
| Pause and resume | yes | not documented | **Real**: to disk; any node resumes |
| Suspend when idle, wake on traffic | yes | not documented | **Real**: `idleTimeout` or `--idle-pause-after`, plus `autoResume`. Idle means no traffic *and* a quiet guest CPU, so unwatched work is never frozen |
| HTTPS URL per VM | yes | yes | **Partial**: `{port}-{id}.{domain}` over TLS; operator-supplied wildcard or [hostname certificate bundles](CUSTOM_DOMAINS.md), with [built-in Certbot renewal scheduling and deployment recovery through real KVM traffic](benchmarks/2026-10-03/tls-renewal-worker/README.md); [automatic custom-domain discovery and initial issuance](benchmarks/2026-10-03/discovery-tls-kvm/README.md) verified with owned ACME/KVM; public CA operation remains unverified |
| Per-port HTTP(S) URLs | yes | HTTPS proxy; authenticated additional ports 3000–9999, one public target | **Real**: guest HTTP(S) port URLs through authenticated node/control proxies; see [private guest URLs](PRIVATE_GUEST_URLS.md) |
| Authenticated local TCP/UDP tunnels | raw public forwards documented; local tunnel equivalence not checked | SSH/HTTP access; raw UDP not established | **Real in recorded accepted context**: CLI TCP and framed IPv4/IPv6 loopback UDP through control/node APIs, with TLS/mTLS, peer isolation and lifecycle closure ([UDP evidence](benchmarks/2026-10-03/udp-ipv6-forwarding/README.md), [source alignment](benchmarks/2026-10-03/udp-ipv6-source-alignment/README.md)) |
| Managed public raw TCP/UDP port allocation | yes; stable allocated port, TCP/UDP/both, max 3 per VM, owner-only; persists across reboots | not established in reviewed docs | **Partial**: owner-only API/CLI, stable Memory/Redis allocations and gateway supervision. [Sixteen both-protocol ports per VM](benchmarks/2026-10-03/native-sixteen-tcp-udp-capacity/README.md) and [32-worker mixed TCP/UDP traffic](benchmarks/2026-10-03/native-mixed-tcp-udp-capacity/README.md) pass real KVM checks with exact payloads and pause/resume/delete cleanup. [Gateway/Redis restart recovery](benchmarks/2026-10-03/native-gateway-redis-outage/README.md), [administrator legacy-owner adoption](benchmarks/2026-10-03/legacy-owner-adoption-workflow/README.md), and [one stable both-protocol port across node migration](benchmarks/2026-10-03/native-public-port-migration/README.md) are verified in owned fixtures. [Automatic local registration recovery](benchmarks/2026-10-03/automatic-registration-kvm/README.md) and [repeated store-timeout recovery](benchmarks/2026-10-03/registration-store-timeout-kvm/README.md) preserve uncertain initial/resumed guests. A [rebooted sandbox keeps its ID, token, URL and policy](benchmarks/2026-10-06/reboot-in-place-kvm/README.md), but traffic through native ports across a reboot is unverified. Daemon-crash claim recovery, managed failover, public Internet operation and competitor performance remain unverified |
| Custom domains | yes | yes | **Real**: authenticated cluster bindings to guest HTTP ports, Memory/Redis ownership, HTTPS forwarding; [operator DNS and certificates](CUSTOM_DOMAINS.md) |
| DNS validation and automatic domain TLS | yes | not checked | **Partial**: [opt-in DNS TXT ownership verification](CUSTOM_DOMAINS.md) gates new domain claims and port updates; [operator-configured certificate worker](TLS_RENEWAL_WORKER.md) verifies scheduling, activation and crash recovery with real ACME-to-KVM traffic, plus [fresh-account initial issuance through KVM](benchmarks/2026-10-03/initial-tls-kvm/README.md); [automatic claim discovery and initial issuance](benchmarks/2026-10-03/discovery-tls-kvm/README.md) verified with owned ACME/KVM; retirement and public DNS/CA operation incomplete |
| Private URLs with login, identity headers | public web URL; team shell sharing | yes (`X-ExeDev-Email`) | **Partial**: [dedicated operator-issued browser credentials and trusted subject headers](PRIVATE_GUEST_URLS.md), TLS/mTLS required, expiry and atomic reload; [exact sandbox-ID sharing grants, revocation, fork exclusion and domain rebinding](benchmarks/2026-10-02/private-web-scopes/README.md) pass 25 KVM/TLS checks; [owner API/CLI grants and opt-in HTTP/1+HTTP/2 TLS enforcement](benchmarks/2026-10-04/web-sharing-proxy/README.md) verified in local fixtures; [five local Redis AOF-always hard restarts](benchmarks/2026-10-04/web-sharing-aof-restart/README.md) preserve grants/revocations; [ten real KVM owner API/CLI/TLS cases](benchmarks/2026-10-05/owner-sharing-kvm/README.md) verify fork exclusion, paused revocation and shipped-control-plane restart; [thirteen live KVM/Redis outage cases](benchmarks/2026-10-05/owner-sharing-redis-outage/README.md) verify paused no-wake denial and active/revoked AOF recovery; managed durability remains open; SSO and verified email claims absent |
| SSH to a VM by name | yes | yes | **Partial**: [persisted metadata names](benchmarks/2026-10-01/ssh-name.md) and authenticated stdio transport verified with real KVM/TLS, binary transfer and duplicate/key rejection; [reserved aliases for existing VMs](benchmarks/2026-10-02/reserved-alias/README.md) verified with deletion/reuse and fork ownership; [control-plane named creation](benchmarks/2026-10-02/reserved-create/README.md) is KVM/TLS verified, including deletion/reuse; [forked children omit the parent name](benchmarks/2026-10-02/fork-name/README.md), preserving parent lookup; [node-side atomic completion and discarded-descriptor recovery](benchmarks/2026-10-02/node-completion/README.md) are KVM-verified; [committed-name recovery after real node response loss and timeout](benchmarks/2026-10-02/node-response-loss/README.md) passes 24 KVM/TLS checks; [guest preservation after post-commit event failure](benchmarks/2026-10-02/node-publication-fault/README.md) is KVM-verified; [registration ACL partial-write prevention](benchmarks/2026-10-02/registration-acl/README.md) is verified against Redis and [real guest registration-write failure preservation](benchmarks/2026-10-02/node-registration-fault/README.md) passes 26 combined KVM/TLS checks; [authenticated node reconciliation of the same preserved guest](benchmarks/2026-10-02/registration-reconcile/README.md) is KVM-verified; [uncertain-registration lifecycle guards](benchmarks/2026-10-02/registration-lifecycle/README.md) are KVM-verified across the idle deadline, including [connect and checkpoint mutation protection](benchmarks/2026-10-02/registration-mutations/README.md); [Automatic named registration recovery](benchmarks/2026-10-03/named-automatic-registration-kvm/README.md) now passes two real KVM profiles with duplicate refusal, preserved operation identity, deletion and name reuse; fleet migration, daemon-crash recovery and guest SSH provisioning remain incomplete |
| exec, and file copy in and out | yes | ssh/scp | **Real**: `/exec`, envd processes with PTY and stdin, files |
| **Env vars for every command in a VM** | org-wide | creation env supported; command inheritance not checked | **Real**: E2B's `envVars`, kept in the guest so pause, fork and snapshots carry them |
| Secrets held off the VM, injected at the edge | host-bound placeholder substitution in headers, query, body and Basic auth | yes | **Partial**: exact-ID operator scopes rewrite headers, Basic auth, query, JSON, form and raw bodies over verified HTTPS in real KVM; delimiter escaping, rejected reload retention, rotation, revocation, fork exclusion and pause/resume verified. Binary payloads, upstream hostname refusal and rotation/revocation on an established guest HTTPS connection are verified; managed org scope and exact lifecycle race schedules remain incomplete ([current guide](EGRESS_SECRETS.md), [keepalive evidence](benchmarks/2026-10-03/secret-https-keepalive/README.md)) |
| Workload identity (AWS/GCP federation) | no | yes | **Real**: JWT-SVIDs minted at the gateway, JWKS and OIDC discovery |
| Egress policy per VM | egress allowlist documented; enforcement details not checked | no | **Real**: allow/deny lists, live updates, decision log, reserved ranges refused |
| VM-to-VM networks by tag | yes | via proxy | **Partial**: [same-node guest TCP/DNS](benchmarks/2026-10-04/private-guest-gateway-kvm/README.md) and [two daemon nodes on one owned host](benchmarks/2026-10-04/private-cross-node-kvm/README.md) pass KVM checks with Internet disabled, exact one-MiB payloads, owner/tag DNS isolation, stale-address recovery and source pause/resume and [long-lived guest TCP closure after target membership removal](benchmarks/2026-10-04/private-guest-stream-revocation/README.md), plus [source membership removal and fresh-binding rejoin recovery](benchmarks/2026-10-04/private-source-stream-revocation/README.md). Owner-scoped membership, atomic Memory/Redis address publication and generation-pinned mTLS transport are implemented. [Actual source guest IPv4 UDP through the maximum 65,507-byte payload](benchmarks/2026-10-04/private-udp-max-source-guest-kvm/README.md) preserves empty/binary/fragmented datagrams through the production gateway/router/connector, with owner binding refusal and stale-address recovery, plus [established UDP source/target membership revocation and rejoin](benchmarks/2026-10-04/private-udp-active-membership-kvm/README.md) and [target pause/resume](benchmarks/2026-10-04/private-udp-target-pause-kvm/README.md) and [route-store lookup failure/recovery](benchmarks/2026-10-04/private-udp-store-outage-kvm/README.md) and [target deletion](benchmarks/2026-10-04/private-udp-target-delete-kvm/README.md) and [source pause/resume/deletion](benchmarks/2026-10-04/private-udp-source-lifecycle-kvm/README.md). Independent hosts, complete lifecycle/pending-publication tests, complete guest stream/half-close/saturation gates, private IPv6 and full UDP lifecycle gates, and crash durability remain unfinished or unverified |
| Teams, roles, sharing | yes | yes, with SSO | **Partial**: [teams](TEAMS.md) isolate tenants on the control plane: each key and each sandbox belongs to a team, and a key reaches its whole team's sandboxes by role (operator/observer) and no other team's, with events, webhooks and volumes partitioned per team and the resources not yet partitioned (snapshots, template builds) closed to team keys. Also operator-provisioned browser identities with owner-managed grants; creator-bound sandbox API admission and filtered inventory passed a rebuilt-release KVM gate. Per-team namespaces for those shared resources, durable team membership with an API remain incomplete. [Single sign-on](SSO.md) through any OpenID Connect provider gives verified-email members API sessions with their team and role (browser flow and `hm sandbox vm login`, tested against a fixture provider); SSO for private guest URLs is not yet done |
| Scoped, expiring API keys | yes | yes | **Real on the control plane**: hashed operator-provisioned keys, request-time expiry and capability scopes; single team; [atomic policy replacement and Unix signal reload](API_KEY_ROTATION.md) are verified through real HTTP and a running process |
| Persistent volumes shared between VMs | persistent movable disks, one attachment at a time; concurrent sharing not documented | not documented | **Partial**: E2B volumes over 9P, live and shared (Linux hosts), with existing authenticated control-plane management and bearer-token content forwarding; integrated CLI management, streaming upload/download, directory listing/stat and directory creation are verified through owned real routes. Exact 4 GiB client transfers and [path-command HTTPS trust checks](benchmarks/2026-10-03/volume-path-cli-tls/README.md) pass in protocol fixtures; managed-storage guarantees remain incomplete. Exclusive metadata creation and file/directory flushing, with two-daemon binary upload/replacement/restart checks. Opt-in atomic upload preserves previous bytes on ordinary pre-publication failure; default replacement remains in-place. **Movable block disks are real on a node**: ext4 [disks](DISKS.md) one sandbox holds at a time, released when it ends and claimed by the next, with the data intact; this was [verified on real KVM](benchmarks/2026-10-06/block-disk-kvm/README.md), including a killed daemon's claim not outliving it. Disks are node-local, with no control-plane routing; a sandbox with a disk cold-boots and cannot pause or fork; power-loss equivalence is not established ([volumes guide](VOLUMES.md)) |
| Build images from Dockerfiles or OCI | compose | Dockerfile | **Real**: E2B template builds, no Docker |
| Backups to object storage | yes | no | **Partial**: [encrypted offline S3 snapshot-store backup and recovery](OBJECT_STORAGE_BACKUPS.md), verified with an owned S3 HTTP emulator and real KVM paused guests, mounted volumes and named snapshots; [receipt-driven exact-version retention](benchmarks/2026-10-02/backup-retention/README.md) verifies scoped deletion and failure handling; [durable scheduled offline capture](benchmarks/2026-10-02/scheduled-backups/README.md) verifies restart, uncertainty/reconciliation, pins and catalog repair on owned emulators; [integrated locked retention](benchmarks/2026-10-02/locked-retention/README.md) verifies serialization with capture and pins; [scheduled real KVM recovery](benchmarks/2026-10-02/scheduled-kvm/README.md) verifies paused guest memory, volumes and named snapshots; managed-store durability, IAM and coordinated guest maintenance remain unverified or absent |
| Scheduled jobs and event triggers | `*.run.ts` | no | **Partial**: lifecycle webhooks, [durable delayed host-process jobs and interval publication](JOBS.md), and [explicit VM dispatch verified with KVM/TLS](benchmarks/2026-10-01/scheduled-dispatch.md); an [automatic VM worker is verified with KVM/TLS](benchmarks/2026-10-01/scheduled-worker.md), with operator-recorded completion recovery; [calendar catch-up execution is KVM/TLS-verified](benchmarks/2026-10-01/scheduled-calendar.md), with [bounded batch publication and restart verified on KVM](benchmarks/2026-10-01/calendar-publication-kvm.md) and [matched local publication timings](benchmarks/2026-10-01/calendar-publication.md); live DST scheduling, guest execution throughput and automatic guest reconciliation remain incomplete |
| Desktop in a browser, browser for agents | yes | web terminal | **Absent** |
| MCP for agents | skill + MCP | remote MCP with browser login; Shelley agent | **Partial**: 12 lifecycle/exec/checkpoint tools plus 2 opt-in binary file tools over [MCP stdio](src/ai/mcp-server.md), with cancellable client waits checked on real KVM; accepted remote work can continue, and [operator-authenticated MCP JSON HTTP](benchmarks/2026-10-02/mcp-http/README.md) passes local transport/cancellation tests with a loopback listener for an operator TLS proxy; [official-client HTTPS and real KVM lifecycle verification](benchmarks/2026-10-02/mcp-http-kvm/README.md) passes checkpoint restore, pause/resume, fork isolation and cleanup; [opt-in HTTP binary file tools](benchmarks/2026-10-02/mcp-http-files/README.md) pass a 256 KiB roundtrip and size refusal through trusted envd HTTPS; [real-guest HTTP cancellation](benchmarks/2026-10-02/mcp-http-cancellation/README.md) verifies session reuse and continued accepted remote work after correcting an official-client interoperability failure; [observer-role inheritance](benchmarks/2026-10-02/mcp-http-observer/README.md) verifies thirteen refusals under an observer/admin-scope policy; OAuth/browser login, streaming plus the wider `hv2-agent` surface remain incomplete |
| Email in and out | no | yes | **Absent** |
| Multi-node, self-hosted | contact sales | enterprise | **Real**: control plane, Redis store, cross-node resume, mTLS, Helm chart |
| GPU | no | no | **Partial**: VFIO code, not wired to sandboxes |
| Hypervisor you can read | no | no | **Real**: our own VMM; KVM, WHPX, HVF; a Type-1 path |

Custom domains now have authenticated management endpoints, proxy routing and
CLI bind/list/unbind commands. Memory/Redis claims enforce sandbox ownership
and deletion cleanup. A real KVM guest served HTTPS content through both
proxies, followed a guest-port update, retained its Redis binding across a
control-plane restart, resumed on alias access with `autoResume` enabled, and
released its hostname after deletion. HTTP/1.1 and HTTP/2 fixture tests preserve
the public host, path, query and body. See [setup and evidence](CUSTOM_DOMAINS.md).
Operators still provide DNS and certificates. Optional sandbox-bound TXT ownership
verification now gates domain claims and port updates; ten real KVM checks verify
refusal, successful routing, unchanged routing after failed revalidation, restart
and replacement isolation through a trusted owned HTTPS resolver. The same binary
passes the six legacy-mode checks without the policy. Existing bindings are not
retroactively revalidated and public DNS propagation remains unverified.
These functional passes establish no performance win.

The [integrated ACME deploy-hook check](benchmarks/2026-10-03/acme-deployment/README.md)
passes thirteen issuer/deployment checks and fourteen real KVM checks. Certbot
performs real HTTP-01 renewal against Pebble and its deploy hook activates the
new leaf while an existing 16 MiB guest download completes. Failed activation
rolls back the manifest and leaf; expired-certificate recovery, idempotent retries,
process/manifest identity and concurrent-deployment refusal are verified. The
earlier [issuer-only check](benchmarks/2026-10-03/acme-issuance/README.md) is preserved.
Initial certificates were operator-supplied in that fixture. [Later automatic claim discovery and initial issuance](benchmarks/2026-10-03/discovery-tls-kvm/README.md) verifies owned ACME/KVM provisioning; operators still configure the authenticator/webroot. The
later [renewal worker fixture](benchmarks/2026-10-03/tls-renewal-worker/README.md)
passes twenty-one scheduling/input/process tests, twenty-one live issuer/worker
checks and fourteen KVM checks. Continuous due checks, exclusive journal locking,
new-PID recovery after successful issuance, and hard termination after publication
or activation are verified without duplicate issuance. Later owned domain-discovery checks verify automatic provisioning; public CA operation and fleet-wide orchestration remain incomplete.

The [certificate bundle fixture](benchmarks/2026-10-03/tls-bundle/README.md) passes
fourteen combined DNS/TLS/KVM checks plus six legacy checks on the same binary.
New handshakes see a renewed leaf while an existing 16 MiB guest download completes;
rejected key mismatch preserves the certificate and guest route. Optional default
certificates activate and are removed through reload. All 1,067 API library tests
and strict API/cluster lint pass. The later ACME fixture uses that same compiled
control plane; its operator hook supplies verified certificate deployment.

The [Boxd documentation](https://docs.boxd.sh/llms-full.txt) distinguishes public
web access from team shell sharing, describes platform-held integration credentials,
automatic domain TLS, and forked egress allowlists. The [exe.dev documentation](https://exe.dev/docs/all)
describes remote MCP and VM copying, but does not establish live-memory copying.
These are documentation findings, not independent runtime tests. “Not documented”
and “not checked” do not establish that a competitor lacks a capability.

## Current measured performance scope

| Workload | Verified result | Remaining comparison limit |
| --- | --- | --- |
| Prepared VM creation, 100 concurrent | HyperMachine P50 853 ms / P99 1449 ms versus Firecracker P50 1018 ms / P99 1646 ms in the recorded owned fixture | HyperMachine PSS is higher; managed boxd/exe.dev endpoints are unmeasured |
| [Recorded optimized cold VM creation, 1 concurrent](benchmarks/2026-10-03/current-release-cold-sweep/README.md) | HM/Firecracker P50 402.77/341.64 ms; P99 418.03/358.75 ms; held PSS 150.72/85.92 MiB | 4/4 per engine passed; same eight-CPU affinity and guest; owned WSL, short cohort, managed endpoints unmeasured |
| [Recorded optimized cold VM creation, 8 concurrent](benchmarks/2026-10-03/current-release-cold-sweep/README.md) | HM/Firecracker P50 504.84/410.66 ms; P99 554.34/520.40 ms; held PSS 897.75/670.91 MiB | 32/32 per engine passed; same eight-CPU affinity and guest; owned WSL, short cohort, managed endpoints unmeasured |
| [Recorded optimized cold VM creation, 50 concurrent](benchmarks/2026-10-03/current-release-cold-sweep/README.md) | HM/Firecracker P50 3114.63/2743.81 ms; P99 3318.44/4284.52 ms; held PSS 4305.52/4180.28 MiB | 100/100 per engine passed; same eight-CPU affinity and guest; owned WSL, short cohort, managed endpoints unmeasured |
| [Recorded optimized cold VM creation, 100 concurrent](benchmarks/2026-10-03/current-release-cold-sweep/README.md) | HM/Firecracker P50 6871.04/5673.70 ms; P99 7238.33/5943.06 ms; held PSS 8586.02/8360.31 MiB | 200/200 per engine passed; same eight-CPU affinity and guest; owned WSL, short cohort, managed endpoints unmeasured |
| [MMIO guests boot with `pci=off`](benchmarks/2026-10-05/mmio-pci-off/README.md), cold creation | Per boot: 26,319 → 23,958 VM exits (Firecracker ~23,450), the ~2,080 PCI config-space exits gone. One guest: HM p50 457 → 432 ms; gap to Firecracker 42 → 19 ms across four interleaved blocks (80/80 passed) | C8 gap 105 → 78 ms (noisy); C50 inconclusive on the shared host; HyperMachine still slower than Firecracker |
| [Guest RAM on huge pages](benchmarks/2026-10-06/guest-thp/README.md) (#143), cold creation | Per boot: 24,159–24,624 → 2,809–2,991 VM exits; nested page faults 22,100 → 659 (Firecracker 23,536–25,061 exits). Daemon RSS +~6 MB per guest | Exit counts only: latency not measured (host at 100% CPU). Sparse-touch guests and fragmented hosts not tested |
| [MMIO guests on hardware-reduced ACPI](benchmarks/2026-10-06/acpi-hw-reduced/README.md) (#144), cold creation | Per boot: 2,809–2,991 → 2,250–2,303 VM exits; I/O APIC accesses 439 → 30, PIC port I/O 38 → 0 (Firecracker 50 and 0). 3/3 boots ready | Exit counts only: latency not measured. No networked guest traced; PCI guests unchanged |
| Secret HTTPS, one client/one guest vCPU | Original → optimized 17.340 → 14.844 ms | Large synthetic raw body, new TLS connection, local WSL |
| Secret HTTPS, eight clients/one guest vCPU | 114.349 → 113.806 ms | No consistent concurrent latency gain |
| Secret HTTPS, eight clients/two guest vCPUs | 48.070 → 48.652 ms; mixed pair directions | No consistent concurrent latency gain at equal resources |
| Same optimized daemon, one → two guest vCPUs | 115.217 → 45.574 ms | Additional resource allocation, not a code or competitor win |
| [UDP 64-byte HTTPS/KVM round trips](benchmarks/2026-10-03/udp-latency/README.md), one outstanding | CLI P50 0.956/0.968 ms; P99 1.535/2.442 ms in two runs | Development build, 100 samples per run; no sustained load or competitor endpoint |
| [Native UDP vs CLI tunnel, matched KVM ABBA](benchmarks/2026-10-03/native-cli-abba/README.md), 4 KiB | Native/CLI completion rate: 1,793/1,429 round trips/s (IPv4, 2 peers); 3,817/3,390 (IPv6, 8 peers). Native mean peer medians 20.5%/11.6% lower | Internal product paths; same guest port, both processes live; unpinned development builds, 1,000 samples/peer/block. Boxd/exe.dev unmeasured |
| [Release native UDP vs CLI tunnel, KVM ABBA](benchmarks/2026-10-03/native-cli-release-abba/README.md), 4 KiB | Native/CLI completion rate: 2,921/2,359 round trips/s (IPv4, 2 peers); 7,399/6,549 (IPv6, 8 peers). Native mean peer medians 19.3%/12.6% lower | Optimized internal paths; same 1-vCPU/1,024-MiB guest; unpinned short runs, mixed eight-peer P99 block ranking. Boxd/exe.dev unmeasured |

[Current cold-admission repeats](benchmarks/2026-10-03/current-admission-sixteen/README.md) expose a reliability gap beyond the all-passing engine sweep: sixteen admitted starts passed 800/800 versus 571/800 uncapped. Successful P99 was higher with the limit in both cohorts, and paired median improvements were inconsistent. The default remains unchanged; these are same-product configuration measurements, not competitor wins.

The VM measurements and their provenance are recorded below. [HTTPS performance evidence and configuration guidance](EGRESS_SECRETS.md#measured-performance-and-guest-sizing) links exact reports and immutable binary identities. No measured result supports superiority across every workload, feature or resource metric.

## Remaining delivery gaps

| Area | Verified foundation | Work still required |
| --- | --- | --- |
| Edge secrets | Exact-ID scopes; full request formats; completed reload on reused guest HTTPS connections | Managed organization scopes, multi-tenant authorization, fleet distribution and controlled lifecycle races |
| Persistent storage | 9P sharing, exclusive creation, private metadata, sync ordering and two-daemon upload/restart | Maximum-sized real-volume-store transfers and shipped-control-plane TLS verification, automatic health/Redis/network-storage failover, hard-kill staging recovery, power-loss checks, network-storage guarantees, and control-plane routing and pause/fork for [block disks](DISKS.md) |
| Networking | Per-VM egress; authenticated IPv4/IPv6 local tunnels; sixteen owner-managed native TCP/UDP ports; mixed-protocol KVM traffic, stable-port node migration and same-node private guest TCP/DNS | Public Internet operation, native public ports through a [guest reboot](REBOOT.md) (the sandbox's identity, URL and policy are KVM-verified to persist), controlled loss/overload recovery, fleet failover, cross-node private guest networking and complete lifecycle/durability gates |
| Identity and collaboration | Scoped operator keys, exact-ID browser grants, owner API/CLI, real KVM/TLS admission, shipped-control-plane restart, local AOF-always Redis hard-restart and live outage/no-wake recovery | Per-team namespaces for snapshots and templates (sandbox access, events, webhooks and volumes are [team-isolated](TEAMS.md)), SSO for private guest URLs ([browser and CLI SSO](SSO.md) are in), durable team membership, power-loss guarantees and managed/fleet-scale durability |
| Agent interfaces | Verified MCP transports and VM operations | Browser OAuth, streaming, desktop/browser environments and email |
| Hosted operations | Owned ACME issuance/renewal and discovery; local backup/recovery and scheduling; opt-in automatic local registration reconciliation under refusal and store timeouts | Public CA/DNS operation, automatic certificate retirement, managed storage/IAM, fleet rollout and durable daemon-crash registration/claim recovery |
| Performance comparison | Reproducible owned HyperMachine/Firecracker fixtures and matched secret-processing binaries; a cold boot now takes about 2,300 VM exits against Firecracker's 23,700 on the same kernel and initrd | Equal-host managed-product measurements, fleet throughput/P99, and quiet-host latency for the exit reductions (#139, #143, #144) |

The absence of competitor endpoints prevents managed-product measurements but does not prevent implementation work. Existing fixtures prove only their stated local scope. Automatic certificate retirement and publication remain separate authorization-dependent steps; their pending state does not establish a feature or performance win.

## Implementation journal

The entries below record successive implementation and verification steps. Statements such as “not yet wired” or “not rebuilt” describe the state at that step and may be superseded by later evidence. Use the matrix and current guides above for the latest status; frozen archives preserve earlier source, reports and limitations.

## HyperMachine capabilities to compare

An October 3 targeted recheck of the [Boxd primary documentation](https://docs.boxd.sh/llms-full.txt)
corrects two earlier comparisons. Its egress guide documents host-bound secret
placeholders substituted in several request locations. HyperMachine's verified
header injection alone does not establish that broader parity. Its disk guide
documents persistent storage movable between VMs with a single attachment and
same-host placement constraints. Concurrent sharing is a separate capability;
HyperMachine's 9P volumes do not establish matching block-device workflows.
These are provider documentation claims, not independent service measurements.
The [exe.dev primary documentation](https://exe.dev/docs/all) was also retrieved;
absence of a volume entry establishes no runtime absence.

**Superseded:** host-bound placeholder substitution has since been built and
verified on real KVM, covering headers, Basic auth, the query, and form, JSON and
raw bodies, with exact-sandbox scopes and SIGHUP rotation and revocation. See the
[operator guide](EGRESS_SECRETS.md) and the
[KVM evidence](benchmarks/2026-10-03/secret-substitution-kvm/README.md). The rest
of this paragraph records the state at that step.

At that step, the HTTPS interception relay kept request bodies streaming and
accepted only header transforms; it had no host-bound placeholder policy or
query/body/Basic-auth substitution API. Before extending that surface, request
URI debug logging was removed from the working relay because paths and query
strings may contain guest credentials. Method and response status remain logged.
This source change is not yet in the accepted benchmark daemon and establishes
neither substitution parity nor a performance gain.

[Egress logging regression evidence](benchmarks/2026-10-03/egress-uri-logging/README.md)
records all 167 passing networking library tests against the exact changed relay
in an isolated accepted-source checkout. Header injection, interception TLS,
destination-policy checks and DNS rebinding refusal pass. This is library
validation; the accepted benchmark daemon was not rebuilt or replaced.

The working networking library now contains a host-bound raw substitution
component (`secret_substitution`). It validates unique fixed-length opaque
placeholders, exact hostname scopes and bounded secret material. Replacement
accepts binary input, runs once without reprocessing inserted values, and bounds
both input and expanded output to 1 MiB. Debug output reports only binding count.
All 171 networking library tests pass in the isolated checkout, including four
new component tests for scope, configuration, expansion and nonrecursive behavior.
The component grants no egress access and requires its caller to authenticate the
upstream hostname. Gateway wiring, operator policy/API, query encoding, Basic-auth
decoding, streaming body framing, rotation and live KVM verification remain
incomplete; existing header transforms are unchanged. Secret values are held in
ordinary process memory without explicit zeroization. This is an implementation
step, not completed secret-substitution parity or a performance improvement.

The component now adds a lock-protected store: validation completes before atomic
policy replacement, placeholders can remain stable across value rotation, and
an empty replacement revokes all bindings. Failed validation preserves the active
policy. Owned binding values are zeroized on drop, and failed expansions use
[zeroizing temporary storage](https://docs.rs/zeroize/1.9.0/zeroize/struct.Zeroizing.html).
This supersedes the ordinary-memory limitation above for component-owned storage;
successful returned request bytes and caller-created copies remain the caller's
responsibility. All 172 networking tests pass in the isolated checkout, including
rotation failure preservation, stable-token replacement and revocation. Offline
lock reconciliation changed only metadata for unrelated accepted-source packages;
those differences were not copied into the working repository. Gateway/API
integration and live rotation verification remain incomplete.

Basic-auth substitution is now implemented in the component: it decodes a Basic
credential, applies the authenticated-host policy and re-encodes changed bytes.
Unchanged credentials and other authorization schemes retain their original
bytes. Malformed Base64 or decoded credentials without a colon are refused;
decoded/replacement temporary buffers are zeroized and encoded expansion remains
bounded. All 173 networking library tests pass, including scope and malformed
credential cases. An initial compile error from an unstable slice helper was
corrected with stable operations before the successful run. Gateway integration,
query encoding, body framing, operator policy and live verification remain open.

Query substitution now handles ampersand-separated names and values with one
percent-decoding pass. Unchanged components retain their original bytes;
substituted components encode delimiters and binary bytes so secrets cannot add
parameters or alter the request target. Literal plus signs are not interpreted
as form spaces. Malformed escapes are refused, and percent-encoded expansion is
bounded to 1 MiB with zeroizing intermediate buffers. All 174 networking library
tests pass, including query structure, scope, double-encoding preservation and
encoded expansion refusal. Gateway integration, body framing, operator policy
and live verification remain incomplete.

The component now rewrites a fully buffered HTTP request under one policy read
lock. It requires the request hostname to match the authenticated upstream,
applies raw/header, Basic-auth, query and raw-body substitution, and updates
Content-Length. Fields commit only after validation; malformed replacement
headers preserve the original request. Host/routing and named hop-by-hop headers
are not substituted. Compressed bodies, trailers and undecoded transfer framing
are refused. All 176 networking library tests pass, including request-field
rewriting and failure preservation. The caller must bound body collection,
authenticate upstream TLS and handle HTTP serialization. JSON/form escaping,
streaming gateway integration, operator configuration and live KVM verification
remain incomplete. Returned HTTP fields use ordinary HTTP buffers; this is not a
claim that every temporary or serializer copy of a secret is zeroized.

An opt-in relay entry point now uses the component on actual HTTP requests.
Substitution mode collects at most 1 MiB under a ten-second body deadline,
rejects trailers, removes decoded transfer framing and forwards corrected body
length. The existing entry point retains streaming bodies without substitution.
All 177 networking tests pass; an owned duplex HTTP fixture verifies the upstream
header, query, body and Content-Length bytes and downstream response. The caller
must already have verified upstream TLS for the supplied hostname. Gateway
selection/configuration, owned TLS substitution verification, KVM operation,
JSON/form escaping and performance comparison remain incomplete. This opt-in
library path does not yet make the feature available to sandbox operators.

The opt-in TLS interception entry point now performs the existing verified
upstream handshake before calling the secret relay. An owned duplex TLS fixture
verifies substituted query/header bytes at the trusted upstream and handshake
refusal for an untrusted upstream CA, with no HTTP request delivered in that
case. The guest trusts a separate interception CA. All 178 networking library
tests pass in the isolated checkout. This closes the library TLS-substitution
verification gap; gateway selection, operator policy/API, KVM traffic and
performance measurements remain incomplete. The accepted daemon is unchanged.

Gateway handles now accept a validated secret store only when interception is
enabled. Matching TLS SNI hosts select the substitution interceptor after the
existing destination/egress checks; bindings do not grant access. Plain HTTP
keeps its existing header-transform path and never substitutes store-held
secrets. Store rotation applies between requests; replacing/removing the handle
applies to new intercepted connections, so operators must revoke existing
connections through the retained store. All 179 networking tests pass, including
configuration refusal without interception, unchanged policy and store revocation.
This verifies handle configuration and preserves the prior TLS fixture, but does
not yet verify secret selection through the full Ethernet gateway or a KVM guest.
Daemon configuration/API, JSON/form escaping and performance remain incomplete.

The full Ethernet gateway selection path now has a passing owned fixture. A
second smoltcp guest sends TCP/TLS through the gateway to a mapped local TLS
upstream, with a host-bound store and no header-transform rule. The upstream
receives the substituted Authorization value, unrelated guest header bytes
remain intact, and the gateway records interception. All 180 networking tests
pass. This supersedes the Ethernet selection gap above; Linux/KVM guest traffic,
daemon configuration, live rotation/revocation and performance remain unverified.

An owned persistent HTTP relay now verifies rotation and revocation across three
requests on the same connection. With an unchanged guest placeholder, the
upstream observes the original secret, the rotated value and finally the raw
placeholder after revocation. Header, query, raw body and Content-Length agree
within each request. All 181 networking tests pass. This supersedes the relay
rotation gap above; it is not TLS/Ethernet/KVM rotation evidence, concurrent
request stress or a performance measurement. Daemon configuration remains open.

Form-body substitution now selects URL-encoded handling from Content-Type,
including case-insensitive media types with parameters. It decodes percent
escapes once and treats plus as space, then percent-encodes changed components
so secret delimiters cannot introduce fields. Unchanged components preserve
their original representation. All 182 networking tests pass, including encoded
placeholders, field preservation, space semantics and updated Content-Length.
This closes component form escaping; multipart forms, JSON escaping, daemon
configuration and live guest verification remain incomplete.

JSON-aware substitution now validates the document as a raw JSON value, then
rewrites string values only. Keys, whitespace, unchanged string representations
and numeric literals retain their original bytes, including numbers beyond
machine-integer precision. Changed strings receive JSON escaping; malformed JSON
or non-UTF8 substituted string values are refused. Request rewriting selects this
mode for application/json and application/*+json. All 183 networking tests pass,
including escaped placeholders, quote/backslash/newline secrets, unchanged keys
and a 30-digit numeric literal. This closes component JSON escaping, not daemon
configuration, multipart support, guest verification or performance parity.

The store now accepts a strict version-1 operator JSON document with `bindings`
entries containing `placeholder`, UTF-8 `value` and exact `hosts`. Input is bounded
to 1 MiB; unknown fields, duplicate struct fields, wrong version/types and
invalid bindings fail with redacted errors. Reload parses and validates before
commit; failed reload preserves active bindings and an empty document revokes
them. All 184 networking tests pass. Private file handling and sandbox ownership
remain caller responsibilities. Inspection located daemon egress integration in
`hv2-sandboxd::start_network`; its network request is also stored for pause/resume
and forks, so lifecycle persistence and isolated per-sandbox rotation must be
resolved before daemon wiring. No secret file is loaded by the daemon yet.

The component now supplies a strict version-1 `sandboxes` registry keyed by
exact sandbox IDs (at most 64 scopes, bounded 1-MiB input). All policies validate
before reload. Existing scopes retain their store handles during rotation;
removed scopes clear retained stores before removal, revoking open-relay access
to future substitutions. Unlisted fork IDs receive no policy. Duplicate IDs,
invalid selectors and unknown schema fields are refused. Commit is atomic per
sandbox/request, not fleet-wide across concurrent requests. All 185 networking
tests pass, including retained-handle rotation/revocation, invalid reload and
explicit fork exclusion. Daemon private-file loading, lifecycle ownership and
guest verification remain incomplete.

Linux scoped-policy loading now requires an absolute path, owned 0700 immediate
directory and owned 0600 regular file with one hard link. File opening refuses
symlinks and uses nonblocking mode before checking file type; input reads remain
bounded and use zeroizing storage. Reload retains validation-before-commit.
All 186 networking tests pass, including valid private loading and refusal of
symlinks, hard links and unsafe permissions. Ancestor-directory policy and daemon
startup/reload/lifecycle integration remain operator/caller responsibilities.

The working daemon now accepts opt-in `--egress-secrets-file` with `--network`
on Linux. Startup privately loads the exact-ID registry; new or resumed gateways
select only their own sandbox scope, so forks do not inherit parent-ID entries.
SIGHUP validates/reloads the file and updates active gateway handles. Failed
reload retains active policy; removed scopes revoke retained stores. Policies
remain operator files rather than guest request fields or snapshot metadata.
Other nodes require their own configured policy file. The isolated daemon passes
`cargo check --offline -p hv2-sandboxd` after correcting an initial synchronous
lock/await mismatch. Binary startup, signal reload, lifecycle behavior and real
KVM traffic are not yet verified; the accepted benchmark daemon is unchanged.

[Owned daemon startup/reload evidence](benchmarks/2026-10-03/daemon-secret-policy/README.md)
records seven passing checks against a separate isolated debug executable.
Unsafe permissions, symlink files and malformed policies refuse startup;
networking is required. Valid private startup and valid/invalid SIGHUP delivery
are verified, with an empty guest inventory and complete owned-process cleanup.
Exact tested sources and the isolated source hash catalog are archived. This
supersedes the startup/signal gap above; log markers and empty inventory do not
prove active secret contents. Guest substitution, lifecycle behavior and
performance comparisons remain outstanding.

### Measured performance comparison

The managed products have no measured entries: no competitor endpoints or matched
host were available. Firecracker is a local engine control, not a measurement of
boxd or exe.dev. The rows below use eight pinned CPUs and matched 1-vCPU/1-GiB
guests on shared WSL KVM; cold timing includes admission queueing. Latencies are
conditional on successful attempts, and PSS excludes kernel memory and unmapped
cache. Prepared restore and cold creation measure different operations.

| Product/engine and workload | Passed attempts | P50 ms | P99 ms | Median held PSS MiB | Evidence |
|---|---:|---:|---:|---:|---|
| boxd managed service | unmeasured | — | — | — | No endpoint available |
| exe.dev managed service | unmeasured | — | — | — | No endpoint available |
| HyperMachine, prepared C100 | 400/400 | 853 | 1449 | 333.95 | [Matched prepared comparison](benchmarks/2026-10-02/resource-c100/README.md) |
| Firecracker, same prepared comparison | 400/400 | 1018 | 1646 | 272.31 | [Same run](benchmarks/2026-10-02/resource-c100/README.md) |
| HyperMachine baseline, cold C8 | 32/32 | 500.06 | 600.93 | 879.55 | [Cold allocator comparison](benchmarks/2026-10-03/mmap-threshold-cold/README.md) |
| Firecracker beside baseline, cold C8 | 32/32 | 425.40 | 472.37 | 670.78 | [Same run](benchmarks/2026-10-03/mmap-threshold-cold/README.md) |
| HyperMachine baseline, cold C100 | 400/400 | 3752.10 | 8138.99 | 8597.77 | [Cold allocator comparison](benchmarks/2026-10-03/mmap-threshold-cold/README.md) |
| Firecracker beside baseline, cold C100 | 400/400 | 7951.29 | 10440.96 | 8361.44 | [Same run](benchmarks/2026-10-03/mmap-threshold-cold/README.md) |

HyperMachine's prepared C100 aggregate latency is lower, while held memory is
higher in every pair. Cold C8 remains slower and uses more held memory than the
local control. The static allocator candidate reduced memory but increased cold
C100 mean from 3877.37 to 4253.31 ms and P99 from 8138.99 to 9640.11 ms; it remains
deferred. Shared-host control shifts and inconsistent paired tails prevent a
universal performance claim. The original reports retain sample identities,
counterbalance, executable hashes, failures and cleanup checks.

The working readiness analyzer now enforces diagnostic flags, stage/request
identity confirmations, C100 concurrency and all four executable/input hashes
with unconditional checks instead of Python assertions. Replaying the preserved
current-readiness reports reproduces the original analysis exactly; 13 damaged
contracts are rejected under `python -O` on both Windows and Linux by
`tools/test-current-readiness-analysis.py`. Frozen historical analyzers remain
unchanged. This closes an optimization-mode validation gap and establishes no
runtime improvement or root cause for the retained readiness failures.

Raw readiness validation now independently matches both stage-map key sets to
the successful HyperMachine request IDs and refuses duplicate IDs. Successful
requests require successful agent stages; stage durations must be numeric,
finite and nonnegative. The same replay still matches the archived analysis
exactly. Twenty-one damaged contracts are rejected under optimized Python on
Windows and Linux, including unrelated stage IDs, duplicate request IDs,
negative/nonfinite/boolean durations and a failed stage concealed behind the
coordinator's positive confirmation flags.

### Certificate lifecycle status

[Receipt-bound explicit retirement](benchmarks/2026-10-03/tls-retirement-receipt/README.md)
passes 17 owned ACME/live TLS checks, including rejection of a wrong prior-leaf
receipt and recovery after a publication crash. Discovery retains completed-job
identity until retirement is verified. Automatic retirement scheduling remains
unimplemented and awaits explicit authorization; public CA/DNS operation and
retirement-specific KVM traffic remain unverified. These functional results
establish no performance win.

Cold boot now has an [optional per-node admission budget](COLD_START_ADMISSION.md).
At 100 concurrent requests on the eight-CPU native fixture, the final binary's
eight-slot budget reduced P50 readiness from 6702 to 3639 ms; P99 increased
from 7898 to 8434 ms. All 800 attempts passed, and paired mean readiness improved
in four of four pairs. [Raw comparisons and controls](benchmarks/2026-10-02/cold-start-admission/README.md)
retain earlier failures and the slower default-disabled C8 control. This is an
opt-in tradeoff, with no managed competitor performance claim.

A subsequent [matched native comparison](benchmarks/2026-10-02/cold-budget-tuning/README.md)
passed 400/400 attempts per engine at C100. HyperMachine with eight slots had
lower P50 readiness (3530 versus Firecracker's 5678 ms) and lower paired means
in four of four pairs, but worse P99 (12857 versus 9168 ms) and higher held PSS
(8617.59 versus 8360.78 MiB). A four-slot experiment also worsened the tail.
The settings remain optional; these local results establish no managed-service
or across-the-board win.

[Two direct eight/sixteen-slot repeats](benchmarks/2026-10-02/cold-budget-comparison/README.md)
passed all 1600 attempts. Sixteen improved paired P99 in six of eight pairs but
paired means in only four. A matched sixteen-slot native comparison passed 400/400
attempts per engine: HyperMachine had lower P50 (3807 versus 5579 ms) and worse
P99 (11741 versus 11042 ms) and held PSS (8658.38 versus 8361.85 MiB). These
tradeoffs leave tail latency and held memory as performance gaps.

A [same-daemon jemalloc comparison](benchmarks/2026-10-02/allocator-comparison/README.md)
retains 400/400 GNU-libc and 398/400 jemalloc attempts in its complete C100
cohort. All three fully passing pairs reduce held memory but worsen mean,
P50 and P99 readiness. An interrupted earlier cohort retains 38 candidate
timeouts and failed cleanup evidence. The tested jemalloc defaults are rejected;
the accepted daemon's allocator remains unchanged.

The latest matched sixteen-slot C100 comparison is summarized below. Both engines
used the same native host and guest inputs; these are local engine measurements,
not managed-platform measurements. [Full evidence](benchmarks/2026-10-02/cold-budget-comparison/README.md).

| Metric | HyperMachine, sixteen cold-start slots | Firecracker 1.17.0 | Better result |
|---|---|---|---|
| Successful attempts | 400/400 | 400/400 | Equal in this cohort |
| Readiness P50 | 3806.696 ms | 5578.907 ms | HyperMachine |
| Readiness P99 | 11741.469 ms | 11041.528 ms | Firecracker |
| Held proportional memory | 8658.382 MiB | 8361.850 MiB | Firecracker |

A separate [matched prepared-snapshot comparison](benchmarks/2026-10-02/prepared-engines/README.md)
passes all 680 main restores plus four smoke restores. At concurrency eight,
two 20-pair cohorts compare restored file and live-process state with identical
guest commands. The table below separates prepared startup from the cold
comparison above. HyperMachine uses a persistent daemon and Firecracker starts
a fresh VMM. These are cache-warm sources on a shared WSL/KVM host.

| Concurrency / cohort | Engine | Passed / planned | P50 ms | P95 ms | P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|---|
| 1 / c1 | hypermachine | 20/20 | 144.550 | 776.709 | 996.605 | 72.299 | 31.340 |
| 1 / c1 | firecracker | 20/20 | 251.022 | 814.257 | 965.727 | 22.324 | 22.324 |
| 8 / c8 | hypermachine | 160/160 | 224.202 | 630.366 | 676.490 | 91.011 | 46.083 |
| 8 / c8 | firecracker | 160/160 | 275.289 | 930.576 | 1151.289 | 39.786 | 39.786 |
| 8 / c8-repeat | hypermachine | 160/160 | 242.313 | 529.611 | 609.110 | 92.385 | 46.428 |
| 8 / c8-repeat | firecracker | 160/160 | 252.420 | 429.677 | 455.355 | 41.003 | 41.003 |

HyperMachine has higher process PSS in every profile and worse single-restore
P99; concurrency-eight aggregate P95/P99 rankings reverse on repeat.
PSS omits kernel memory and unmapped page cache; it does not establish
fleet density. All main and smoke cleanup checks pass; three retained setup
failures prevented 12 planned restores, rather than failing scored restores.
No runtime change or across-the-board performance claim follows from this data.

A [prepared-memory mapping diagnostic](benchmarks/2026-10-02/prepared-memory/README.md)
passes 96 additional restores across two fresh-node cohorts. HyperMachine
has about 44 MiB outside guest-sized mappings versus Firecracker's 6 MiB;
much is already present in the empty daemon. This accounts for roughly three
quarters of the observed total PSS gap and directs investigation toward host
allocation ownership. Raw smaps and parser checks are retained; the diagnostic
does not prove reclaimability or a runtime optimization benefit.

A [boot-buffer allocation probe](benchmarks/2026-10-02/boot-allocations/README.md)
reproduces temporary copies in two fresh daemons: calculating the highest guest
address materializes about 14.1 MiB of boot regions, then frees them without
shrinking the GNU allocator arena. Four prepared restores pass with clean
cleanup. This identifies redundant layout copies but does not attribute the
full retained PSS gap or establish a performance improvement. No runtime
optimization is adopted from this diagnostic.

The resulting [borrowed boot-region candidate](benchmarks/2026-10-02/borrowed-boot/README.md)
passes 3,328 scored restores across an eight-guest profile and two 100-guest
cohorts. Held PSS is lower in all six baseline/candidate outer pairs, but mean
and P99 latency rankings remain mixed. The 100-guest repeat's candidate mean
and P99 are worse in aggregate. All 2,304 core library tests have passing evidence,
including two explicit KVM hardware tests. Default adoption is deferred; the
archive retains the source patch, reproducible accepted overlays, raw comparisons,
and a failed broad lint invocation caused by missing generated guest binaries.

The [single-guest and 50-guest extension](benchmarks/2026-10-02/borrowed-boot-profiles/README.md)
passes 816 more restores with the same binaries. Held PSS is lower in all ten
outer pairs across both archives. Single-guest means and observed tails are worse
with the candidate, while 50-guest results improve alongside shifting Firecracker
controls. The single-guest profile has just four samples per variant/engine;
no stable tail estimate or universal latency win follows from it.

A [same-binary owned/borrowed counterfactual](benchmarks/2026-10-02/boot-buffer-modes/README.md)
passes 1,664 additional restores with runtime activation verified before scoring.
Held PSS is lower in all four mode pairs, but borrowed-mode aggregate means
and observed P99 are worse in both profiles. A 5.5-second P99 occurs in the
100-guest borrowed cohort alongside slower Firecracker controls. This removes
separate executable builds as a confound without proving a latency root cause.
The experiment is isolated, and default adoption remains deferred.

[Creation-stage diagnostics](benchmarks/2026-10-02/boot-buffer-stages/README.md)
pass 1,600 additional restores. [Execution-stage diagnostics](benchmarks/2026-10-02/exec-stages/README.md)
pass 1,604 restores, with cleanup verified and fourteen damaged contracts or
ranking promotions rejected. Blocking-pool wait accounts for 0.6–2.8% of
execution time; command RPC and time outside the HTTP handler remain broad
aggregate costs. Debug logging may perturb timing, so these runs establish no
performance win. The coordinator now checks guest resources after all timed
attempts complete, avoiding validation GETs during peer latency measurements.

The [corrected C100 prepared comparison](benchmarks/2026-10-02/resource-c100/README.md)
passes 400/400 restores per engine. HyperMachine has lower aggregate P50/P99
(853/1449 ms versus 1018/1646 ms), but higher median held PSS
(333.95 versus 272.31 MiB). Paired means favor HyperMachine in three of four
pairs and paired P99 in two of four; held memory is higher in every pair.
This establishes a local memory gap, not a managed competitor win or a causal
performance improvement from the coordinator correction.

A [same-binary C100 repeat with corrected resource timing](benchmarks/2026-10-02/corrected-buffer-modes/README.md)
passes all 1,600 restores. Borrowed buffers reduce held PSS by 14.60/26.41 MiB
in the two fresh-daemon pairs, but worsen means by 100.31/77.54 ms and P99 by
358.51/113.83 ms. Aggregate held PSS falls from 340.81 to 321.00 MiB while
P99 rises from 1081 to 1437 ms. Firecracker controls shift in opposite directions
between pairs, so no causal latency claim follows. The repeat does not support
default adoption; the isolated buffer candidate remains deferred.

A [C100 raw mapping diagnosis](benchmarks/2026-10-02/prepared-memory-c100/README.md)
passes all 400 diagnostic restores with verified cleanup. Paired mapping PSS
is 56.34/58.56 MiB higher for HyperMachine. Guest-sized file mappings contribute
23.30/22.84 MiB; heap and anonymous categories contribute further differences,
partly offset by Firecracker process stacks. About 13 MiB of the aggregate
guest-sized difference is private dirty, whose host/guest write cause is not
isolated. This supports investigating both retained host allocation and restore
write ownership; mapping sizes alone do not prove subsystem ownership.

The [accepted-source restore audit](benchmarks/2026-10-02/restore-memory-audit/README.md)
confirms image restore already uses KVM copy-on-write mapping, skips cold boot
loading, and grows console output buffers on demand. Those paths do not support
an unconditional-image-copy or eager-console-cap optimization. Private-dirty
ownership still needs observations before the first vCPU run and after guest
maintenance; no restore write is removed without that evidence.

[Address-bound single-guest probes](benchmarks/2026-10-02/restore-memory-boundaries/README.md)
pass eight activated restores and fifteen evidence-rejection checks. All four
measured HyperMachine children show 1,528 KiB private dirty before first vCPU
execution. Later private-dirty classification varies markedly. Private dirty
also includes uniquely mapped dirty file-cache pages, so the earlier C100
private-dirty difference must not be interpreted as anonymous COW allocation
or attributed solely to host/guest writes. Anonymous and private-clean counters
are needed next; no runtime change is adopted.

[Reanalysis of the original C100 raw smaps](benchmarks/2026-10-02/prepared-memory-c100-anonymous/README.md)
finds median guest-sized anonymous residency of 216.170 MiB for HyperMachine
and 203.510 MiB for Firecracker, a 12.660 MiB gap. In that C100 cohort the
private-dirty gap is mostly consistent with anonymous residency, rather than
solely dirty file-cache classification. This still does not identify the writing
subsystem or account for the entire PSS gap. The single-guest before-run
observations require their own extended counters and remain unclassified.

The [extended single-guest boundary study](benchmarks/2026-10-02/restore-memory-anonymous/README.md)
passes eight restores and eighteen evidence-rejection checks. Median anonymous
residency is 1,408 KiB before first vCPU execution, 1,436 KiB after restore
acknowledgment and 1,912 KiB after the verification command. The much larger
private-dirty rise includes dirty file-page residency. Anonymous pages are
already present before guest execution in this fixture, but their responsible
restore step and the cause of the C100 difference remain unisolated. No runtime
optimization is adopted from these diagnostic readings.

[Host-stage probes](benchmarks/2026-10-02/restore-memory-host-stages/README.md)
pass eight restores and twenty-two rejection checks. Anonymous residency rises
from zero after image mapping to 1,412 KiB by the post-machine-state boundary,
then remains unchanged through vCPU/device restore before guest execution.
The interval includes layered snapshot-page application, not only machine-state
ioctls. Named snapshot capture currently stores a layer over its base; a full
sparse prepared-source image is the next candidate for avoiding repeated
overlay writes, pending lifecycle and matched performance verification.

The [full sparse named-image candidate](benchmarks/2026-10-02/sparse-named-lifecycle/README.md)
builds but fails a real KVM lifecycle comparison: after deleting the source,
its child still executes and pauses, then fails resume because the paused layer
references the deleted image. The accepted child preserves state through the
same sequence. Both nodes clean up completely. The candidate is rejected for
adoption, and its timing driver refuses cohorts until a matching lifecycle check
passes. Image ownership must include live children and persisted dependencies
before any performance improvement is evaluated.

The [retained-image revision](benchmarks/2026-10-03/sparse-retained-images/README.md)
passes that real KVM deletion/pause/resume comparison for both binaries, preserving
files, child writes and a live process with complete cleanup. Its offline image
collector passes 15 dependency and lock tests. The revision remains isolated:
restart, replacement, collection with real persisted guests, shared-store races,
and capture/physical-storage tradeoffs still require verification before adoption.
Its concurrency-one smoke restores all pass, but candidate mean readiness is
49.90 ms versus 35.46 ms and median capture time is 744.29 ms versus 13.68 ms;
held PSS is also higher. The optimization remains deferred. This small local
sample, with varying Firecracker controls, establishes no general performance win.

The [layered compare-before-write candidate](benchmarks/2026-10-03/layer-compare/README.md)
preserves the existing snapshot capture and format while skipping writes whose
bytes already match the mapped base. Strict core Clippy, 2,298 core tests and
the real guest lifecycle fixture pass. A scored concurrency-eight repeat passes
all 128 restores, but candidate mean readiness is 76.16 ms versus 65.39 ms and
held daemon PSS is 92.62 MiB versus 90.97 MiB. Both outer pairs have higher
candidate mean latency and held PSS; tails and Firecracker controls vary. This
candidate is also deferred. An earlier cohort overlapping test compilation is
preserved separately and excluded from performance conclusions.

A subsequent [prepared-source sham/trim probe](benchmarks/2026-10-02/prepared-reclaim/README.md)
passes all 128 diagnostic restores. A one-time trim after named-source
preparation reduces empty-daemon PSS by a sham-adjusted 28.70/28.75 MiB
and later held PSS by 26.77/27.96 MiB in two fresh-daemon pairs. This proves
bounded reclaimability in the instrumented fixture, not latency neutrality
or the benefit of trimming after initial template construction. A production
candidate and matched latency/lifecycle verification remain required; no
runtime change is adopted.

An isolated [one-time startup trim candidate](benchmarks/2026-10-02/startup-reclaim/README.md)
passes all 776 scored restores and controls across smoke and two C8 cohorts.
Unlike the preload probe, it calls trim after initial template construction,
before listening. Median held PSS falls by 27.49/28.51 MiB, with lower held
PSS in all twelve matched pairs and nearly unchanged incremental guest PSS.
Paired means improve in 4/6 then 6/6 pairs, and paired P99 in 3/6 then 6/6;
Firecracker controls also improve on every candidate-side repeat. This verifies
a bounded fixed-overhead memory reduction, not a causal latency win or fleet
density gain. The candidate remains isolated; larger single-guest and higher
concurrency, sustained allocations, lifecycle and platform checks remain.

The startup candidate is now [rejected after larger prepared bursts](benchmarks/2026-10-02/startup-reclaim-scale/README.md).
All 2400 restores and controls at C50/C100 pass, but candidate paired means
and P99 are worse in all four pairs. At C100, P99 is 8656.581 ms versus
1410.779 ms, despite lower held PSS. These larger-burst regressions outweigh
the earlier C8 observations for adoption. Both HyperMachine variants perform
the same clock/RNG maintenance; direct Firecracker controls omit that
operation, so their startup contract is different and a fully matched service
comparison remains required. Raw earlier evidence remains preserved.

A [matched guest clock/RNG comparison](benchmarks/2026-10-02/prepared-contract/README.md)
now sends the Firecracker control a `Restored` notice with current host time
and 64 fresh entropy bytes before the state command, matching HyperMachine's
built-in creation maintenance. All 1124 restores pass at smoke/C8/C100 with
cleanup verified. C8 median readiness is nearly equal (122.771/121.982 ms);
HyperMachine P99 is lower (551.961/952.504 ms) but total PSS is higher. At
C100 HyperMachine P50/P99 are 1687.128/5739.314 ms versus
1133.562/1423.038 ms, and total PSS is 336.362 versus 285.053 MiB.
The maintenance contract is now matched, while HTTP-daemon/fresh-VMM paths,
kernel arguments and hypervisor implementations still differ. Earlier direct
controls and raw evidence remain preserved; no managed-platform or universal
performance claim follows.

[Client phase accounting at C100](benchmarks/2026-10-02/prepared-phases/README.md)
adds monotonic response-boundary timestamps without extra guest RPCs. Two
cohorts pass all 1600 restores but do not reproduce the earlier multi-second
tail. Among HyperMachine attempts of at least one second, creation/readiness
accounts for 60.25%/64.12% of summed time and execution for 39.75%/35.88%.
Both phases are material; client timestamps do not identify server CPU,
queueing or guest-scheduling causes, or establish an optimization benefit.

[Owned server readiness traces](benchmarks/2026-10-02/prepared-stages/README.md)
correlate all 800 successful HyperMachine children across two logging-enabled
C100 cohorts; all 1600 C100 restores and four smoke restores pass with cleanup.
Multi-second tails are observed again (HyperMachine P99 5764.574/5811.669 ms).
Blocking queue wait contributes 1.63%/1.10% of total time, connection
21.59%/20.84%, clock/RNG acknowledgement 11.87%/10.80%, other creation
38.75%/35.68%, and execution 26.15%/31.58%. Existing stage logs narrow the
diagnostic search, but logging can perturb timing and these are not ranking
cohorts. Neither internal root cause nor an optimization benefit is proved;
the accepted runtime remains unchanged.

[Creation-stage diagnostics](benchmarks/2026-10-02/prepared-creation/README.md)
use existing accepted-daemon build/launch/agent/setup logs. All 1604 restores
pass with cleanup, and 802 successful HyperMachine attempts correlate to
creation and readiness records. Build plus launch accounts for 3.39%/3.41%
of summed readiness time across two C100 cohorts; agent answering accounts
for 40.76%/38.98%, outside logged creation for 23.18%/32.17%, and execution
for 32.57%/25.41%. This narrows the next experiment toward simultaneous
guest readiness rather than launch offloading. Logging can affect timing;
internal root cause and optimization benefits remain unproved.

An isolated [sixteen-slot prepared admission candidate](benchmarks/2026-10-02/restore-admission/README.md)
passes all 3200 matched paired C100 restores. Paired means improve four of four
times, while tails improve only two of four and held PSS only two of four.
First-cohort P99 regresses from 1040.757 to 1278.194 ms; the repeat reverses
that ranking, with a materially faster Firecracker control in one pair.
Default adoption is rejected because tail benefits are inconsistent and the
shared-host comparison cannot isolate all effects. A separate diagnostic
verifies 64 injected launch failures followed by 128 passing restores and
cleanup; queued-request cancellation remains untested. Accepted runtime is
unchanged.

A subsequent [owned heap-reclamation diagnostic](benchmarks/2026-10-02/heap-reclaim/README.md)
passed all 432 guest attempts and all post-probe guest checks. At C100, two pairs
observed approximately 100–112 MiB reclaimable while guests remained running,
after no-op adjustment. This identifies a memory optimization candidate; the
diagnostic helper changes the fixture, and concurrent latency, lifecycle and
competitor comparisons remain untested. No heap policy or default was adopted.

An isolated [250 ms periodic-reclamation candidate](benchmarks/2026-10-02/periodic-heap/README.md)
passed all 800 C100 attempts and reduced held PSS in four of four pairs, but
increased pooled P50 by 23.9% and P99 by 103.6%; only one paired mean improved.
That tested policy was rejected and is not part of the deployed daemon. The
memory gap remains; reclaimability alone does not establish a performance win.

A [reclamation candidate guarded by the entire cold-start budget](benchmarks/2026-10-02/guarded-heap/README.md)
verified seven reclamation calls with no admitted-cold overlap, failure recovery
and snapshot resume. Its C100 cohort retained 725/800 passes and 75 disabled-worker
creation failures. Only one of three complete pairs improved mean readiness and
a different one improved held memory; none improved both. The guarded policy was
not adopted, and the pass-count difference establishes no reliability fix.

- Open source and self-hosted, down to the VMM.
- Drop-in for the E2B SDKs, so existing agent code needs no changes.
- Per-sandbox egress policy with a decision log.
- Credential injection and workload identity at the gateway, so tokens never enter the VM.
- Live shared volumes.
- Paused sandboxes resume on any node.

## Competitive verification work (2026-09-30)

The scope now includes [CubeSandbox](https://github.com/TencentCloud/CubeSandbox),
[Agent Substrate](https://github.com/agent-substrate/substrate),
[E2B](https://e2b.dev/), [Daytona](https://www.daytona.io/docs/en/sandboxes/),
[Blaxel](https://blaxel.ai/platform/sandboxes),
[Modal](https://modal.com/blog/scaling-to-1-million-concurrent-sandboxes-in-seconds),
and [Firecracker](https://github.com/firecracker-microvm/firecracker/blob/main/SPECIFICATION.md).
Firecracker is an engine baseline; the others are sandbox platforms.

**No across-the-board win is established.** Published figures use different
hardware, guest images, concurrency, and readiness criteria. In particular,
HyperMachine's 3.19 ms minimal-unikernel boot is not a Linux/Python sandbox
creation benchmark. Its measured SDK creates are 59 ms for base and 91 ms
for Python. Nested KVM measurements are not substitutes for bare-metal ones.

| Product | Published or repository measurement | Feature comparison target | Comparable win verified? |
|---|---|---|---|
| HyperMachine | Historical nested-KVM SDK create: base 59 ms, Python 91 ms; template restore to answering agent median 12.9 ms | VM lifecycle, checkpoints, forks, egress policy, workload identity, shared volumes | Baseline only; fresh measurements below must specify hardware and workload |
| CubeSandbox | Advertises startup below 60 ms; 50 concurrent starts average 67 ms, P95 90 ms, P99 137 ms on its benchmark host | E2B compatibility, ARM64, BPF networking, secret injection, Kubernetes/Terraform, object-storage resume | No matched run |
| Agent Substrate | Advertises resume below 500 ms and more than 500 activations/second | Kubernetes, gVisor/microVM backends, stateful oversubscription | No matched run |
| Blaxel | Advertises approximately 25 ms stateful resume | Managed lifecycle and automatic suspension | No matched run |
| Daytona | Advertises sandbox startup below 90 ms; default sandbox is a container | Linux/Windows VMs and GPU workloads | No matched run; container startup differs from VM readiness |
| Modal | Reports below 500 ms median API-to-user-code latency in its million-concurrent-sandbox benchmark | Managed fleet scale and GPU workloads | No matched run or equivalent fleet |
| E2B | No precise current latency verified in this review | SDK compatibility and managed execution | No matched run |
| Firecracker | Specification targets at most 125 ms from InstanceStart to init and at most 5 MiB VMM overhead for its specified minimal guest | Engine isolation and efficiency | Matched native cold bursts at concurrency 1/8/50/100 passed 1800/1800 attempts per engine; Firecracker was faster in every profile. At concurrency 100, HyperMachine P99 was 10.71 s versus Firecracker 7.18 s. Earlier failures remain recorded; bounded passes do not establish universal reliability. [Current-daemon refresh](benchmarks/2026-10-02/current-native-engines/README.md): HyperMachine 218/318 attempts passed versus Firecracker 318/318, with a failed HyperMachine 100-guest batch; Firecracker has lower successful P50 in all profiles. Specification metrics differ from application readiness and PSS |

These figures describe the linked providers' own claims or repository runs,
not an independently reproduced ranking. A lower headline number does not
pass the acceptance criteria below.

| Workstream | Acceptance criterion | Current gap |
|---|---|---|
| Creation and execution latency | Same guest workload and readiness command; raw samples, failure rate, P50/P95/P99 at concurrency 1, 8, 50 and 100; lower latency than each tested competitor with repeatable results | Native Firecracker sweep verified at all four concurrencies; HyperMachine still trails. Managed competitor runs and bare-metal HyperMachine runs are missing |
| Stateful resume, pause and fork | Verify live process memory and filesystem state, then measure API-to-first-successful-command latency under the same concurrency | Paired synchronized SDK sweep at concurrency 1/8/50/100: event-preserving KVM runtime passed 1008/1008, parent 1007/1008. Separate synchronized pause/resume bursts passed 503/504 with one resume timeout retained. Pending-event restoration is verified by a direct guest-handler control. A reliability SLA and matched competitor runs remain missing |
| Memory and density | Incremental PSS after the same command and idle period; same guest resources; document shared-template memory; preserve state through oversubscription | Fixed five-second idle PSS and per-batch empty-node baselines verified at concurrency 1/8/50/100, 477/477 attempts per engine; HyperMachine held PSS was higher in all 12 paired batches. Retained-memory allocation source, shared-template attribution and stateful density remain unverified. Vendor VMM overhead and PSS are different quantities |
| Throughput and tails | Sustained arrivals on identical host resources; include failures, queueing, and recovery rather than counting accepted requests | Matched short fixed-rate schedules at 5/25 arrivals per second passed 560/560, including client queue delay and full cleanup/drain. HyperMachine trails Firecracker in both profiles. Longer arrivals, overload failures, load-change recovery and equivalent fleet scale remain unverified |
| CLI and SDK usability | Shipped client for lifecycle, execution, files and checkpoints, tested against a real node and control plane | VM CLI verified on a real KVM node and authenticated control plane, including binary files through the control-plane proxy |
| Isolation and governance | Enforced tenant boundaries, scoped expiring keys, roles, auditable access, and escape tests | Scoped expiring keys, [single-team observer/operator roles](API_KEY_ROTATION.md#single-team-observer-and-operator-roles), and [durable protected-API admission/completion records](ACCESS_AUDIT.md) are verified, including restart, storage failure and KVM/TLS lifecycle requests. Tenant boundaries, team membership/sharing, retention automation and guest/proxy activity auditing remain incomplete; [opt-in keyed sandbox target references](ACCESS_AUDIT.md) are verified on protected control-plane routes, while creation-result, template/volume and proxy attribution remain incomplete |
| Networking and access | Custom domains, certificate automation, authenticated private URLs, SSH, raw TCP/UDP and isolated VM groups | Custom-domain HTTP/HTTPS routing, authenticated raw TCP and egress are implemented. [The API socket fix](benchmarks/2026-10-01/tcp-api-buffering.md) reduced observed 1 MiB native TCP median latency from 48.5–50.8 ms to 7.0–7.9 ms in bracketed runs (1600/1600 transfers verified). That sequential comparison did not establish a median win over Firecracker; large tails and TLS performance remain unresolved. [Eight-stream controls](benchmarks/2026-10-01/tcp-concurrent.md) corrected the shared guest-agent backlog and verified 1920/1920 transfers on both native paths; observed median/tail rankings vary by profile. A [Unix relay prototype](benchmarks/2026-10-01/tcp-unix-relay.md) passed 5440 transfers but did not establish a repeatable gain and was reverted. [Private browser URLs and exact sandbox-ID sharing](benchmarks/2026-10-02/private-web-scopes/README.md) pass 25 KVM/TLS checks; SSO, self-service sharing and isolated groups remain absent. [Owned certificate discovery/issuance](benchmarks/2026-10-03/discovery-tls-kvm/README.md), [scheduled renewal](benchmarks/2026-10-03/tls-renewal-worker/README.md), and [sixteen native TCP/UDP ports](benchmarks/2026-10-03/native-sixteen-tcp-udp-capacity/README.md) are now verified; public CA/Internet operation remains unverified; [control-plane named creation](benchmarks/2026-10-02/reserved-create/README.md) is KVM/TLS verified, [fork name inheritance is fixed and KVM-verified](benchmarks/2026-10-02/fork-name/README.md), [node-side atomic completion](benchmarks/2026-10-02/node-completion/README.md) is verified, as is [committed-name recovery after response loss and timeout](benchmarks/2026-10-02/node-response-loss/README.md), while fleet name migration and in-flight recovery remain incomplete; [reserved aliases for existing VMs](benchmarks/2026-10-02/reserved-alias/README.md) pass real KVM/TLS verification |
| Platforms and workloads | Verified ARM64 execution, GPU sandboxes, browser/desktop workloads, and persistent storage limits | ARM64 execution and GPU sandbox wiring remain unverified or absent |
| Operations | Object-storage backups and recovery, quota enforcement, scheduling/event triggers, load-tested multi-node failover | [Encrypted offline snapshot-store S3 recovery](OBJECT_STORAGE_BACKUPS.md) is verified against an owned S3 emulator with real KVM state, named snapshots and mounted volumes. Managed-store durability/IAM, backup automation, external cluster metadata recovery and load-tested failover remain incomplete; shared-directory snapshots and host job queues do not cover all these capabilities |

Mark a capability complete only after checking its effect through a shipped
interface. Mark a performance win only after equivalent runs establish it;
do not infer it from a vendor headline or a component microbenchmark.

The [fixed-idle memory comparison](benchmarks/2026-10-01/idle-memory.md) records
held totals, same-batch increments, individual idle ages and post-cleanup
baselines. At concurrency 100, median held PSS was 8503.60 MiB for HyperMachine
versus 8360.42 MiB for Firecracker; median same-batch increments were 8320.55
versus 8360.42 MiB. The difference reflects a daemon baseline that grew after
earlier batches. Three alternating pairs and shared nested hardware do not
establish a repeatable memory or density win.

Follow-up [mapping diagnostics](benchmarks/2026-10-01/memory-retention.md)
verified that KVM VM/vCPU handles and mappings of at least 1 GiB disappeared
after each eight-guest batch, while smaller anonymous mappings remained
resident. A separate arena-limited diagnostic reduced median post-cleanup
process PSS from 193.52 to 110.02 MiB across three batches. All 48 guest attempts
passed, but the runs were sequential and had no matched latency/tail comparison.
This suggested an allocator candidate; the controlled evaluation below did not
support adoption. Production defaults remain unchanged.

That candidate was then [counterbalanced at concurrency 100](benchmarks/2026-10-01/allocator-experiment.md)
across four blocks and 3200 attempts. HyperMachine's arena-limited process PSS
was higher in all three complete paired blocks, and its mean readiness was
lower in only one. Both engines had readiness failures (3029/3200 attempts
passed overall), with clean teardown and unchanged artifacts. The arena limit
was not adopted; neither a memory win nor a reliability fix is established.

A separate [boot-sizing allocation candidate](benchmarks/2026-10-01/boot-sizing.md)
removed temporary kernel/initrd copies during Linux memory sizing. Allocation
and guest-state checks passed, followed by 3328/3328 passing, cleaned-up native
attempts against a matched current-parent build. At concurrency 8 both paired
blocks improved readiness and memory. At concurrency 100 the candidate was
slower in three of four blocks, with a median paired mean-readiness increase
of 1455.22 ms and inconsistent memory changes. The runtime change was reverted;
exact candidate sources, tests, raw reports and comparison tables are retained.
This micro-level allocation reduction did not establish a product improvement.

A [refused-agent connection backoff candidate](benchmarks/2026-10-01/refusal-retry.md)
was evaluated against a current-source release baseline with synchronized
command rounds at concurrency 1/8/50/100, two alternating pairs per profile.
All 15600 commands and 636 guest preparations passed with clean teardown,
but candidate mean latency was higher in seven of eight pairs. The change was
reverted. Exact candidate source/tests, build hashes, raw cohorts and the
initial permission-related setup failures are retained; no command attempts
were made in those setup failures. Production keeps the prior retry cadence.

The [synchronized stateful SDK sweep](benchmarks/2026-10-01/stateful-sweep.md)
now prepares whole resume/fork batches before timing them, with observed client
start spreads. Resume passed 504/504; fork passed 503/504 at concurrency
1/8/50/100. Every successful sample checks retained live-process memory and
filesystem state; forks also check an independent child write and unchanged
parent state. At concurrency 100, conditional P99 readiness was 1089.49 ms for
resume and 1064.27 ms for fork. One concurrency-1 fork missed the unchanged
15-second child-agent readiness deadline; its nonzero cohort and interrupt
diagnostics remain recorded. Teardown and artifact checks passed throughout.
The initial 636/636 passing sweep is retained too. This expands verification;
it establishes neither universal reliability nor a matched competitor win.

New KVM snapshots now [preserve vCPU event handoffs](benchmarks/2026-10-01/kvm-events.md)
alongside LAPIC state. A direct real-KVM control verifies the restored interrupt
enters a guest handler, while the legacy omission takes the main path; NMI,
shadow and exception state also round-trip. Real checkpoint/resume/fork checks
passed. The paired SDK sweep retained all 2016 attempts: updated runtime
1008/1008, parent 1007/1008, with the parent's resume timeout kept intact.
Timings are mixed, including a higher updated resume P99 at concurrency 100.
This fixes the demonstrated event-state omission, without establishing the
cause of earlier timeouts, a speed win or universal reliability. Old snapshots
remain readable but require recapture to include event state never stored.

The separate [synchronized pause/resume sweep](benchmarks/2026-10-01/pause-bursts.md)
prepares all live-state probes before concurrently pausing each batch, verifies
paused state, then synchronizes resume independently. It passed 503/504 across
concurrency 1/8/50/100. At concurrency 100, conditional pause P99 was 961.88 ms
and resume-to-command P99 was 1095.98 ms. One concurrency-1 resume timed out
despite event preservation; its raw diagnostics and nonzero cohort are retained.
This closes the pause-burst measurement gap without proving a reliability fix
or a comparative performance win.

A [timer-only hardware control](benchmarks/2026-10-01/deadline-timer.md) restores
a halted real-mode guest with a captured TSC deadline and verifies entry into
its interrupt handler. Omitting the deadline leaves the control halted until
the bounded test kicks and joins its runner. Both controls passed, narrowing
the resume investigation without proving the cause of the retained timeout.
The extended control also measures that guest TSC has passed the captured
deadline before first entry, then still reaches the handler. This verifies
that specific delayed-start wakeup window without explaining the SDK failure.

Native-engine runs now explicitly cap both guests' readiness phase at 15 seconds.
Firecracker's cap begins after `InstanceStart` and is also bounded by its total
startup deadline (30 seconds by default). Older cohorts used HyperMachine's
15-second guest limit but Firecracker's remaining total startup budget; retain
that limitation when interpreting their failure rates. The next reports record
these budgets and the driver's actual CPU affinity. The generic host limitation
no longer says every run is unpinned: pinned coordinator profiles already record
their affinity separately. Eleven harness tests pass, including guest-deadline
clipping and cleanup on timeout. These methodology changes do not establish a
performance or reliability improvement.

### VM client and readiness benchmark

The [fixed-rate native comparison](benchmarks/2026-10-01/fixed-arrivals.md)
adds planned arrival times, submission lag, client worker queues and complete
lifecycle cleanup/drain to the engine harness. Two alternating pairs at
5 and 25 offered arrivals/second passed all 560 attempts. At the higher rate,
scheduled command P99 was 2683.92 ms for HyperMachine versus 2034.47 ms for
Firecracker; both accumulated queues with eight workers. Cleanup occupies
workers, so these short runs do not measure server-only maximum capacity or
prove sustained fleet throughput. Longer overload and recovery work remains.

Build the client with `cargo build -p hm-cli --bin hm`. It talks to sandboxd
or the control plane, selected by `--endpoint` or `HV2_SANDBOX_URL`.
`HV2_API_KEY` supplies authentication without putting the key in shell arguments.
Use TLS when connecting to a remote endpoint.

```sh
export HV2_SANDBOX_URL=http://127.0.0.1:3980
hm sandbox vm create --template base
hm sandbox vm list
hm sandbox vm exec SANDBOX_ID -- /bin/sh -c 'printf hello'
hm sandbox vm --endpoint https://sandbox-api.example.com tcp SANDBOX_ID --port 5432 --listen 127.0.0.1:15432
hm sandbox vm files SANDBOX_ID --envd-endpoint ENVD_URL upload local.bin /root/file.bin
hm sandbox vm files SANDBOX_ID --envd-endpoint ENVD_URL download /root/file.bin downloaded.bin
hm sandbox vm checkpoint save SANDBOX_ID before-change
hm sandbox vm checkpoint restore SANDBOX_ID before-change
hm sandbox vm pause SANDBOX_ID
hm sandbox vm resume SANDBOX_ID --lifetime 300
hm sandbox vm fork SANDBOX_ID --count 2
hm sandbox vm delete SANDBOX_ID

hm sandbox vm benchmark --template base --samples 100 --concurrency 8 \
  --environment 'CPU model; RAM; OS; nested/bare-metal; image hash; daemon commit' \
  --require-snapshot --max-p99-ready-ms 150 > readiness.json
```

The benchmark includes CLI HTTP calls, guest execution, and client-side
queueing within each sample's creation-to-ready interval. A sample is ready
only when `/bin/sh` produces its expected unique marker with exit 0 and no
timeout. Creation and first-execution times are reported separately.
Every known sandbox is deleted, including after a failed readiness command;
a cleanup error fails the sample. Raw records and nearest-rank percentiles
are JSON. An empty successful sample set has null latency statistics, not
zero. Any failed sample or exceeded threshold makes the CLI exit nonzero.
The throughput field measures complete create/execute/delete lifecycles,
including cleanup; it is not a create-only throughput figure.

The 150 ms command above demonstrates setting a gate; it is not evidence of
a performance win or a universal target. No warmups are discarded. Template
preparation happens outside the run and must be reported separately. This
benchmark uses HyperMachine's `/exec` extension, so other providers require
equivalent execution adapters before their results can be compared. It does
not measure memory, stateful resume, or general application initialization.
Before timing samples it queries `/templates` and records the selected template's
server metadata, including snapshot availability, CPU and memory sizes. Failure
to obtain metadata is recorded explicitly. `--require-snapshot` refuses to create
any guests unless the server confirms snapshot mode; omit it when intentionally
measuring cold boots. Errors include their underlying cause.

### Fresh KVM readiness baseline

File transfers require a reachable sandbox envd listener or proxy URL. The client
calls the control API's `connect` operation to retrieve the sandbox token, resumes
paused sandboxes and extends their lifetime to at least 300 seconds. A separate
HTTP client sends that token to envd without forwarding `HV2_API_KEY`; redirects
are disabled. Binary contents and URL-special path characters are preserved.
Transfers are bounded to envd's 512 MiB limit. Downloads stream to a temporary
file beside the destination and publish only after completion, refusing to
overwrite even if another process creates the destination during the transfer.
Use TLS for remote envd URLs. A node's direct envd listeners bind on loopback;
remote clients need the proxy. The local KVM check verified a 256 KiB binary
upload/download and guest SHA-256, plus refusal to overwrite the downloaded file.
Protocol fixtures also checked HTTP failures and a concurrent destination creator.
For a proxy reached by IP or a shared hostname, `--envd-host` sets the sandbox's
virtual host while TLS still authenticates the supplied endpoint URL. The
`tools/e2e-sandbox-vm-cli.py --envd-proxy URL` check exercised binary transfers
through the real control-plane proxy, with an API key and separate cluster token.
Lifecycle, checkpoint rollback, pause/resume, forked state and cleanup also passed
through that control plane. Checkpoint routes require the API key and forward
to the sandbox's owning node with the cluster token.

The control plane's `/templates` now reports snapshot state and guest sizes from
node heartbeats. Snapshot readiness is true only when every offering node confirms
it, false if any reports cold boot, and null when confirmation is missing.
CPU/memory sizes are null unless all offering nodes report identical sizes;
per-node details remain visible. Legacy heartbeats remain readable and are
treated as unknown, so `--require-snapshot` cannot infer readiness from a name.
Advertisements update names and metadata together. These are point-in-time
heartbeat observations; they do not guarantee a fleet cannot change afterward.

The shipped Windows debug client was checked against the release daemon on
2026-09-30 using `tools/e2e-sandbox-vm-cli.py`. Quoted arguments remained
literal, guest exit 7 propagated, checkpoints restored a changed file,
pause/resume preserved it, both forks inherited it, and deletion cleaned up
the created guests. This verifies the node interface, not the control plane.

Eight runs of 100 samples each completed with **zero failed samples**.
Raw JSON is in [benchmarks/2026-09-30](benchmarks/2026-09-30/), named
`readiness-cN.json` and `readiness-prefault-cN.json`.

| Concurrency | Default P50 / P95 / P99 ready (ms) | Prefault P50 / P95 / P99 ready (ms) |
|---|---|---|
| 1 | 67.37 / 103.12 / 142.32 | 44.88 / 205.65 / 216.17 |
| 8 | 113.36 / 140.62 / 149.04 | 53.94 / 106.48 / 121.42 |
| 50 | 947.43 / 1493.28 / 1641.66 | 882.99 / 1363.84 / 1453.71 |
| 100 | 2679.86 / 2934.73 / 3038.36 | 1848.60 / 2132.19 / 2153.59 |

Hardware: Ryzen 9 9900X, 24 logical CPUs, 45.9 GiB available to WSL2 Debian,
nested Hyper-V/KVM, one vCPU and 1024 MiB per guest, prewarmed BusyBox/static
agent template, networking disabled. Kernel and initrd hashes, client build
mode and run timestamps are recorded in each JSON. The daemon code is based
on commit `2dbf31f`; the client includes the subsequent metadata/default-port
fixes on this branch. Other host workloads were active. Runs were sequential,
with default runs preceding prefault runs, so host variation and order effects
prevent attributing the differences solely to prefaulting. No default changes
or competitor wins follow from this experiment. High-concurrency tail latency
remains an optimization target; dedicated-host repeats and equivalent provider
runs are required.

A subsequent diagnostic daemon startup failed its 120-second template-agent
readiness check and fell back to cold boots. Its API was not yet available when
the diagnostic benchmark began: all 100 requests failed, with null latency
statistics. This is an additional startup-reliability gap, not a measured restore
latency. The original eight runs explicitly announced snapshot-backed templates.
The new snapshot gate prevents comparing an unnoticed fallback with those runs;
the underlying intermittent guest startup failure still needs diagnosis.
Operators can now start `hv2-sandboxd --require-template` to fail startup if
any configured snapshot template cannot be prepared, before API/proxy listeners
or cluster registration. It conflicts with `--no-template`. The default still
permits cold-boot fallback. `tools/e2e-template-policy.py` checks failure behavior
and, with `--check-ready`, verifies successful snapshot-backed startup using
the images supplied through `HV2_KERNEL` and `HV2_INITRD`.
The conflicting-option, strict missing-image failure and default fallback checks
passed against both Windows and Linux daemon binaries. Successful strict startup
with a snapshot-backed template passed on a real KVM guest.

A diagnostic repeat using strict startup and the client snapshot gate completed
20 samples at concurrency 1 and 100 at concurrency 100 with no failures. P99
readiness was 54.03 ms and 1327.48 ms respectively. Raw reports are
`readiness-strict-stages-c1.json` and `readiness-strict-stages-c100.json`; the
concurrency-100 internal timings are in `restore-stages-c100.log` in the same
benchmark directory. The replacement image hashes match the original images.
At concurrency 100, internal P50/P99 build time was 12.16/56.92 ms, launch was
30.68/76.88 ms and guest-agent readiness was 699.67/1030.54 ms. These stage
percentiles are independent and must not be added. Guest readiness dominates
the measured create path and is the next profiling target. Host workloads and
logging settings changed between cohorts, so this is diagnostic evidence,
not proof of an optimization or competitor win.

The authenticated control-plane diagnostic cohort completed 20 samples at
concurrency 8 with no failures, server-confirmed snapshot mode, P50 readiness
223.15 ms and P99 325.55 ms. Raw evidence is
`benchmarks/2026-09-30/readiness-control-plane-c8.json`. Its metadata includes
the offering node, guest sizes and snapshot state. This validates the shipped
benchmark through the cluster interface; the small shared-host cohort does not
establish a latency advantage over other products.

## Shared SDK comparison harness

`tools/bench-e2b-sdk.py` measures create-to-verified-command readiness through
the unmodified [E2B Python SDK 2.51.0](https://pypi.org/project/e2b/2.51.0/).
Install it in an isolated virtual environment. Configure `E2B_API_KEY` in the
environment and supply explicit endpoints; leave `E2B_API_URL`,
`E2B_SANDBOX_URL` and `E2B_ENVD_POOL_SHARDS` unset. For the local fixture:

```powershell
python tools/bench-e2b-sdk.py --provider HyperMachine `
  --api-url http://localhost:14095 --sandbox-url http://localhost:14096 `
  --template base --environment 'Describe the actual client and host' `
  --image-description 'Describe the matched image and preparation' `
  --expected-cpus 1 --expected-memory-mb 1024 --samples 100 --concurrency 8
```

Use `--operation resume` or `--operation fork` to measure stateful lifecycle
readiness. Before the timed request, the harness starts a long-lived process
with a unique marker in its environment and writes a file and boot identity.
The first command after the operation must verify all three plus the original
live PID. Resume requires the API to report `paused` before connecting; fork
requires a distinct child ID, changes and verifies the child's private state
file, then verifies that the parent retains its original process and file state.
The child-write isolation check is outside readiness timing. It expects a
private `/tmp` filesystem; explicitly shared volumes require separate semantics.
These probes detect cold boots and lost process state; they do not exhaustively
validate every application or open socket. Creation and preparation are outside
resume/fork readiness timing. Lifecycle throughput includes them, state checks,
resource inspection and deletion. `pause_ms` measures the pause response alone.

For compatible managed providers, omit `--sandbox-url` to use their sandbox
domains. The harness disables internet access and retries, forces normal SDK
mode so deletion actually occurs, checks server-reported CPU/RAM, and deletes
every known sandbox. Incorrect output, resource mismatch or cleanup failure
fails the run. Dependency versions are captured before execution. Reports also
record `harness_sha256` for the exact script bytes (including checkout line
endings) and `harness_unchanged_during_run`. A source change during the run
fails its overall exit status even when every sample succeeds. This detects
ordinary edits, not malicious source replacement or guest-image equivalence.
Historical reports without these fields retain their original evidence; no
fingerprint is retroactively attributed to them. Failure accounting and
provenance are checked by `tools/test-bench-e2b-sdk.py` (19 tests). The CI
`Sandbox Benchmark and Chart Gates` job runs these tests without
SDK/cloud credentials, and also runs the chart render tests and Helm lint.
The default workload verifies POSIX command readiness; `--workload python` requires a real
Python image. Unknown IDs after an interrupted create cannot be cleaned up by
the client. Provider labels and guest sizes do not establish matched hardware,
image contents or snapshot preparation; those need independent evidence.

The first recorded SDK cohort completed 20/20 samples at concurrency 8 with
1 vCPU and 1024 MiB: readiness P50 **260.40 ms**, P95 **491.52 ms**, P99
**511.19 ms**. Raw evidence is `benchmarks/2026-09-30/readiness-e2b-sdk-c8.json`.
It uses Windows SDK transport, a BusyBox snapshot guest, debug profiling and
a shared nested-KVM host. It is distinct from the native CLI cohorts and does
not demonstrate a performance improvement. Lifecycle throughput also includes
the resource-info check and deletion. All sandboxes were confirmed deleted.

Correlated internal stages for the same 20 IDs are in
`benchmarks/2026-09-30/readiness-e2b-sdk-stages-c8.json`: blocking queue P50/P99
0.08/6.99 ms, guest connection 61.34/134.18 ms, restored clock/entropy exchange
15.77/31.93 ms. Independent percentiles must not be added. The connection
stage is the largest of these measured stages; this does not explain all SDK
latency or establish the cause of earlier concurrency-100 tails. Debug events
record timings and VM IDs without entropy, tokens or request contents.

This profiling exposed a restore-clock correctness issue: host time was
captured before worker scheduling and guest connection, so those waits became
clock lag when the guest applied the timestamp. The restored guest now receives
time sampled on the connected channel. Invalid host clock ranges fail instead
of becoming zero or wrapping. Protocol tests verify sampling after channel
creation, entropy preservation and propagation of RNG reseed failure. Remaining
transport and guest-processing delay is not compensated; this is a correctness
fix, not evidence of a latency reduction or competitor advantage.
The corrected release daemon passed five real SDK creates, five stateful
resumes and five stateful forks at concurrency 1, with no failures and all
known sandboxes deleted. Raw validation reports are
`benchmarks/2026-09-30/readiness-e2b-sdk-fresh-clock-{create,resume,fork}-c1.json`.
These small cohorts validate runtime compatibility and state preservation;
the protocol tests establish timestamp placement, not a measured bound on
guest clock offset. All 13 guest-protocol tests and strict agent Clippy passed.

No competitor endpoints or matched host are available from the user. Published
competitor claims remain separate from measured HyperMachine results; universal
feature or performance superiority is unverified.

Stateful SDK diagnostics on the same shared host completed 20/20 samples per
operation at concurrency 8, with every known sandbox confirmed deleted:

| Operation | Readiness P50 | P95 | P99 | State evidence |
|---|---:|---:|---:|---|
| Resume | 129.73 ms | 183.29 ms | 188.91 ms | Explicit paused state; original live process, memory marker, file and boot identity |
| Fork | 126.63 ms | 183.93 ms | 186.61 ms | Distinct child; preserved child and parent process/memory/file/boot state |

Raw reports are `benchmarks/2026-09-30/readiness-e2b-sdk-resume-state-c8.json`
and `readiness-e2b-sdk-fork-state-c8.json`. Earlier exploratory cohorts without
the explicit paused/running-state gate remain in `readiness-e2b-sdk-resume-c8.json`
and `readiness-e2b-sdk-fork-c8.json`; do not combine their percentiles. Neither
cohort is a matched competitor comparison or proof of a performance improvement.

A stronger fork cohort completed 20/20 samples at concurrency 8, each with
`fork_filesystem_isolation_verified: true`: child mutations did not change the
parent's saved file, and both retained their live process state. All parent
and child sandboxes were confirmed deleted. Raw evidence is
`benchmarks/2026-09-30/readiness-e2b-sdk-fork-isolation-c8.json`, including harness
fingerprint and unchanged-source confirmation. Readiness P50/P95/P99 was
125.49/193.95/194.29 ms. This cohort used warning-level daemon logging on the
shared nested-KVM host; do not infer a latency change from earlier cohorts
with different logging and workloads. Extra isolation commands are included
in lifecycle throughput, not readiness latency. Earlier reports without the
isolation field do not establish this new check. This probes filesystem fork
independence, not comprehensive security isolation or shared-volume behavior.

## Scoped, expiring control-plane keys

`hv2-control-plane --api-keys-file keys.json` loads an operator-provisioned
policy array before opening listeners or the store. Each entry requires
`sha256` (64 hex characters), `expires_at` (Unix UTC seconds) and a nonempty
`scopes` array. Use a unique, randomly generated high-entropy key for each
credential; the policy holds only its digest. For example, with a credential
already configured in `HV2_SCOPED_API_KEY`, generate a one-hour inventory policy:

```python
import hashlib, json, os, time
from pathlib import Path
policy = [{"sha256": hashlib.sha256(os.environ["HV2_SCOPED_API_KEY"].encode()).hexdigest(),
           "expires_at": int(time.time()) + 3600, "scopes": ["inventory"]}]
Path("keys.json").write_text(json.dumps(policy))
```

Clients send the original credential as `X-API-Key`; the existing SDK and CLI
authentication works unchanged. Expiration is checked on every protected API
request, including existing client connections. Missing, wrong and expired
credentials return 401; a valid key without the required scope returns 403.
Malformed, empty and duplicate-key policies fail startup rather than disabling
authentication. A maximum of 256 policies is accepted.

| Scope | Capability |
|---|---|
| `inventory` | GET/HEAD sandbox listings, template listings, aggregate sandbox metrics and cluster nodes; no sandbox detail or access tokens |
| `sandboxes` | Sandbox API operations, including create, detail/access token retrieval, execution, lifecycle, checkpoints and networking |
| `templates` | Template and snapshot APIs, including builds, uploads and deletion |
| `volumes` | Volume management and its credentials |
| `events` | `/events` APIs, including webhook management and delivery inspection |
| `admin` | All protected control-plane APIs |

Scopes combine and apply to the entire configured team, with no tenant or
per-sandbox restriction. Inventory deliberately excludes sandbox detail,
volume APIs and webhook APIs: their credentials can grant mutation through
other interfaces. Public health/metrics/OIDC endpoints and existing bearer-token
upload/content routes retain their existing authentication model. Expiring an
API key does not revoke previously issued envd, upload or volume bearer tokens,
terminate running guest operations, or expire a distinct legacy `HV2_API_KEY`
admin credential. Startup rejects an admin key whose digest appears in any
scoped policy, including an expired one. Embedded library instances that skip
startup validation enforce the scoped policy rather than granting legacy admin
access for an overlapping credential. Nodes in a
cluster must require their cluster token so direct node calls cannot bypass the
control plane. Policies load at startup; rotation requires restarting every
control-plane replica with the same updated policy. Dynamic key administration,
immediate bearer revocation and tenant roles remain open. Single-team observer/operator roles are described in [key policy configuration](API_KEY_ROTATION.md#single-team-observer-and-operator-roles); durable protected-API records are described in [access audit configuration](ACCESS_AUDIT.md).

The sandbox Helm chart supports a read-only policy Secret:

```yaml
auth:
  apiKeysSecret: sandbox-api-policies
  apiKeysSecretKey: keys.json
  legacyAdminEnabled: false
```

Create the Secret from the policy file in the release namespace, then deploy
with these values and a control-plane image built from this revision. The
existing published image tag is not evidence that it supports the new flag.
The chart projects the selected Secret key to `/etc/hv2-api-keys/keys.json`
and passes `--api-keys-file`. Policy-only mode removes `HV2_API_KEY` from the
control-plane environment; cluster authentication and the store password remain.
The default keeps legacy admin authentication. The chart rejects disabling it
without a policy Secret, nonboolean mode values and an empty policy Secret key.
Missing or malformed policy data prevents startup. After changing the policy,
run `kubectl rollout restart deployment/RELEASE-hv2-control-plane` and wait for
the rollout in the release namespace. Every replica must load the same policy;
Secret projection updates alone do not reload it.

`tools/test-sandbox-chart.py --helm HELM_BINARY` verifies default and policy-only
authentication, all eight policy/mTLS/store-TLS mount combinations and invalid
configuration rejection. Four render tests and Helm 3.17.3 lint passed. These
checks parse rendered YAML and validate mount/argument consistency; no live
Kubernetes deployment has been performed for this change.

Validation: 27 cluster library tests, 13 real-HTTP integration tests and strict
cluster Clippy passed. `tools/e2e-control-keys.py --control-plane BINARY` checks
the shipped Windows binary's scope enforcement, live expiry, token-access
denial, fail-closed empty-policy startup and rejection of admin/scoped credential
overlap without modifying user credentials. Startup errors omit the credential.
Real-HTTP node fixtures additionally verify sandbox creation by a scoped key,
token-free inventory, denied writes and legacy admin compatibility. This
validation does not establish multi-tenant isolation or a performance win.
The CI test matrix is configured to build the shipped control-plane binary
and run `e2e-control-keys.py` on Linux, Windows and macOS. Local Windows
execution is verified; remote matrix results remain subject to CI completion.
The first remote matrix skipped these checks after a `setup-protoc` server
error, so it is not cross-platform authentication evidence. CI now installs
checksum-locked protoc 23.4 from fixed official release URLs, without release
enumeration, and preserves independent platform jobs when one fails. The
installer rejects altered archives and unsupported hosts; its platform and
checksum tests pass, and the pinned Windows and Linux x86-64 compilers run
locally. On CI run `36770058106`, pinned compiler installation and execution
passed on Linux, Windows and macOS. Workspace tests and shipped authentication
checks still need their own completed results; successful setup is not proof
of those downstream checks.

### Protected API access records

The control plane emits structured tracing events under `hv2_cluster::access`
for protected API requests. Accepted requests have a start event and a response
event sharing a generated request ID. Denied requests also have response events.
Fields include the route template, a standard HTTP method category, credential
category, HTTP status and elapsed time until the response is constructed.
Configured credentials are identified by the first 16 hex characters of their
SHA-256 digest; unknown credentials are never fingerprinted. Request headers,
bodies, queries, supplied path values and nonstandard method strings are omitted.
The existing info-level logger emits these events by default; an operator can
select them with `RUST_LOG=warn,hv2_cluster::access=info`.

The shipped Windows binary checks verify allowed and denied status records,
expired-key records, matching start/response IDs and absence of fixture
credentials or private request values from the logs. All 13 real-HTTP
authorization tests and strict cluster Clippy passed after this change.

These are diagnostic access records, not a durable or tamper-evident audit
store. Route templates omit resource attribution. Public and independently
authenticated bearer/proxy routes are outside this middleware. A response
record reports headers/status, not completion of a streamed body; interrupted
accepted requests may have only a start event. Retention, comprehensive guest
activity auditing and performance impact under load remain unverified.

## Changelog of this page

- **2026-09-29, checkpoints.** `POST /sandboxes/{id}/checkpoints` saves a running sandbox, and
  `POST .../checkpoints/{name}/restore` rolls it back in place. The sandbox keeps its ID, access
  token and URL, and its memory and filesystem go back to the saved state. List and delete are
  also available, with at most 10 per sandbox; checkpoints end with the sandbox.
  - The replacement guest boots before the old one stops, so a failed restore changes nothing.
  - Checked on KVM guests (`tools/e2e-checkpoints.sh`: 26 checks). The key check: a process killed
    after the checkpoint runs again after the restore.
  - Limitation: checkpoints stay on the node that took them.
- **2026-09-29, idle pause.** A sandbox pauses to disk after `idleTimeout` seconds unused, set per
  sandbox or node-wide with `--idle-pause-after`. With `autoResume`, the next request through the
  proxy wakes it and is answered.
  - "Unused" means three things: no proxy request in flight or begun, no `/exec` running, and every
    CPU sample the node took in the window under 5%. boxd suspends on idleness alone. A long
    build, a training run or a crawler nobody is watching keeps running here.
  - Checked on KVM guests (`tools/e2e-idle-pause.sh`: 11 checks).
  - Two mutations were run: removing the CPU gate pauses the busy guest, and removing the exec
    guard freezes a quiet 50-second command halfway. The checks catch both.
- **2026-09-29, sandbox env vars.** `envVars` on create, as E2B's API has it.
  - They are written into the guest's template defaults, which the guest agent applies to every
    command. That puts them in guest memory, so pause, resume on another node, fork and
    snapshot-templates all carry them.
  - They are write-only: no response returns them.
  - Checked end to end on KVM guests (`tools/e2e-sandbox-env.sh`: 13 checks). The checks cover a
    command seeing the variables, pause and resume, a fork, a sandbox created from its snapshot,
    responses not echoing values, and invalid names being refused.

## Interactive SDK output validation

`tools/e2e-sdk-interactive.py` exercises the unmodified E2B Python SDK 2.51.0
against a real snapshot guest. Each operation starts a fresh process, varies
input arrival by 0–40 ms outside the timer, then measures the input/EOF/signal
request through verified process exit. It checks the exact stdin marker and
exit status, records failures separately, and deletes its sandbox. SIGKILL
must produce exit status 137. Run with explicit API/proxy URLs, `--environment`,
`--label`, and an `E2B_API_KEY` environment variable; `--samples` is per operation.

Successful input, EOF and signal requests now notify that process's output
poller; idle output retains the 50 ms tick. The first faster-poll cohort exposed
an existing guest race: exit could be published before pipe readers collected
final output. The corrected agent waits for reader completion and observes
completion before draining buffers. A Linux regression checks final stdout and
stderr across 100 rapidly polled child processes. Processes whose descendants
retain output pipes keep their streams open until those pipes close.

| Cohort | Successful checks | Stdin P50/P99 | EOF P50/P99 | Signal P50/P99 |
|---|---:|---:|---:|---:|
| Fixed polling, original guest | 60/60 | 43.20/94.78 ms | 33.21/77.16 ms | 41.61/87.18 ms |
| Input wake, original guest | 59/60 | 9.64/63.41 ms | 6.74/61.08 ms | 8.39/63.31 ms |
| Input wake, corrected output drain | 300/300 | 56.14/99.64 ms | 50.18/100.99 ms | 56.31/101.29 ms |

The failed wake-only stdin sample had an output/exit mismatch; its result is
retained and that cohort is invalid as a passing benchmark. Percentiles include
successful samples only, so its lower timings cannot establish an improvement.
The corrected cohort validates compatibility and cleanup, but is slower than
the baseline in these diagnostics. No latency gain is established. Cohorts ran
sequentially on the shared Windows/WSL nested-KVM host with warning logs, one
sandbox, 1 vCPU/1024 MiB, and 20 versus 100 samples per operation. Host contention,
reader scheduling and different guest binaries prevent causal attribution or a
competitor comparison. Notifications remove an intentional wait when queued;
RPC, process execution and scheduling delay remain.

Raw reports under `benchmarks/2026-09-30/` are
`interactive-fixed-poll-c1.json`, `interactive-input-wake-c1.json`, and
`interactive-wake-output-drain-c1.json`. The corrected image SHA-256 is
`1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c`;
the earlier cohorts use the original image documented above. Reports capture
the interactive script digest at completion; unlike the shared readiness
harness, this diagnostic does not check for source edits during execution.
All known sandboxes were confirmed deleted after each cohort.

The full GitHub CI run [36772587222](https://github.com/nervosys/HyperMachine/actions/runs/36772587222)
passed for commit `a422192`, including Linux/Windows/macOS workspace tests,
shipped control-plane authorization checks, benchmark/chart gates and Clippy.
This predates the interactive wake/output-drain change; its new-head CI must
pass separately. Local checks for that change passed the three notification
tests, strict API Clippy, 15 Linux guest-protocol tests, the 100-process regression,
and 300 real SDK operations. New-head CI at `d306f4a` found the regression test
module placed before production functions (`items_after_test_module`). An
all-targets local check found the same issue in the API notification tests.
Moving both modules to the end preserves runtime behavior; strict Linux guest Clippy
then passed locally using an extracted Debian Clippy 1.95 package matching the
installed Rust compiler. Strict API Clippy also passed with all targets included.
The corrected head still needs its own CI result.

### CI baseline comparison reliability

The separate Benchmarks workflow at `a422192` failed in its comparison job
before any baseline ran: both repositories are checked out under `baseline/`
and `pr/`, but the protoc installer was invoked from the empty workspace root.
It now runs from `pr/`; the baseline checkout uses the exact PR base commit,
and the artifact records both actual commits, compiler versions and runner CPU.
Baseline execution, copying and candidate execution failures now fail the job
instead of being ignored. A fixture verified that nested Criterion baseline
files reach an initially absent candidate target directory. Actionlint passed.
Raw comparison estimates, output and environment metadata are uploaded even
on failure. These are sequential crypto microbenchmarks on a shared runner,
with regression reporting rather than a latency gate; they do not establish
sandbox readiness or superiority over any competing product. Remote execution
of the corrected workflow remains pending.

The primary crypto/API benchmark job also separates measurements from the
restored Cargo cache: a fresh `CRITERION_HOME` under the runner temporary
directory is used for execution, conversion and artifact collection. Both
benchmark commands must succeed before results are stored. The converter
rejects missing/empty results, incomplete estimates, nonnumeric/nonfinite or
invalid timing values, invalid names and duplicate names instead of silently
emitting empty or partial results. Fifteen standalone PowerShell fixture checks
passed, including single/multiple JSON array serialization and sorting;
actionlint passed. CI runs these checks before benchmarks and uploads raw
measurement JSON and logs even when a benchmark fails. This prevents cached
measurements from being reported as a fresh run. The primary Performance
Benchmarks job passed at `518fe70` (run 36781437624); its separate baseline
comparison was cancelled by the newer MCP commit; the new-head comparison remains unverified. These Criterion means and standard errors are component
microbenchmarks, not VM lifecycle P50/P95/P99 or matched competitor measurements.

## Guest boot debug-scan experiment (2026-09-30)

The fixed-UART guest still performs Linux's `CONFIG_DEBUG_WX` boot-time
page-table check. We tested disabling only that defconfig option, retaining
strict kernel/module W^X protections, MP-table/I/O APIC support, serial
console, virtio networking/vsock and 9P. The experimental Linux 6.6.52
kernel built from the checksum-pinned source, then passed real-KVM
checkpoint rollback, pause/resume and two state-preserving forks.

Twenty counterbalanced blocks alternated kernel order; each kernel run
alternated two engine pairs on a fresh isolated node. Both engines received
identical kernel/initrd bytes, 1 vCPU/1024 MiB and a 15-second guest readiness
budget. One pinned CPU worker remained alive throughout. All 160 attempts
passed, artifact identities remained unchanged and owned processes were
cleaned up. This was another shared nested-KVM run, with substantial host
noise; its timings cannot be substituted into earlier cohorts.

| Kernel debug scan | Engine | Passing attempts | Readiness P50 | Readiness P99 |
|---|---|---:|---:|---:|
| Enabled, current default | HyperMachine | 40/40 | 1527.00 ms | 4648.16 ms |
| Disabled, experiment | HyperMachine | 40/40 | 1446.36 ms | 8929.95 ms |
| Enabled, current default | Firecracker 1.17.0 | 40/40 | 1343.26 ms | 6906.98 ms |
| Disabled, experiment | Firecracker 1.17.0 | 40/40 | 1412.88 ms | 12351.56 ms |

Disabling the scan was faster for HyperMachine in only 8/20 blocks and
increased its overall mean by 452.59 ms; the median paired block mean
increased by 61.07 ms. The descriptive paired bootstrap interval for the
enabled-minus-disabled mean was [-953.66, -16.87] ms (10000 resamples,
seed 0); shared-host drift limits causal interpretation. Forty samples per
group also make P99 the maximum observation, a weak estimate of a tail.
**No repeatable improvement was established; the default retains the scan.**
This experiment also does not establish a new engine ranking or regression.

`tools/guest-image/build-kernel.sh` now accepts `--config DEFINITION` and
`--config-output RESOLVED_CONFIG`, while preserving its default defconfig.
It resolves input paths before changing directories, rejects identical
kernel/config output paths, and saves the resolved configuration after a
successful build. The exported experimental config and defconfig are
`benchmarks/2026-09-30/debug-wx-off-resolved.config` and
`debug-wx-off.defconfig`; raw/summary reports, exact benchmark/analysis
sources and the state probe share the `debug-wx-` prefix. Reports record
the kernel, configuration, builder, daemon and harness hashes. Reproduction
requires supplying local artifact paths in the recorded coordinators.

## Shipped MCP sandbox interface

`hm sandbox vm --endpoint https://sandbox-api.example.com mcp` serves MCP
2025-11-25 over standard input/output. It follows the
[stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
[initialization lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tool result](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
interfaces. Configure `HV2_API_KEY` in the server process environment through
your client's environment settings or secret store. `HV2_SANDBOX_URL` also
sets the endpoint. API scope and expiry are enforced by the control plane.
The command opens no additional network listener and uses the same remote
API client as the VM CLI, with redirects disabled and a 120-second default
HTTP deadline. CLI tracing goes to stderr so stdout contains protocol only.

The tools are `sandbox_create`, `sandbox_list`, `sandbox_inspect`,
`sandbox_exec`, `sandbox_pause`, `sandbox_resume`, `sandbox_fork`,
`sandbox_delete`, `checkpoint_save`, `checkpoint_list`, `checkpoint_restore`
and `checkpoint_delete`. Execution accepts an argv array, quotes it for the
guest shell, and reports guest failure/timeout as `isError`. Partial forks
also report `isError` while retaining successful child IDs. Structured
`envdAccessToken` and `accessToken` fields are removed recursively; upstream
HTTP error bodies are not echoed. Guest command output is still returned as
requested and may contain data the command itself prints.

Add `--envd-endpoint https://sandbox-proxy.example.com` after `mcp` to expose
`file_upload` and `file_download`; for a local proxy that routes by Host, also
set `--envd-domain sandbox.local`. This endpoint is operator configuration,
not a tool argument. Each file call connects through the control API, then
uses only the sandbox token at envd; the platform key stays on the control
API client. Tool results do not expose either credential. Uploads replace
the specified guest file. Both tools use base64 and accept at most 256 KiB
of decoded bytes, with a streaming download limit even without Content-Length.
Guest paths are encoded as literal query values, and no host file is read
or written. File results include matching text and structured JSON. The
smaller file limit keeps both representations within the 1 MiB frame budget.

Discovery and tool calls require initialization; notifications do not execute
tool calls or receive responses. Invalid IDs/arguments are protocol errors,
and input frames larger than 1 MiB terminate the session. Tool operations
execute sequentially, while the transport continues reading bounded input.
Streaming output, Streamable HTTP,
resources/prompts/tasks and the wider `hv2-agent` tool surface
remain unsupported. Created VMs persist until deleted or their lifetime
expires; closing the client does not delete them. Interrupted creations can
leave IDs unknown to the client, as with the other API clients.

Six protocol/failure-accounting tests, all six existing shipped CLI HTTP tests,
and strict all-targets CLI Clippy passed.
`tools/e2e-mcp-sandbox.py` used the unmodified official MCP Python client 1.23.3
to launch the shipped debug binary and complete 21 tool calls through all 12
tools on a real nested-KVM release node. It verified exact guest output,
1 vCPU/1024 MiB resources, checkpoint rollback, pause/resume file state,
distinct fork identity and private-file independence, guest exit code 7,
HTTP failure reporting and deletion of both parent and child. The node's
sandbox list was then confirmed empty. Raw functional evidence, with client,
script and binary identities, is `benchmarks/2026-09-30/mcp-stdio-lifecycle.json`.
This is compatibility evidence, not a latency benchmark or a competitor win.

The file-tool extension passed all 15 sandbox CLI/MCP tests and strict
all-targets CLI Clippy. The official client 1.23.3 then discovered all 14
tools and completed 25 calls on an isolated 1 vCPU/1024 MiB real KVM node,
including a 256 KiB roundtrip containing every byte value at a guest path
with a quote and ampersand, plus rejection of a 262145-byte download.
The existing checkpoint, pause/resume, fork isolation and failure checks
also passed. The client deleted both guests, the node list was empty, all
artifacts retained their recorded hashes, and the owned node was stopped.
Raw evidence is `benchmarks/2026-09-30/mcp-stdio-files.json`; its exact
coordinator is `mcp-stdio-files-coordinator.py`. This functional check does
not measure performance or establish full agent-tool parity.

The transport handles MCP `notifications/cancelled` during an active API
wait, matching the original request's string or integer ID. It drops the
client's HTTP future and sends no response for that cancelled request.
Queued requests can also be cancelled before reaching the API. Unknown,
completed and malformed cancellation notifications are ignored, and
`initialize` is not cancellable. Up to 8 queued messages and at most 1 MiB
of aggregate queued input are accepted; exceeding either bound terminates
the session. Partial frames survive a response/cancellation race and retain
the cumulative 1 MiB frame limit. Input EOF drops the active client wait.
This follows the [MCP cancellation protocol](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation).

**Cancellation does not undo accepted remote work or kill a guest process.**
The underlying lifecycle, file and exec APIs may continue their operations;
an interrupted creation can still leave an ID unknown to the client.
Stopping guest processes requires the existing process signal interface,
and deleting sandboxes requires an explicit lifecycle call.

All 22 sandbox CLI/MCP tests and strict all-targets CLI Clippy pass on
Windows and Linux. The official client
1.23.3 cancellation exercise confirmed that a real
guest command had started before sending a cancellation notification. The
same session then answered a ping within the two-second functional budget
(1.73 ms observed), sent no cancelled-request response, and remained usable.
The guest command finished afterward, explicitly verifying the limitation
above. The 25 existing tool calls, one cancelled exec request and one ping
also retained the binary-file, checkpoint, pause/resume, fork and failure
checks. Both guests were deleted, the node list was empty, the owned node
stopped and recorded artifact hashes were unchanged. Raw evidence and its
exact coordinator are `benchmarks/2026-09-30/mcp-stdio-cancellation.json`
and `mcp-stdio-cancellation-coordinator.py`; the fixture uses public official
client APIs and observes the outgoing protocol ID through a transparent
write-stream wrapper. This is a functional responsiveness check, not a
latency benchmark or full remote-operation cancellation.


## Concurrent native cold-start comparison (2026-09-30)

`tools/bench-local-engines-concurrent.py` compares barrier-released batches
on the same shared WSL nested-KVM host. The driver and both engines inherit
CPU affinity 0–7; one controlled worker runs on CPU 0. Each guest has 1 vCPU
and 1024 MiB, boots the same fixed-UART Linux kernel and guest-agent initrd,
and must return an exact unique shell marker. Resource declarations are
checked afterward. Engine batch order alternates AB/BA, and every guest
remains held until all attempts finish validation and aggregate process
PSS is read. Cleanup is then released; known guests/processes are deleted
or stopped, the node list is checked empty, and the owned daemon and worker
are reaped. Both guest-readiness budgets remain 15 seconds.

| Concurrent guests | Engine | Passing attempts | Readiness P50 | P95 | P99 | Median burst ready rate |
|---:|---|---:|---:|---:|---:|---:|
| 1 | HyperMachine | 100/100 | 398.10 ms | 440.86 ms | 457.89 ms | 2.48/s |
| 1 | Firecracker 1.17.0 | 100/100 | 371.15 ms | 400.68 ms | 415.58 ms | 2.67/s |
| 8 | HyperMachine | 200/200 | 514.61 ms | 663.18 ms | 690.03 ms | 12.22/s |
| 8 | Firecracker 1.17.0 | 200/200 | 427.53 ms | 573.48 ms | 606.44 ms | 13.96/s |
| 50 | HyperMachine | 500/500 | 3219.51 ms | 4591.53 ms | 4892.71 ms | 14.86/s |
| 50 | Firecracker 1.17.0 | 500/500 | 2560.56 ms | 2746.57 ms | 2778.06 ms | 18.23/s |
| 100 | HyperMachine | 1000/1000 | 6466.88 ms | 10046.07 ms | 10709.99 ms | 14.67/s |
| 100 | Firecracker 1.17.0 | 1000/1000 | 5450.96 ms | 7019.59 ms | 7175.85 ms | 17.55/s |

All 3600 attempts passed. The 100/25/10/10 paired batches per concurrency
are the independent workload repetitions; guests within a batch share load
and are not independent repetitions. Burst ready rate divides validated,
cleanup-checked passing attempts by the interval from barrier release to
the last guest/resource validation, excluding cleanup. It is not sustained
arrival throughput. Per-attempt latency starts at its native creation path,
while measured launch spread captures scheduling and driver preparation.
Median launch spreads for HyperMachine/Firecracker were 0/0 ms at concurrency
1, 1.26/5.75 ms at 8, 39.59/51.27 ms at 50 and 102.83/107.81 ms at 100.
The host is shared, and HyperMachine uses a persistent HTTP node while
Firecracker starts a process and configures its Unix API for each guest.
These eight-CPU profiles cannot be substituted into earlier one-CPU cohorts.

| Held guests | HyperMachine node PSS, median | Sum of held Firecracker process PSS, median |
|---:|---:|---:|
| 1 | 183.03 MiB | 85.90 MiB |
| 8 | 918.55 MiB | 670.98 MiB |
| 50 | 4315.47 MiB | 4180.95 MiB |
| 100 | 8568.50 MiB | 8360.36 MiB |

These are aggregate process footprints immediately after each batch's last
validation. HyperMachine includes its daemon and allocations retained from
prior batches; Firecracker sums fresh processes. The initial empty-node PSS
was approximately 7 MiB, but no per-batch empty-node baseline or fixed idle
period was measured. Kernel allocations are excluded, earlier-ready guests
have longer idle time, and the readings are neither per-VM incremental
memory nor evidence of a stateful density win.

The sweep establishes measured native-engine concurrency coverage and an
observed performance gap: HyperMachine remains slower, with a larger tail
gap at 50 and 100 guests. It does not isolate a cause or establish managed
platform, SDK, snapshot, bare-metal or fleet superiority. Raw cohorts and
the matrix summary use the `benchmarks/2026-09-30/local-engines-bursts-`
prefix; exact coordinator and analysis sources are retained alongside them.
The analysis checks ordering, attempts, deadlines, resource/CPU profiles,
artifact identities, timing consistency, worker liveness and cleanup.
Ten synchronization/workload/failure tests pass on Windows and Linux and
are included in the existing benchmark CI gate. Earlier harnesses remain
unchanged so their recorded source hashes are preserved. Reproduction
requires updating local artifact/output paths in the recorded coordinator.

## Concurrent startup diagnostics (2026-10-01)

Two diagnostic cohorts each ran two alternating engine batch pairs at
concurrency 100, with the same fixed-UART guest, eight-CPU affinity and one
controlled CPU-0 worker. All 800 attempts passed; each cohort matched all
200 HyperMachine request IDs to daemon stage logs. Artifact identities,
worker liveness, sandbox deletion and process cleanup passed. These traced
cohorts are investigative evidence, excluded from the scored comparison.

| Mean daemon stage | Stage-only cohort | Guest-probe cohort |
| --- | ---: | ---: |
| Build | 6.63 ms | 6.06 ms |
| Launch | 108.36 ms | 99.01 ms |
| Agent readiness | 7254.91 ms | 5471.78 ms |
| Network and envd | 0.11 ms | 0.08 ms |
| Total bring-up | 7370.00 ms | 5576.93 ms |

The second cohort collected guest logs from every tenth request after all
100 guests in each batch validated readiness. Those extra commands affect
memory readings and cleanup duration, but occur after recorded startup.
The 20 sampled guests handed off to `/init` at a mean guest timestamp of
1531.49 ms; their paired host agent-wait duration was about 3985.26 ms longer.
Host durations and guest timestamps cannot isolate dispatch, guest userspace
or handshake delay. All 20 sampled logs show CRNG initialization before the
handoff; this does not establish entropy behavior for unsampled guests.

Agent readiness dominates both cohorts. Image build/launch timing does not
support prioritizing image-loading optimization on this workload. Existing
code already offloads cold guest connection/ping to a blocking task and
runs each vCPU on a dedicated thread; finer queue, dispatch and handshake
tracing is needed before selecting a runtime change. Only two batch pairs,
shared nested hardware and asymmetric tracing limit causal conclusions.

Exact reports, coordinators, the original stage-only wrapper and analysis
are retained in `benchmarks/2026-10-01`. The optional wrapper
`tools/diagnose-concurrent-startup.py` leaves scored harnesses unchanged;
three duration/identity parser tests pass on Windows and Linux and run in CI.
No production performance change or competitor win follows from this step.

Cold readiness now also exposes opt-in debug events under the dedicated
`hv2_agent::cold_readiness` target. Enable it with
`RUST_LOG=hv2_sandboxd=debug,hv2_agent::cold_readiness=debug` to separate
blocking-task queue time, vsock connection time and ping time by VM name.
Connection failures record queue/connect timing and `phase=connect`;
completed ping attempts include all three durations and their success state.
This instrumentation preserves the existing blocking-task execution and
timeout arguments. Formatting and Linux all-targets agent compilation pass;
a finer-stage cohort is recorded below.

The dedicated traced daemon (`c3e96cab…`) then completed two alternating
C100 batch pairs: all 400 attempts passed and all 200 HyperMachine IDs
matched both daemon and cold-readiness stages. Guest probes were disabled;
the scored daemon was restored to its original `8ea52a36…` hash.

| Cold readiness component | Mean | Median | Maximum |
| --- | ---: | ---: | ---: |
| Blocking-task queue | 8.73 ms | 5.19 ms | 116.07 ms |
| Vsock connection | 6239.36 ms | 6225.59 ms | 8902.98 ms |
| Ping | 15.62 ms | 6.14 ms | 168.63 ms |

Connection timing includes waiting for guest boot, vsock driver and listener
readiness; it is not isolated transport overhead. This cohort directs the
next investigation toward that path rather than blocking-pool queueing or
ping handling. Two correlated batches per engine, shared nested hardware
and tracing do not establish causal attribution or a performance win.
Cleanup, worker liveness and artifact checks passed. The raw log, matched
stages, exact coordinator and verifying analysis are retained under the
`cold-readiness-` prefix in `benchmarks/2026-10-01`; five parser tests pass
on Windows and Linux. Historical wrapper versions remain archived so their
recorded hashes survive the optional finer-stage collection mode.

The guest init script now offers two opt-in kernel-timestamped milestones
with `HV2_BOOT_TRACE=1` on its kernel command line: `HV2_BOOT
core_mounts_ready` after proc/sys/dev mounts, and `HV2_BOOT agent_launch`
after the remaining mounts, network and environment setup, just before
starting the agent. These use `/dev/kmsg` and appear in guest `dmesg`.
Together with the existing kernel init-handoff timestamp, they can bound
pre-launch userspace work. They do not mark socket bind or acceptance.
Normal boots emit neither milestone. Linux shell syntax validation passes;
runtime collection follows below; its timing impact has not been isolated.

The diagnostic image (`781c3944…`) enables these milestones identically for
both engines. Archive verification confirms equal entry sets and modes,
with only `init` content changed; BusyBox and the guest agent are byte-identical
to the prior image. Another two alternating C100 batch pairs passed all 400
attempts, matched all 200 HyperMachine stage IDs and recorded both milestones
in all 20 sampled guests. Artifact, worker and cleanup checks passed.

| Guest-clock interval, 20 samples | Mean | Median | Maximum |
| --- | ---: | ---: | ---: |
| Init handoff to core mounts | 18.45 ms | 11.81 ms | 63.92 ms |
| Core mounts to agent launch | 11.79 ms | 1.20 ms | 61.69 ms |
| Init handoff to agent launch | 30.24 ms | 26.45 ms | 103.58 ms |

Mean guest agent-launch timestamp was 1810.64 ms. Host queue/connect/ping
means were 8.11/5585.95/9.28 ms for all 200 HyperMachine requests. The small
init interval does not support prioritizing shell setup optimization for
these samples. Guest scheduling and socket/packet readiness remain candidate
paths; clock origins differ and no socket bind/accept milestone was collected.
Probes occur after batch readiness and affect memory/cleanup readings.
These traced, sampled, shared-host cohorts are excluded from scored comparisons
and establish neither a runtime improvement nor a competitor win.
The `boot-milestones-` reports, build/coordinator/analysis sources and separate
archive verifier are retained in `benchmarks/2026-10-01` with exact hashes.

A subsequent diagnostic guest-agent build adds best-effort, opt-in
`agent_listening` and `agent_first_accept` kernel markers. Two C100 batch
pairs passed all 400 attempts with 200 matched HyperMachine stage IDs and
20 complete guest observations. Mean guest launch-to-listen was 16.72 ms
(median 1.99 ms, maximum 67.85 ms); listen-to-first-accept was 23.93 ms
(median 20.11 ms, maximum 135.55 ms). Host queue/connect/ping means were
9.06/6052.85/12.71 ms. These samples do not show seconds of delay after
listening. Aligning host VM dispatch and guest boot clocks remains necessary
before attributing the larger host connection wait.

The first marker implementation wrote formatted fragments as separate kmsg
records. This cohort's analysis uses the individually timestamped label
records; its exact executed agent source is archived. Current source submits
each marker as one buffer. All 16 Linux guest-agent tests and formatting pass
after that correction; corrected-marker runtime collection remains pending.
Archive verification confirms only init and agent content changed, equal modes
and entry sets, and identical BusyBox. Cleanup and artifact checks passed.
Sources and reports use `listener-milestones-`, `listener-image-` and
`guest-listener-trace-` names in the same evidence directory. This remains
sampled diagnostic evidence, excluded from performance rankings.

Paired host/guest clock observations then completed another 400/400 C100
attempts with all 200 HyperMachine stage IDs matched. Each of 20 sampled
guests supplied two uptime reads, each bounded by host monotonic request
start/end timestamps. All 20 guest elapsed intervals fit the host bounds
after allowing two centisecond quantization bins. This checks only those
post-readiness intervals, not earlier boot-time clock stability.

Conditionally assuming stable elapsed clocks back through boot, the mean
inferred guest-clock epoch bounds were 4422.50–4449.58 ms after native
creation began. The guest clock epoch is not VM entry: this does not prove
dispatch delay or isolate transport behavior. It directs the next measurement
toward host VM/vCPU dispatch and early guest clock initialization. Reports,
exact executed wrapper, coordinator and analysis use `clock-alignment-`
names in the evidence directory. Cleanup and artifact checks passed; memory
readings follow the extra probes. The scored harnesses remain unchanged.

Host dispatch tracing is available under the dedicated
`hv2_core::cold_dispatch=debug` filter. `VM background dispatch` records
time queued before the background run task starts. `vCPU owner thread entry`
records wrapper-task queueing and time from wrapper entry to OS-thread
entry, with VM name and vCPU ID. It precedes affinity/runtime setup and
the first backend run; it is not a guest-entry timestamp. Linux all-targets
core compilation and formatting pass. Runtime collection is pending and
these events establish no scheduling improvement.

The traced daemon (`08e8aa52…`) then completed two alternating C100 batch
pairs, passing all 400 attempts and matching all 200 HyperMachine IDs to
complete dispatch/readiness records. Mean/maximum VM dispatch queue times
were 5.00/31.49 ms; vCPU wrapper queue times 0.0043/0.0289 ms; wrapper-to-owner
thread entry times 7.91/125.45 ms. All 20 paired guest uptime intervals fit
their host bounds. These dispatch intervals do not account for the seconds
between native creation and the conditional guest clock epoch. Affinity and
runtime setup, first backend run, and early guest work before clock initialization
remain unmeasured boundaries; no scheduling root cause is established.
Artifact, worker and cleanup checks passed. Exact sources, raw log and analysis
use `dispatch-diagnostic-` names in the same directory. Seven parser tests pass
on Windows and Linux. Tracing and post-readiness probes exclude this cohort
from scored comparisons; the original scored binary hash is restored.

First-backend probes extend the dedicated dispatch target with
`owner_setup_ms` (owner-thread entry through affinity/runtime setup and
control checks to the first backend invocation) and `first_backend_ms`
(that invocation through its first successful return). Each emits once per
vCPU execution loop, not once per VM exit. The first invocation is not a
guaranteed KVM guest-entry timestamp, and an error does not emit the successful
return marker. The diagnostic wrapper requires all five dispatch fields for
each passed guest. Linux all-targets compilation, formatting and parser tests
pass; runtime first-backend measurements remain pending. The prior dispatch
wrapper is archived as `dispatch-harness-v1.py` for its recorded source hash.

The first-backend traced daemon (`6718bad3…`) completed another two C100
batch pairs: all 400 attempts passed and all 200 HyperMachine IDs matched
complete five-field dispatch records. Owner setup mean/median/maximum was
1.37/0.087/46.40 ms. First backend call mean/median/maximum was
3480.50/3328.36/4924.20 ms. Thus a substantial measured interval is inside
the backend run operation, rather than affinity/runtime setup. That interval
includes host scheduling and guest execution through the first return; the
exit kind and guest instruction address were not collected. It does not
isolate kernel decompression, CPU work or a hypervisor defect.

All 20 paired uptime intervals fit host bounds, and artifact/worker/cleanup
checks passed. Sources and reports use `first-backend-diagnostic-` names in
the evidence directory. The shared host showed large variation in the two
Firecracker batches; this asymmetric tracing cohort remains excluded from
performance rankings. The original scored binary hash is restored. First-exit
identity and early kernel execution are the next measurement boundaries.

An experimental Linux 6.6.52 LZ4-compressed kernel build completed to test
the compressed early-boot path. The retained `lz4-guest.defconfig` differs
from the default only by disabling kernel gzip compression and selecting
LZ4. The owned build's resolved configuration enables LZ4, strict kernel and
module RWX protection, and DEBUG_WX. It uses the existing source-checksummed
builder with eight-CPU affinity and separate output paths; defaults remain
unchanged. Reproduce with LZ4 installed:

```sh
taskset -c 0-7 bash tools/guest-image/build-kernel.sh \
  --config docs/benchmarks/2026-10-01/lz4-guest.defconfig \
  --config-output /var/tmp/hm-kernel-lz4/resolved.config \
  -o /var/tmp/hm-kernel-lz4/bzImage
```

The kernel hash is `d0b6b580…`; its 17,327,104 bytes exceed the gzip kernel's
12,952,576 bytes. The exact builder, resolved config and build identities are
retained alongside the experimental defconfig. A concurrency-1 smoke cohort
passed all four native attempts (two per engine), including readiness, resource
validation, artifact stability and cleanup. This is functional evidence only.
Six counterbalanced C100 blocks completed, with two alternating engine pairs
per kernel per block. Native timing includes larger image-loading costs.

| Kernel / engine | Passed / attempts | Successful P50 | Successful P99 |
| --- | ---: | ---: | ---: |
| Gzip / HyperMachine | 1190 / 1200 | 6801.47 ms | 15626.92 ms |
| LZ4 / HyperMachine | 1200 / 1200 | 6523.37 ms | 11610.63 ms |
| Gzip / Firecracker | 1200 / 1200 | 5594.62 ms | 11149.61 ms |
| LZ4 / Firecracker | 1200 / 1200 | 4746.36 ms | 5063.31 ms |

All ten failures occurred in HyperMachine's first gzip batch of the final
block. They timed out waiting for guest readiness; retained diagnostics show
halted vCPU samples and continued I/O exits. Artifact stability, controlled
worker liveness, node-empty checks and daemon cleanup passed for every cohort.
Failed create rows have no returned sandbox ID and no client delete success;
the isolated node cleanup checks still passed. The cohort remains failed and
its raw errors are retained. Successful latency quantiles exclude these ten
timeouts and cannot represent unconditional completion latency.

For the five HyperMachine blocks with complete success on both kernels, LZ4
reduced the block mean in only 2/5, with a median delta of **+31.05 ms**
(LZ4 minus gzip). Firecracker improved in all six complete blocks, with a
median delta of -950.88 ms. Shared-host variation, within-batch correlation
and excluding the failed HyperMachine block from paired latency analysis
limit causal conclusions. Zero LZ4 failures here does not establish a
reliability fix. HyperMachine still trails Firecracker on the LZ4 profile.

LZ4 checkpoint restore, pause/resume and two stateful forks pass against a
snapshot-backed template, with guest state, command readiness, I/O APIC
discovery, artifact stability and cleanup verified. Exact cohort sources,
manifest, raw reports and analysis use the `kernel-compression-` prefix;
state evidence is in `lz4-state.json` and `verify-lz4-state.py`.
This experiment does not establish a repeatable HyperMachine performance
improvement or decompression as the root cause. The default kernel is unchanged.

All ten gzip timeouts share owner RIP `0xffffffff81eda95f`, halted run state
and LAPIC timer value `0x400ec`. A separate matched-kernel Firecracker guest
resolved `/proc/kallsyms`: `default_idle` starts at `0xffffffff81eda950`,
15 bytes before the sampled RIP. That diagnostic guest passed readiness and
cleanup; kernel and helper hashes were unchanged. The exact query and source
are retained as `gzip-timeout-halt-symbol.json` and its resolver script.
This confirms a shared idle-path address, not a root cause.

Failure reports now include raw `TSC` and `TSC_DEADLINE` values already captured
by the owner-safe vCPU snapshot. Missing values print `unavailable`, distinct
from a captured zero. No extra register reads, snapshot schema changes or
runtime timer behavior are introduced. MSR reads are sequential and these
values do not form an atomic timer comparison. All six Linux boot-diagnostic
tests and formatting pass, including missing/zero/full-width clock values.
Runtime capture of these fields during another failed boot remains pending.

A separate failure-diagnostic daemon (`0341fe7e…`) then ran ten alternating
C100 engine pairs with the original gzip guest, eight-CPU affinity and one
CPU-0 worker. All 2000 attempts passed (1000 per engine), with stable artifacts,
worker liveness, empty-node checks and daemon cleanup. Warning-level logging
was retained. This cohort had no failures and therefore collected no failed-boot
timer samples. It does not resolve the ten prior timeouts or establish a
reliability fix; only failure-message formatting changed, not timer behavior.
The exact coordinator, raw report and analysis use `timer-diagnostic-` names
in the same evidence directory. The original scored binary hash is restored.

Failure reports now also decode the already captured KVM I/O APIC irqchip
payload. They include controller base/select/id/IRR and all 24 GSI routes,
with full-width raw values, vector, mask, level-trigger, remote-IRR and
destination fields. Decoding requires the complete 512-byte union and ignores
its trailing padding; missing or malformed captures print `unavailable`.
These machine reads remain sequential, taken before the owner kick, and are
not an atomic interrupt snapshot. No additional ioctls, snapshot schema or
interrupt behavior change. All eight Linux boot-diagnostic tests and formatting
pass, including full-width routes, boundary pins, poisoned padding and malformed
lengths. The existing workspace CI test job includes these daemon tests.
Unforced failed-boot runtime I/O APIC route capture remains pending; no wakeup
fix follows from this formatter change.

A negative-control image omits the agent launch line while retaining all other
archive content and modes, including identical agent/BusyBox binaries. Against
the diagnostic daemon (`5e6916bf…`), its create request returned the expected
503 readiness failure after 15.56 seconds. The API error contained captured
TSC/deadline values and all 24 I/O APIC routes in order. The node was empty,
daemon cleanup passed and artifact hashes were unchanged. This verifies live
failure formatting; the deliberate missing agent does not reproduce the ten
earlier timeouts. Image build/verification and API evidence use the `no-agent-`
names in the evidence directory.

The same daemon then passed all 400 normal-guest attempts in two alternating
C100 pairs with the original gzip image and controlled host profile. Artifact,
worker and node/daemon cleanup checks passed, but no intermittent failure or
its route state was observed. Reports and sources use `ioapic-diagnostic-`
names. Both controls are diagnostic evidence, excluded from scored comparisons;
the original scored binary hash is restored. No reliability fix is claimed.

## Same-host Firecracker cold comparison (2026-09-30)

The user has no competitor endpoints or dedicated matched host. We therefore
ran Firecracker v1.17.0 locally against HyperMachine on the same shared WSL
nested-KVM host. Cohorts contain 20, 100 or 200 alternating AB/BA pairs, concurrency
one, 1 vCPU and 1024 MiB. Both engines boot the exact same Linux kernel and
BusyBox guest-agent initrd and must return an exact unique shell marker.
HyperMachine uses a prestarted HTTP daemon with `--no-template`; Firecracker
starts a process and configures its UNIX API for each sample. Timings end at
the verified first command response. These native control paths differ, and
the host has other workloads. Most cohorts are unpinned; the explicitly labeled
pinned contention cohort places both engines and one CPU worker on CPU 0. This is a cold sandbox
comparison, not SDK, managed-platform, snapshot, density or bare-init timing.

| Cohort | Engine | Valid / attempted | Readiness P50 (ms) | Readiness P99 (ms) |
|---|---|---:|---:|---:|
| Initial | HyperMachine | 19 / 20 | 1913.34 | 4222.52 |
| Initial | Firecracker 1.17.0 | 20 / 20 | 1137.38 | 2468.34 |
| Repeat with memory diagnostics | HyperMachine | 18 / 20 | 1230.74 | 1819.96 |
| Repeat with memory diagnostics | Firecracker 1.17.0 | 20 / 20 | 816.65 | 2408.49 |
| Normal-logging reliability repeat | HyperMachine | 98 / 100 | 1174.44 | 2212.68 |
| Normal-logging reliability repeat | Firecracker 1.17.0 | 100 / 100 | 802.88 | 2485.07 |
| Daemon owner-diagnostic build | HyperMachine | 200 / 200 | 692.47 | 763.00 |
| Daemon owner-diagnostic build | Firecracker 1.17.0 | 200 / 200 | 403.88 | 454.65 |
| Two unpinned CPU workers | HyperMachine | 100 / 100 | 717.83 | 823.35 |
| Two unpinned CPU workers | Firecracker 1.17.0 | 100 / 100 | 414.28 | 470.27 |
| One contending worker, all pinned to CPU 0 | HyperMachine | 91 / 100 | 1159.99 | 1324.16 |
| One contending worker, all pinned to CPU 0 | Firecracker 1.17.0 | 100 / 100 | 820.63 | 874.96 |
| Pinned APIC diagnostic repeat | HyperMachine | 92 / 100 | 2771.78 | 5910.02 |
| Pinned APIC diagnostic repeat | Firecracker 1.17.0 | 94 / 100 | 2266.94 | 4000.67 |
| PIT stub and API-readiness correction, pinned repeat | HyperMachine | 93 / 100 | 1748.13 | 3366.14 |
| PIT stub and API-readiness correction, pinned repeat | Firecracker 1.17.0 | 100 / 100 | 1245.19 | 3125.96 |
| Pre-kick PIC/PIT diagnostics, matched readiness deadlines | HyperMachine | 97 / 100 | 1170.51 | 3865.63 |
| Pre-kick PIC/PIT diagnostics, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 815.35 | 3719.40 |
| APIC base diagnostic, matched readiness deadlines | HyperMachine | 97 / 100 | 1313.56 | 3346.01 |
| APIC base diagnostic, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 913.93 | 2321.68 |
| Singleton topology correction, matched readiness deadlines | HyperMachine | 93 / 100 | 1796.93 | 11752.53 |
| Singleton topology correction, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 1373.81 | 4418.93 |
| Owner interrupt events, matched deadlines and thread probes | HyperMachine | 82 / 100 | 1897.66 | 6517.16 |
| Owner interrupt events, matched deadlines and thread probes | Firecracker 1.17.0 | 100 / 100 | 1488.91 | 5003.76 |
| KVM retry counters, matched readiness deadlines | HyperMachine | 91 / 100 | 1896.21 | 8296.29 |
| KVM retry counters, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 1631.53 | 6573.30 |
| Singleton MP table, matched readiness deadlines | HyperMachine | 100 / 100 | 1107.25 | 2263.95 |
| Singleton MP table, matched readiness deadlines | Firecracker 1.17.0 | 100 / 100 | 804.85 | 1740.15 |
| Singleton MP table, 200-pair pinned repeat | HyperMachine | 200 / 200 | 1079.50 | 1867.38 |
| Singleton MP table, 200-pair pinned repeat | Firecracker 1.17.0 | 200 / 200 | 769.48 | 1573.45 |
| Counterbalanced kernel blocks, original kernel | HyperMachine | 100 / 100 | 1119.17 | 2645.60 |
| Counterbalanced kernel blocks, original kernel | Firecracker 1.17.0 | 100 / 100 | 787.76 | 1956.21 |
| Counterbalanced kernel blocks, fixed UART IRQ | HyperMachine | 100 / 100 | 859.85 | 1922.86 |
| Counterbalanced kernel blocks, fixed UART IRQ | Firecracker 1.17.0 | 100 / 100 | 773.83 | 1375.09 |

Percentiles use nearest rank over successful, cleaned-up samples only; failures
are retained in the reports and invalidate the first three normal-logging comparisons
and all pinned contention comparisons before the singleton MP-table change.
The 200-pair, unpinned two-worker and singleton MP-table cohorts passed. The lower
HyperMachine repeat P99 is not evidence of a performance win. HyperMachine
failed 5/140 attempts in the first three normal-logging cohorts while Firecracker passed 140/140. HyperMachine's median was slower in every recorded normal-logging cohort. We have not achieved across-the-board superiority.

Both memory-diagnostic repeat failures were 15-second guest-agent readiness timeouts with no
console output and 2401 total vCPU exits, with no further exits during the
0.5-second diagnostic window. The startup stall's root cause is not yet known.
Node PSS fell after deletion and plateaued around 72 MiB in the latter half of
the repeat, arguing against a growing per-VM memory leak in this cohort; it
is not a density benchmark. Memory collection occurs outside readiness timing.

Raw evidence: [initial cohort](benchmarks/2026-09-30/local-engines-cold-c1.json),
[diagnostic repeat](benchmarks/2026-09-30/local-engines-cold-memory-c1.json),
and [Firecracker bring-up](benchmarks/2026-09-30/firecracker-cold-bringup.json).
The reports retain failures, cleanup outcomes and before/after source hashes.
`tools/install-firecracker-benchmark.py` pins the official
[1.17.0 release](https://github.com/firecracker-microvm/firecracker/releases/tag/v1.17.0)
archive checksum before executing its binary. `tools/bench-local-engines.py`
reproduces the alternating comparison; its failure-accounting tests run in CI.


Cold-start investigation can opt in to
`--hypermachine-log-filter warn,hv2_core::backends::kvm::boot=trace`.
The KVM trace samples exit reason, RIP, flags and I/O port on the vCPU owner
thread after `KVM_RUN` returns. Each failed HyperMachine sample preserves its
immediate node log tail. Reports mark non-default logging as diagnostic
tracing; the extra register ioctls and log writes perturb timing, so those
reports must not be used as performance comparisons. Normal runs retain the
`warn` default. The tracing is investigative instrumentation, not a startup fix.


Five installer integrity tests cover checksum rejection before execution,
linked/duplicate binary rejection, preservation of an existing different
binary, extraction of only the selected binary, official version parsing with
its extra exit log, and refusal of a mismatched executable version. They run
in the benchmark CI gate without downloading or executing fixture binaries.


The first [boot-tracing diagnostic](benchmarks/2026-09-30/local-engines-boot-trace.json)
passed 20/20 HyperMachine and 20/20 Firecracker workload/resource/cleanup checks.
Before/after artifact hashes matched, and the retained log confirms actual KVM
exit tracing. It did not reproduce a failed startup, so it yields no failure
address or root cause. No startup behavior was changed, and tracing perturbs
timing; this cohort does not establish a reliability fix or performance gain.
The earlier 3/40 HyperMachine startup failures remain relevant evidence.


The [100-pair normal-logging repeat](benchmarks/2026-09-30/local-engines-cold-normal-100.json)
reproduced two HyperMachine readiness failures (pairs 25 and 60), each with no
console output and 2401 total exits. HyperMachine passed 98/100 checks;
Firecracker passed 100/100. Artifact hashes were unchanged, setup and final
node cleanup had no errors, and the overall report correctly failed. Successful
samples had HyperMachine/Firecracker P50 of 1174.44/802.88 ms and P99 of
2212.68/2485.07 ms. The smaller HyperMachine P99 excludes failed startups and
is not evidence of a win. No startup behavior was changed, and the earlier
failure is confirmed to persist with diagnostic tracing disabled.


The [100-pair tracing diagnostic](benchmarks/2026-09-30/local-engines-boot-trace-100.json)
passed 99/100 HyperMachine and 100/100 Firecracker checks. HyperMachine pair 58
failed with the same no-console, 2401-exit signature. The preserved trace ends
at I/O port 1017 (`0x3f9`, COM1 interrupt-enable register), RIP
`0xffffffff81648115`, flags `0x6`, followed by about 15 seconds without another
recorded exit. Disassembly of the exact guest kernel ELF confirms an `out`
instruction at that address. The extracted ELF SHA256 is
`e97635a1ec12e5611b68906eb900baeef2f77ee23a89015810e0efe65255e425`.
This locates the last observed exit during UART initialization, not necessarily
the subsequent stalled guest instruction or root cause. Further architectural
state inspection must run on the vCPU owner after it leaves `KVM_RUN`; reading
registers concurrently with execution would not be sound evidence. This is a
failed diagnostic cohort, not a performance comparison or reliability fix.


Readiness-failure reporting now requests architectural samples through
`VM::diagnostic_vcpu_states`: a bounded channel message plus vCPU kick asks the
execution owner to read its state between backend run calls. The response wait
is bounded to five seconds. Each CPU is sampled independently; these are
investigative samples, not an atomic restore snapshot. Replies include RIP, flags, CR3, run state and RSP, plus the raw LAPIC
timer, priority and ISR/IRR registers when a complete register image is available.
Absent or malformed LAPIC state is explicitly marked unavailable. Successful readiness
checks do not request these samples. A real-KVM regression obtained ten samples
each from an interrupt-disabled halted guest and an interrupt-disabled spinning
guest, checked their exact raw-code instruction positions, then stopped both.
Strict all-targets Clippy passed for the core, agent and daemon on Windows.

`cargo build --locked --profile test -p hv2-agent --example cold_owner_diagnostics`
builds a smaller probe of the same AgentVM cold-boot path. Run it as
`cold_owner_diagnostics KERNEL INITRD 100`. It uses the daemon's 1-vCPU/1024-MiB
configuration and shared kernel arguments, prints one JSON record per attempt,
samples owners after failed 15-second guest readiness, and cleans up every
launched VM before continuing. This is a diagnostic-only probe under the test
profile, not a release API or competitor performance benchmark.


The [test-profile owner probe](benchmarks/2026-09-30/cold-owner-diagnostics-test-profile.json)
completed 100/100 guest-readiness checks and cleanups with unchanged binary,
source, kernel and initrd hashes. It did not reproduce a failure and therefore
captured no stalled guest state. Its lower-level control path and optimization
profile differ from the failing release daemon; it does not establish that the
startup defect is fixed. The later pinned contention cohort verified live daemon failure-report integration
in nine failed readiness responses.


The [release-profile lower-level probe](benchmarks/2026-09-30/cold-owner-diagnostics-release.json)
completed 200/200 readiness checks and cleanups, with unchanged artifact hashes,
but captured no stalled state. Its control path still differs from the daemon:
it uses the machine model's automatic COM1 attachment, whereas the daemon
explicitly initializes and registers COM1 before launch, as well as a different
CID range and no HTTP request lifecycle. No root cause or fix follows from
this non-reproduction. The later pinned contention cohort reproduced the actual daemon failure and
sampled its architectural state beyond the previously captured UART exit.


The [rebuilt-daemon 200-pair cohort](benchmarks/2026-09-30/local-engines-daemon-owner-200.json)
passed 200/200 workload/resource/cleanup checks for each engine, with unchanged
artifacts and no setup or final-cleanup errors. It captured no readiness
failure and therefore no stalled architecture. HyperMachine P50/P99 was
692.47/763.00 ms versus Firecracker 403.88/454.65 ms, so it remained slower
on this measured cold control path. The implementation added only failed-boot
diagnostics, not a startup fix. Differences from older cohorts on this shared
host do not prove a reliability or latency improvement. A direct
`KVM_GET_SUPPORTED_CPUID` query returned 62 entries with MONITOR, MWAITX and
WAITPKG absent, so the proposed advertised-CPU-delay-feature explanation is
unsupported on this host; no CPU features were changed.


The [two-worker CPU-load cohort](benchmarks/2026-09-30/local-engines-owner-load-100.json)
passed 100/100 workload/resource/cleanup checks for each engine. Both controlled
CPU workers stayed alive through the full cohort and were terminated and reaped
afterward. The [coordinator fixture](benchmarks/2026-09-30/owner-load-coordinator.py)
retains the exact workload and this host's paths; its checksum matches the raw
report. Neither workers nor engines were pinned. HyperMachine P50/P99 was
717.83/823.35 ms versus Firecracker 414.28/470.27 ms. No readiness warning or
stalled-owner sample was produced. This supports only these measured attempts,
not a fixed startup defect or superiority under general host contention.


The [pinned contention cohort](benchmarks/2026-09-30/local-engines-owner-pinned-load-100.json)
completed only 91/100 HyperMachine attempts versus 100/100 Firecracker attempts.
Both engines, the driver and one contending CPU worker inherited CPU-0 affinity.
The worker stayed alive throughout and was terminated and reaped afterward;
artifact hashes were unchanged and node cleanup completed. Its
[exact coordinator](benchmarks/2026-09-30/owner-pinned-load-coordinator.py) SHA256
is `e89f9e673e16b4bdc78c8b3ac2357b23e201f194b293c318472c817bae0bb577`,
matching the raw report. Failed attempts invalidate this performance comparison;
the successful-sample percentiles in the table are not a reliability-adjusted win.

All nine failures had no console output, 2401 total exits and no further exits
during the diagnostic window. Owner samples consistently returned
`RIP=0xffffffff81eda95f`, `CR3=0x2a2e000` and flags with interrupts enabled.
Disassembly of the exact benchmark kernel shows `sti` at `0xffffffff81eda95d`,
`hlt` at `0xffffffff81eda95e`, then `cli` at the captured RIP. This is consistent
with a guest halted awaiting an interrupt. The diagnostic kick can wake the
vCPU before its owner samples state, so `run_state=Runnable` does not establish
that the guest was spinning during the timeout. Linux's
[native safe halt implementation](https://raw.githubusercontent.com/torvalds/linux/v6.6/arch/x86/include/asm/irqflags.h)
uses `sti; hlt`; that source is v6.6, while the measured kernel is 6.6.52.
A [symbol query from a successful boot of the same kernel](benchmarks/2026-09-30/boot-halt-symbol.json)
confirms `ffffffff81eda950 T default_idle` and `ffffffff81648100 t io_serial_out`.
The [query coordinator](benchmarks/2026-09-30/boot-halt-symbol-coordinator.py)
retains the exact command and hashes; its owned Firecracker VM was stopped,
and the response was complete with exit status zero and unchanged artifacts.
This is diagnostic evidence, not an additional performance sample. The missing
wakeup cause remains unconfirmed. No startup
fix or across-the-board feature/performance advantage has been established.


The [APIC diagnostic repeat](benchmarks/2026-09-30/local-engines-owner-apic-pinned-load-100.json)
retained the pinned CPU-0 workload with unchanged artifacts and successful final
cleanup. HyperMachine passed 92/100, with eight readiness failures at 2401 exits.
Seven samples stopped at `default_idle`'s post-HLT `cli`; one captured the next
instruction after `cli`, illustrating that diagnostic wakeup can advance state.
All eight had `LVT_TIMER=0x10000`, zero initial/current timer counts,
`LVT0=0x700`, `TPR=PPR=0x10` and empty ISR/IRR bitmaps. This points to the
legacy interrupt path at that boot stage, not an established root cause.
The [exact coordinator](benchmarks/2026-09-30/owner-apic-pinned-load-coordinator.py)
checksum matches the recorded workload; the worker remained alive and was reaped.

Firecracker passed 94/100 in this repeat. Its six connection-refused samples
contain only the startup banner in the bounded console output, consistent with
an initial API readiness race. These are failed harness attempts, not evidence
that Firecracker's guest has the same boot defect. The harness previously waited
only for socket-file existence; it now waits for a successful read-only
`GET /machine-config` before issuing configuration PUTs, retries only missing
socket/connection-refused errors within the original deadline and checks VMM
exit. It never replays a mutation. Ten harness regressions pass, including
readiness deadline/exit and non-retryable error cases. Both engines' failures
invalidate this cohort for a performance claim. Subsequent runs must record the
changed harness hash and extra readiness probe.


A real-KVM raw guest regression verified a separate timer-model gap: reading
port `0x61` returned unmapped-I/O `0xff` before the fix. The test stopped its VM
before asserting and failed on the reserved high bits. KVM PIT creation now
sets `KVM_PIT_SPEAKER_DUMMY` (value 1 in the installed Linux KVM UAPI), enabling
the channel-2 speaker-port stub used during timer calibration. The same real
guest regression then passed. Firecracker's
[versioned PIT setup](https://raw.githubusercontent.com/firecracker-microvm/firecracker/v1.17.0/src/vmm/src/arch/x86_64/vm.rs)
also enables this stub. This establishes corrected port emulation, not that
the cold-boot failures or latency gap are fixed. The matched repeat below
confirms that this port correction leaves the startup stall unresolved.


The [PIT-corrected daemon repeat](benchmarks/2026-09-30/local-engines-pit-speaker-pinned-load-100.json)
passed 93/100 HyperMachine attempts versus 100/100 Firecracker attempts. The
[coordinator](benchmarks/2026-09-30/pit-speaker-pinned-load-coordinator.py)
retained the CPU-0 contention profile; source/artifact checksums stayed
unchanged, the worker stayed alive and was reaped, and node cleanup succeeded.
All seven HyperMachine failures retained the 2401-exit, no-console,
`default_idle` stall and masked/unarmed LAPIC timer evidence. The PIT port
correction therefore does not resolve this startup defect. Differences between
seven, eight and nine failures across shared-host cohorts do not establish
a reliability improvement. Successful-sample P50/P99 was 1748.13/3366.14 ms
for HyperMachine versus 1245.19/3125.96 ms for Firecracker; the failed cohort
cannot support a performance win.

The corrected Firecracker readiness harness produced no connection-refused
samples in this repeat. Its additional read-only readiness probe changes the
control path and is included in measured time; raw reports retain the changed
harness checksum. This result supports these 100 attempts, not general
competitor reliability or a managed-platform comparison.


Failure reporting now prepares a machine-level PIC/PIT sample before kicking
the vCPU owner. These are independent KVM VM-level GET operations, not concurrent
vCPU register reads and not an atomic restore snapshot. It reports PIC request,
mask and in-service registers, and all three PIT channel counts/modes/gates and
load timestamps. Non-KVM, absent or malformed layouts are explicitly unavailable.
Four decoder tests pass, including missing-state handling, PIC bits and PIT's
valid 65536-count boundary. A real-KVM regression read the controller state while
a raw guest remained halted with interrupts disabled, then stopped it; both
that test and the existing halted/spinning owner regression passed. Strict
all-targets core/daemon Clippy passed on Windows. Live Linux-failure integration
captured controller and owner state in all three failures of the latest cohort.
Successful readiness does not request these samples.

The [pre-kick PIC/PIT cohort](benchmarks/2026-09-30/local-engines-pre-kick-pic-pit-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/pre-kick-pic-pit-pinned-load-coordinator.py)
record 97/100 HyperMachine passes versus 100/100 Firecracker passes. Both engines
have a 15-second guest-readiness budget; driver, engines and one contending worker
inherit CPU 0 affinity. Artifact hashes remained unchanged and all owned processes
were cleaned up. HyperMachine failed pairs 1, 5 and 97 with no console output and
2401 exits, unchanged over the 0.5-second observation window. Before the diagnostic
kick, each master PIC reported IRR=0x11, IMR=0 and ISR=0: timer and UART requests
were pending and unmasked. PIT channel 0 reported count 1193, mode 2 and gate 1.
After the kick, owner samples showed the same default_idle address with IF set,
LVT0=0x700 and empty LAPIC ISR/IRR arrays. Independent controller reads and the
later owner sample do not establish an atomic state or root cause. Interrupt
delivery remains under investigation. This failed cohort cannot support a
performance win; the lower failure count versus earlier shared-host runs does
not establish a reliability improvement.

The next diagnostic build also prints the existing owner-captured APIC base
MSR and CR8 alongside the LAPIC image. This exposes APIC enable/BSP bits and
CPU interrupt priority without additional register reads. The formatting check
preserves both values. The [APIC base cohort](benchmarks/2026-09-30/local-engines-apic-base-pinned-load-100.json)
and [coordinator](benchmarks/2026-09-30/apic-base-pinned-load-coordinator.py)
verify live capture in all three failed boots (pairs 20, 29 and 56), each with
APIC_BASE=0xfee00900 and CR8=1. The bootstrap APIC is enabled in these post-kick
samples; pre-kick PIC requests and the idle address match the previous failures.
Both engines retain 15-second guest-readiness budgets and CPU 0 contention.
All artifact hashes remained unchanged and owned-process cleanup succeeded.
HyperMachine passed 97/100 versus Firecracker 100/100, with slower successful-sample
P50 and P99. This cohort does not establish a startup fix or performance win.

A separate [read-only supported-CPUID probe](benchmarks/2026-09-30/supported-topology-cpu0-probe.py)
records the [CPU 0 KVM table](benchmarks/2026-09-30/supported-topology-cpu0.json).
Leaf 1 EBX is 2099200: its logical-processor count field is 32, while its initial
APIC ID is zero. The current backend only normalizes topology for multi-vCPU VMs,
leaving this supported table unchanged for a one-vCPU guest. This system ioctl
does not sample a running guest and does not establish the startup failure's
cause. It identifies a separate guest-topology consistency gap to address and
verify, including the AMD extended topology leaves.

Single-vCPU cold provisioning now normalizes leaf 1's processor count/APIC ID
and clears HTT; supported extended topology levels describe one processor with
zero shift, while terminators remain terminators. AMD size/core/node leaves and
cache-sharing fields agree with that topology; cache geometry remains intact.
The Linux singleton regression passed, and a real KVM guest executing CPUID
confirmed count 1, APIC ID 0 and HTT clear. Existing halted/spinning owner and
pre-kick machine-state regressions also passed (three KVM tests total).
The [rebuilt-daemon contention cohort](benchmarks/2026-09-30/local-engines-singleton-topology-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-topology-pinned-load-coordinator.py)
record 93/100 HyperMachine passes versus 100/100 Firecracker passes. Artifact
hashes remained unchanged, the CPU 0 worker stayed alive throughout, and cleanup
passed. Both engines retain 15-second readiness budgets. HyperMachine failed
pairs 31, 35, 48, 60, 61, 76 and 94 with no console output and 2401 exits,
unchanged over the observation window. Five post-kick owner samples retained
RIP=0xffffffff81eda95f with IF set; pairs 35 and 60 instead reported
0xffffffff81ed9b1d and 0xffffffff81ed9c50 with IF clear. These later samples can
reflect execution after the kick. Pending unmasked PIC requests remained present.
This verifies the topology change does not eliminate the startup defect;
the failed cohort supports no performance win or causal reliability comparison.
Topology field interpretation follows the [Linux x86 topology documentation](https://docs.kernel.org/arch/x86/topology.html).

Owner diagnostics now have a separate event observation alongside architecture,
without adding fields to persisted vCPU snapshots. KVM_GET_VCPU_EVENTS executes
on the vCPU owner between run calls and reports injected IRQ/vector, exception
state, NMI state and interrupt shadow. Shadow and NMI-pending fields require
their KVM validity flags; absent flags produce unavailable values. Unsupported
backends and read failures preserve architectural output without inventing zero
event state. The Linux validity regression and five daemon formatting tests
passed. All three real-KVM owner tests and strict all-targets Linux core Clippy
passed; live failed-boot integration remains unverified. These independent post-kick reads
cannot prove the exact pre-kick state or the startup failure's cause.

During the owner-event diagnostic cohort, a separate one-Hz, 60-second
[read-only thread probe](benchmarks/2026-09-30/owner-thread-state-probe.py)
recorded [owner CPU observations](benchmarks/2026-09-30/owner-thread-observations.json)
for benchmark daemon PID 39474. Thread 45765 remained observable from
03:38:19.386971 to 03:38:34.404970 UTC on October 1, gaining 575 user and 82 system
ticks; all 16 observations were runnable with wait channel zero. Its last sample
preceded the [failed-boot report](benchmarks/2026-09-30/owner-events-thread-failure-excerpts.txt)
at 03:38:34.426989 UTC by 22 ms. The sequential one-VM cohort and timestamps
support correlation, not a direct thread-to-sandbox identity mapping. CPU
accounting alone cannot separate guest execution from host retry-loop work;
KVM_RUN EINTR/EAGAIN counters are the next check. A preceding 15-second probe
also ran during this cohort. These observer workloads and the shared host limit
performance interpretation; the samples are diagnostic evidence, not a win.

The [completed owner-event cohort](benchmarks/2026-09-30/local-engines-owner-events-pinned-load-100.json)
and [coordinator](benchmarks/2026-09-30/owner-events-pinned-load-coordinator.py)
record 82/100 HyperMachine passes versus Firecracker 100/100. Artifacts remained
unchanged and owned-process cleanup passed. All 18 post-kick failed-boot event
samples report flags 0xd, IRQ_INJECTED=0, valid SHADOW=0 and valid NMI_PENDING=0.
The vector field is not evidence of a pending IRQ when injection is zero.
The raw EXCEPTION_PENDING=0 values are unavailable: flags lack
KVM_VCPUEVENT_VALID_PAYLOAD (0x10), which the current formatter neglected to
check. This reporting error requires correction; these values cannot support
exception-state conclusions. The [KVM API documentation](https://docs.kernel.org/virt/kvm/api.html)
specifies that validity requirement. The failed shared-host cohort, including
the separately recorded thread probes, supports no performance or causal
reliability win. Startup reliability remains unresolved.

The current diagnostic code corrects EXCEPTION_PENDING to an optional field
guarded by KVM_VCPUEVENT_VALID_PAYLOAD. The Linux validity regression passed.
KVM_RUN now counts EINTR and EAGAIN retries separately from guest exits; counters
advance only on those retry paths and are sampled on the owner. Totals include
the diagnostic kick itself, so a small EINTR count is expected and does not
establish a retry storm. The real-KVM halted/spinning regression verifies
cumulative counts and interruption while both guests remain stoppable; all
three owner tests passed. Strict Windows core/daemon and Linux core Clippy
passed, as did all five final daemon formatter tests. The release rebuild passed.

The [completed retry-counter cohort](benchmarks/2026-09-30/local-engines-run-retries-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/run-retries-pinned-load-coordinator.py)
record 91/100 HyperMachine passes versus Firecracker 100/100. Artifact hashes
remained unchanged, the CPU 0 worker stayed alive throughout, and owned-process
cleanup passed. Both engines retained 15-second readiness deadlines. Failed
pairs 22, 50, 52, 53, 54, 58, 80, 89 and 93 all report EINTR=1 and EAGAIN=0,
including the diagnostic kick. These observations provide no evidence of a
host retry storm in the failed boots. All nine retain pending unmasked PIC
requests, no console output and 2401 guest exits. Eight post-kick owner samples
report RIP=0xffffffff81eda95f with IF set; pair 53 instead reports
0xffffffff8105fb53 with IF clear. The independently sampled post-kick events
again show no injected IRQ, valid zero shadow and valid zero NMI pending;
exception pending is correctly unavailable. The next controlled experiment
tests an MP table for singleton cold boots: the loader in this cohort omitted it
for one vCPU, whereas [Firecracker 1.17.0 installs it for all configured CPU counts](https://github.com/firecracker-microvm/firecracker/blob/v1.17.0/src/vmm/src/arch/x86_64/mod.rs).
This is a hypothesis, not an established root cause. The failed shared-host
cohort supports no performance or causal reliability win. No startup fix is
established.

The singleton cold-boot MP-table experiment is now implemented. The real-KVM
Linux 6.6.52 regression passed: the guest discovers the table, reports one
processor and an I/O APIC, activates its UART console and reaches userspace
handoff. The regression now accepts the older `console [ttyS0] enabled` message
as well as the newer `legacy console` wording. Strict all-targets Linux core
Clippy and the release rebuild passed. Live checkpoint/pause/fork validation
passed, as did the first matched contention cohort. These results establish guest
topology discovery on this kernel; they do not establish a startup-reliability
or performance improvement.

The [live snapshot-state report](benchmarks/2026-09-30/singleton-mptable-state.json)
and [exact probe](benchmarks/2026-09-30/singleton-mptable-state-probe.py)
exercise the rebuilt daemon with a newly created snapshot-backed base template
at one vCPU and 1024 MiB. Guest kernel messages confirm the MP table and I/O
APIC. Checkpoint restore rolls a file back from `after` to `before`;
pause/resume preserves that value and command readiness; two forked guests
both return `before`. Guest interrupt tables retain I/O APIC routes before and
after restoration, with local-timer and virtio counts advancing. Artifact
hashes remained unchanged and the isolated node's sandbox records and process
were cleaned up. The first probe incorrectly used cold-only mode, which
rejects checkpoints, and the second supplied an empty JSON pause body; those
setup errors were corrected before this passing run. This validates snapshots
created by this build, not persisted snapshots from an older build, density,
fleet scale or performance superiority.

The [singleton MP-table contention cohort](benchmarks/2026-09-30/local-engines-singleton-mptable-pinned-load-100.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-mptable-pinned-load-coordinator.py)
record 100/100 passes for each engine, with identical kernel/initrd bytes,
one vCPU, 1024 MiB and 15-second readiness budgets. This is the first fully
passing pinned contention cohort in this investigation. Artifact hashes
remained unchanged; the CPU 0 worker stayed alive and was reaped; engine and
daemon cleanup passed. No startup failure was observed in these 100 attempts.
The preceding retry-counter cohort recorded HyperMachine 91/100, but separate
runs on shared hardware do not isolate a causal reliability or timing effect.
HyperMachine remains slower than Firecracker in the passing cohort: successful
readiness P50/P99 were 1107.25/2263.95 ms versus 804.85/1740.15 ms. Zero observed
failures in these cohorts does not establish that startup reliability is fully resolved, and no
performance or across-the-board product win is established.

The [200-pair pinned repeat](benchmarks/2026-09-30/local-engines-singleton-mptable-pinned-load-200.json)
and [exact coordinator](benchmarks/2026-09-30/singleton-mptable-pinned-load-repeat-coordinator.py)
also passed every attempt for both engines. Both MP-table cohorts used the same
daemon binary, kernel, initrd and harness hashes. Artifact stability, worker
liveness and cleanup passed. HyperMachine now records 300/300 passing attempts
across these two pinned cohorts; Firecracker also records 300/300. The repeat
still shows a latency gap: P50/P99 1079.50/1867.38 ms versus 769.48/1573.45 ms.

A separate [five-pair stage diagnostic](benchmarks/2026-09-30/local-engines-stage-timing-pinned-diagnostic-5.json)
and [coordinator](benchmarks/2026-09-30/stage-timing-pinned-diagnostic-coordinator.py)
enable `hv2_sandboxd=debug` to capture existing create-stage fields. All ten
engine attempts, artifact checks, worker liveness and cleanup passed. The five
HyperMachine log entries report construction of 0.38–1.49 ms, launch of
47.47–229.38 ms, launch-to-agent-answer of 1195.70–1769.67 ms, and final setup
of 0.066–0.079 ms. Launch-to-answer includes guest boot, connection setup and
ping; it is not a measurement of transport alone. Its dominant elapsed time
narrows the next investigation to guest boot and readiness rather than the
post-readiness control path. Extra logging and shared-host load make this a
diagnostic, not a performance comparison or evidence of a win.

The [three-pair guest boot diagnostic](benchmarks/2026-09-30/guest-boot-stages-diagnostic-3.json)
and [exact coordinator](benchmarks/2026-09-30/guest-boot-stages-diagnostic-coordinator.py)
collect kernel logs, uptime and interrupt tables after a successful command
from each guest. All six guests, artifact checks, worker liveness and cleanup
passed. The extra guest commands are included in the reported readiness
duration, so those durations must not be used as benchmark results.
HyperMachine's three UART-discovery intervals were 261.407, 268.393 and
262.496 ms; Firecracker's were 0.266, 4.921 and 4.798 ms. HyperMachine reports
a legacy UART on IRQ 4; Firecracker reports an ACPI UART on IRQ 27. The logs
narrow the observed kernel-handoff gap to UART discovery, without establishing
the complete host-side latency cause.

The repository guest configuration enables `CONFIG_SERIAL_8250_DETECT_IRQ`.
[Linux's x86 UART definitions](https://github.com/torvalds/linux/blob/v6.6/arch/x86/include/asm/serial.h)
set `UPF_AUTO_IRQ` when that option is enabled and otherwise retain the standard
COM1 IRQ 4. The [8250 IRQ probe](https://github.com/torvalds/linux/blob/v6.6/drivers/tty/serial/8250/8250_port.c)
calls `probe_irq_on()` twice; the [IRQ probing implementation](https://github.com/torvalds/linux/blob/v6.6/kernel/irq/autoprobe.c)
waits 20 and 100 ms on each call. This supports a fixed-IRQ configuration
experiment for the known virtual board. The guest defconfig now disables IRQ
autodetection while retaining UART and console support; a checksum-verified
Linux 6.6.52 rebuild passed. The original benchmark kernel is preserved.
The effective build configuration was observed with UART and console support
enabled and IRQ autodetection disabled. The real-KVM Linux boot regression
passed with the new image. The [new-kernel snapshot report](benchmarks/2026-09-30/known-uart-irq-state.json)
and [exact probe](benchmarks/2026-09-30/known-uart-irq-state-probe.py)
also passed checkpoint rollback, pause/resume and two forked guests, retaining
I/O APIC UART IRQ 4 and command readiness. Artifact checks and cleanup passed.
The rebuilt kernel SHA-256 is
`afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd`.
A 50-block comparison completed: each block tests both kernels in alternating
order, with two alternating engine pairs per kernel and a fresh isolated
HyperMachine daemon per two-pair run. Both engines use the same kernel within
each run. One pinned CPU worker remains active across all blocks. This design
retains every attempt and reduces time/order bias; it differs from the longer
single-node cohorts above.

The [complete counterbalanced comparison](benchmarks/2026-09-30/uart-irq-kernel-blocks-50.json),
[summary](benchmarks/2026-09-30/uart-irq-kernel-blocks-50-summary.json),
[exact coordinator](benchmarks/2026-09-30/uart-irq-kernel-blocks-coordinator.py)
and [analysis](benchmarks/2026-09-30/uart-irq-kernel-blocks-analysis.py)
record 400/400 successful, cleaned-up attempts: 100 for each engine/kernel
combination. Validation checks all block orders, resources, readiness budgets,
affinity, source/artifact hashes and per-run failure accounting. The worker
stayed alive throughout and was reaped. HyperMachine's original-kernel P50/P99
were 1119.17/2645.60 ms; with the fixed-IRQ kernel they were 859.85/1922.86 ms.
The rebuilt kernel was faster for HyperMachine in 47/50 paired blocks;
the median reduction in each block's mean readiness was 266.46 ms.
Firecracker's corresponding P50/P99 were 787.76/1956.21 and 773.83/1375.09 ms;
its paired-block median reduction was 4.11 ms (27/50 blocks faster).
This records a HyperMachine cold-start improvement on this workload.
HyperMachine still trails Firecracker on both percentiles with the same new
kernel, and shared hardware limits extrapolation, especially for tails.

The [fixed-IRQ guest-log diagnostic](benchmarks/2026-09-30/guest-boot-stages-fixed-irq-diagnostic-3.json)
and [exact coordinator](benchmarks/2026-09-30/guest-boot-stages-fixed-irq-diagnostic-coordinator.py)
also passed all six guest captures, provenance and cleanup checks. The three
HyperMachine UART-discovery intervals were 3.004, 3.770 and 12.542 ms, versus
261.407–268.393 ms with the original kernel. UART IRQ 4 and console support
remain present. Firecracker's intervals were 2.384, 4.436 and 2.635 ms. Other
guest boot stages varied substantially across diagnostic runs; their full
handoff times do not isolate the configuration effect. These extra-command
diagnostics are excluded from the benchmark table. Product feature gaps and
broader performance validation remain open; no across-the-board win is claimed.

The [current native-engine comparison](benchmarks/2026-10-02/current-native-engines/README.md) retains all 636 attempts, including 100 HyperMachine creation failures at concurrency 100. Profiles 1/8/50 fully passed; Firecracker had lower successful P50 and held/incremental process PSS in those profiles. The 100-guest successful latency and memory results are conditional on incomplete HyperMachine evidence. Cleanup was verified and no runtime optimization was adopted. This is a shared-host baseline refresh, not a causal comparison with earlier feature versions. High-concurrency boot/agent readiness remains a priority gap.

[Current high-concurrency readiness diagnosis](benchmarks/2026-10-02/current-readiness/README.md) retained the failed scored batch and ran a separate unchanged-artifact diagnostic. All 400 diagnostic attempts passed, without reproducing the failure. HyperMachine's 200 instrumented attempts had median blocking-worker queue 7.32 ms, agent connection wait 8986.04 ms and ping 4.99 ms. Connection wait includes guest boot, driver and listener readiness; tracing changes timing and does not establish a transport cause or a reliability fix. No runtime change was adopted.

The [event-driven connection-wait experiment](benchmarks/2026-10-02/connection-wait/README.md) passed all 800 baseline/candidate attempts at concurrency 100. The candidate improved batch means in only two of four matched pairs; P50 was 6469.43 ms versus baseline 6460.28 ms, with lower candidate P99. The mixed result did not justify adoption. No wakeup savings, reliability fix or competitor win was established.

The [current-kernel symbol and first-return diagnostic](benchmarks/2026-10-02/first-exit/README.md) maps the retained timeout RIP to `default_idle+15` using the exact failed-cohort kernel. A separate C100 diagnostic passed 400/400 attempts and matched 200 HyperMachine first returns, all OUT to PCI CONFIG_ADDRESS (`0xCF8`). The first backend interval had median 3098.63 ms; owner setup had median 0.09 ms. This directs investigation toward early guest execution and host scheduling before the first PCI probe. It does not isolate CPU work, reproduce or fix the timeout, or establish a performance win. The accepted benchmark baseline remains unchanged.

The [first backend CPU/wall diagnostic](benchmarks/2026-10-02/first-cpu/README.md) passed all 436 attempts at C1/8/100 and matched 218 HyperMachine clock pairs. At C100, median first-call wall time was 2986.04 ms and CPU consumption 218.03 ms, with median CPU/wall fraction 7.30%. Elapsed time not charged to the calling thread dominated this interval; scheduling, blocking and nested-host effects remain unseparated. A missing-clock negative collection preserved four passing guest attempts while correctly failing measurement. This supports testing bounded cold-boot concurrency, without claiming an optimal limit, a timeout fix or competitor superiority. Traced timings remain excluded from scored rankings.

The [durable audit writer comparison](benchmarks/2026-10-02/audit-batching/README.md) retained 204,516/204,516 successful protected-API requests and independently verified 187,416 audit records. Grouping queued records behind the same write-and-sync acknowledgment boundary raised main matched median batch throughput at concurrency 8/50/100 from 219/217/219 to 834/4239/4640 requests per second, with repeat gains at 8/100 after storage timing changed. The one-worker audited path was slightly slower; the longer audit-disabled one-worker control had similar median latency and throughput. All 13 process fault checks and 22 KVM/TLS checks passed, including an actual partial write followed by refusal to append or restart. The grouped writer was adopted for the opt-in audit feature. This is HTTP inventory performance, not guest startup or competitor performance.

The [16-versus-32 cold-slot comparison](benchmarks/2026-10-02/cold-budget-32/README.md) retained all 800 C100 attempts. Sixteen slots passed 400/400; 32 passed 381/400 with 19 guest-readiness timeouts. Conditional successful P50/P99 were lower with 32, but held PSS was higher in every complete pair and mean latency improved in only two of three complete pairs. Cleanup and artifact identity passed. No default changed; these results establish neither a generally preferred setting nor a competitor win.

The [retained readiness snapshot diagnostic](benchmarks/2026-10-02/readiness-snapshots/README.md) maps all 10 RIPs from the 19 cold-slot failures using the exact kernel/initrd. Ten snapshots are halted at default_idle; nine are runnable in several kernel functions. Sequential clock samples do not establish a missed timer or listener backlog. The refusal message now preserves that uncertainty; retry logic and deadlines are unchanged. No reliability fix or performance improvement is claimed.

The [same-binary GNU allocator threshold comparison](benchmarks/2026-10-03/mmap-threshold/README.md)
passes all 128 C8 and 1,600 C100 matched restores, plus source-deletion and
pause/resume checks. Setting only `MALLOC_MMAP_THRESHOLD_=131072` reduced C100
held PSS from 330.10 to 314.91 MiB and empty-daemon PSS from 59.24 to 31.17 MiB.
Mean and P99 improved in both C100 pairs; aggregate P50 barely changed and a
C8 pair's P99 regressed. Firecracker control shifts expose shared-host noise.
The setting remains experimental, with no default adoption or managed-product
win claimed. Failed setup collections are retained and excluded in full.

The [cold-creation follow-up](benchmarks/2026-10-03/mmap-threshold-cold/README.md)
passes all 128 C8 and 1,600 C100 cold attempts under the same allocator policies,
16-slot cold admission limit, deadlines and guest resources. At C100 the fixed
threshold saves 233.41 MiB held PSS but worsens mean readiness by 375.94 ms
and P99 by 1501.11 ms. Both outer-pair means regress; Firecracker controls show
large host shifts. The setting is deferred as a general-purpose default, despite
prepared-restore memory gains. Neither a causal allocator slowdown nor a managed
competitor win is established. All failures remain accounted for, and nineteen
negative evidence contracts pass on Linux and Windows with assertions disabled.

The [explicit first-group deployment check](benchmarks/2026-10-03/tls-group-provisioning/README.md)
passes thirteen owned ACME/live TLS checks, eight provisioning tests and
21 existing renewal tests. The deploy hook can now add a non-overlapping named
certificate group with `--provision-new-group`, preserving and pinning the
existing default for rollback. This removes the pre-existing-group requirement
for initial certificate activation. Automatic issuance after a domain claim,
public CA behavior and additional new-group crash boundaries remain incomplete.

The [first-group publication crash check](benchmarks/2026-10-03/tls-first-group-crash/README.md)
passes fourteen owned ACME/live TLS checks. A hard exit after first-group manifest
publication leaves the live fallback and a durable journal; a retry verifies
recovery without CA reissuance. This covers one new-group crash boundary.
Automatic issuance after a domain claim remains open.

[Configured initial issuance](benchmarks/2026-10-03/initial-tls-worker/README.md)
now passes fifteen owned ACME/live TLS checks. Explicit worker jobs can issue
a missing lineage through HTTP-01 and provision its named TLS group, preserving
the default; non-due retries skip issuance. Eight policy and 21 renewal tests
pass. Automatic job discovery after a domain claim, public DNS/CA behavior
remain incomplete.

The [fresh-account worker check](benchmarks/2026-10-03/fresh-account-tls-worker/README.md)
passes sixteen owned ACME/live TLS checks. A separate, initially absent Certbot
account directory gains exactly one registration before initial issuance and
verified named-group activation; a non-due retry skips issuance. Fresh account
registration is now verified against the owned CA. Public DNS/CA operation,
domain-claim job discovery remain incomplete.

[Initial issuance through KVM guest traffic](benchmarks/2026-10-03/initial-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. A fresh account
and lineage issue a certificate and provision its named group while an existing
16 MiB guest download completes. Routing, reload refusal, restart and guest
lifecycle checks pass with clean teardown. Configured initial issuance is KVM
verified; automatic domain-claim job discovery and public DNS/CA operation remain
incomplete. Failed fixture runs are retained and excluded.

[Automatic claim discovery through ACME and KVM](benchmarks/2026-10-03/discovery-tls-kvm/README.md)
now passes seventeen outer ACME and fifteen real KVM checks. An empty job list
is populated from authenticated, certificate-verified API inventory under operator
suffix policy; guarded initial issuance provisions the named TLS group while an
existing 16 MiB guest download completes. This supersedes earlier statements
that domain-claim job discovery was unverified. Certificate retirement, real
ownership-change interruption, public DNS/CA operation and fleet scale remain
incomplete. No managed-product performance win is established.

[Real pending-job unbind verification](benchmarks/2026-10-03/discovery-unbind-kvm/README.md)
passes seventeen outer ACME and sixteen KVM checks. An actual issued-but-not-yet
activated job survives a real unbind: discovery refuses without changing its
journal, manifest or certificate. Restoring the proof-authorized binding activates
the retained certificate. Live owner replacement and port-change interruptions,
certificate retirement and public DNS/CA operation remain incomplete.

[Live pending-job ownership and port changes](benchmarks/2026-10-03/discovery-ownership-kvm/README.md)
pass seventeen outer ACME and seventeen KVM checks. Real unbind, replacement
owner and guest-port changes refuse a pending job without altering its journal,
manifest or issued certificate; restoring the original binding recovers deployment.
Replacement guests are cleaned up. This supersedes earlier statements that live
owner/port interruption checks were unverified. Certificate retirement, public
DNS/CA behavior and fleet operation remain incomplete.

[Explicit named TLS group retirement](benchmarks/2026-10-03/tls-retirement/README.md)
now passes fifteen owned ACME/live TLS checks plus eight provisioning and
21 renewal tests. `deploy-tls-certificate.py --retire-group` removes an exact
entry, preserves the existing default and immutable files, verifies its pinned
fallback leaf, retries idempotently and permits reprovisioning. The existing
manifest/lineage/generation/process arguments remain required; lineage contents
are unused in retirement mode. Provision and retirement flags are exclusive.
Automatic completed-job retirement, no-default bundles and retirement-specific
crash/KVM verification remain incomplete. This operation sends no application
data during fallback pin checks and does not revoke CA certificates or remove
bindings, old files or established connections.

[Retirement publication crash recovery](benchmarks/2026-10-03/tls-retirement-crash/README.md)
passes sixteen owned ACME/live TLS checks. A hard exit after retirement manifest
publication leaves the previous live leaf and a durable journal; retry reconciles
the manifest, verifies fallback activation and clears the journal without changing
immutable-file inventory. Automatic retirement, additional crash boundaries,
no-default bundles and retirement-specific KVM traffic remain incomplete.

[Connection header and revoked-scope verification](benchmarks/2026-10-03/secret-connection-scope/README.md)
passes all 187 networking tests in the isolated accepted-source checkout.
Secret rewriting excludes Connection-nominated headers and refuses malformed
lists transactionally. Removing and re-adding a sandbox ID creates a new store;
old retained connections stay revoked. Real guest HTTPS lifecycle coverage still
requires a separate client fixture image and an explicitly configured owned
upstream trust root. These results add no performance or managed-competitor claim.

[Explicit daemon upstream roots](benchmarks/2026-10-03/daemon-upstream-root/README.md)
now support an owned HTTPS endpoint without trusting the guest interception CA
upstream. The Linux-only `--egress-upstream-ca` accepts a bounded private operator
PEM bundle with `--network`, adds roots to the public Web PKI for intercepted TLS,
and requires restart for changes. It is node-wide and does not grant egress
access. All 188 network tests and twelve owned daemon startup/reload checks pass.
The accepted guest image still lacks an HTTPS client; guest-level lifecycle
substitution and managed competitor parity remain unverified. No performance
binary changed and no new performance claim is made.

[Owned HTTPS client fixture](benchmarks/2026-10-03/secret-https-client-image/README.md)
now adds installed curl and its hashed shared libraries to a separate accepted
initramfs copy. Two builds produce identical 11,017,803-byte images and curl
starts with HTTPS support under chroot. The accepted benchmark image remains
unchanged. Guest HTTPS traffic, secret scope lifecycle behavior and competitor
feature parity remain unverified; this is fixture readiness evidence only.

[Real KVM secret lifecycle verification](benchmarks/2026-10-03/secret-substitution-kvm/README.md)
now observes seven owned HTTPS requests: placeholder pass-through, header/Basic
/query/JSON substitution, hostname exclusion, SIGHUP rotation, fork scope
exclusion, pause/resume reattachment and revocation. The test exposed and fixed
TLS sniffing for address-based egress with a configured secret store. All 188
network tests pass. Concurrent lifecycle/reload races, managed organization
scope and additional KVM body-format coverage remain incomplete. Separate
verification inputs leave accepted performance binaries and image unchanged;
no competitor parity or new performance win is claimed.

[Real KVM raw/form and rejected reload verification](benchmarks/2026-10-03/secret-body-kvm/README.md)
passes eleven owned checks across thirteen HTTPS requests using unchanged
verification inputs. Form and raw bodies, delimiter-bearing secret escaping and
malformed-policy retention are now observed upstream through a real guest, in
addition to the earlier lifecycle checks. Binary KVM payloads, concurrent
lifecycle/reload races and managed organization scopes remain incomplete. No
runtime changes, managed competitor measurements or performance claims result
from this coverage extension.

[Registration scope refresh and overlapping resume/reload](benchmarks/2026-10-03/secret-registration-kvm/README.md)
passes all 41 daemon tests and twelve real KVM checks across seventeen owned
HTTPS requests. The daemon refreshes the scope under its registry lock after
asynchronous bring-up and failed-pause reinsertion. Four pending resume calls
overlap revoke/re-add reloads and converge to the current secret observed
upstream. Exact registration schedules and failed-pause recovery are not forced;
this does not prove every lifecycle race. Accepted performance inputs remain
unchanged, and no managed competitor or performance claim is added.

[Real KVM binary and upstream hostname verification](benchmarks/2026-10-03/secret-binary-kvm/README.md)
passes fourteen owned checks with nineteen successful HTTPS requests and one
wrong-hostname request rejected before HTTP delivery. Binary payloads preserve
NUL/non-UTF8 bytes around replacements with correct framing. The rejected name
is explicitly scoped and enters interception, while the certificate lacks that
name. Verification input hashes remain unchanged. Exact lifecycle schedules,
failed-pause recovery and managed organization scope remain incomplete; no
managed competitor or performance win is claimed.

[Operator egress Secret chart wiring](benchmarks/2026-10-03/egress-secret-chart/README.md)
passes eight chart tests and Helm lint. Existing policy/root Secret keys are copied
at startup into private regular files; the daemon mounts only the output volume
read-only. Schema/template checks refuse missing networking and invalid keys.
The copy script's ownership, permissions and single-link contract are verified
locally against symlinked inputs. [Operator instructions](EGRESS_SECRETS.md)
explain node-local scopes and required pod restarts after Secret updates. Actual
Kubernetes rollout, updated container image and managed organization service
remain incomplete. No competitor or performance win is claimed.

[Exclusive volume creation](benchmarks/2026-10-03/volume-exclusive-create/README.md)
fixes concurrent duplicate creation overwriting access tokens. A directory is
reserved exclusively before metadata publication; duplicates preserve token/data
and incomplete reservations. All 43 daemon tests and four owned API checks pass;
sixteen concurrent HTTP creates yield one 201 and fifteen 409 responses. Shared
multi-node filesystem behavior, crash repair, power-loss durability and
block-device attach equivalence remain incomplete. Accepted performance inputs
are unchanged and no competitor or performance win is claimed.

[Cross-process shared-directory volume creation](benchmarks/2026-10-03/volume-shared-create/README.md)
passes six owned API checks with two independent daemons and with the retained
single-daemon mode. Sixteen creates across two processes yield one 201 and fifteen
409 responses; both observe the same token, and winner restart preserves token
and data. These standalone APIs share one local filesystem, not a Redis cluster
or network filesystem. Guest mounts, coordinated delete/create and power-loss
storage behavior remain untested by this fixture. Verification input hashes
match the earlier volume fix; no new runtime or performance claim is made.

[Independent volume-store chart wiring](benchmarks/2026-10-03/volume-store-chart/README.md)
passes ten chart tests and Helm lint. An existing node.volumeStore claim mounts
separately and configures --volume-dir, independently of the snapshot-store
claim; defaults retain the existing fallback. [Volume guidance](VOLUMES.md)
distinguishes shared directory data from guest memory snapshots and documents
migration and backup scope. Actual Kubernetes/PVC/network-filesystem operation
remains unverified. No runtime or performance input changed.

[Raw secret rewriting baseline](benchmarks/2026-10-03/secret-rewrite-baseline/README.md)
records eight release-mode synthetic cases. A plain 1 MiB body takes 634.10 us
median batch-average per replacement; dense last-of-128 and unmatched tokens take
3865.96 and 4507.04 us. This identifies prefix scanning and token lookup as
optimization targets. It is one unpinned process, not HTTP/TLS/KVM or competitor
performance, and establishes no improvement or service P99 claim.

[Bulk-copy raw secret rewriting](benchmarks/2026-10-03/secret-bulk-copy/README.md)
reduces the plain 1 MiB synthetic component median from 647.03 to 19.78 us and
sparse input from 637.36 to 25.04 us over four alternating release binary pairs.
Dense matching across 128 bindings is essentially unchanged. All 189 networking
tests and fourteen owned KVM checks pass on the changed implementation. These
are raw replacement timings, not full HTTP/TLS/KVM latency or competitor results;
no service P99 or across-the-board performance win is claimed. Accepted VM
benchmark inputs remain unchanged.

[Ordered raw placeholder lookup](benchmarks/2026-10-03/secret-ordered-lookup/README.md)
reduces the dense last-of-128 component median from 3842.64 to 649.18 us and dense
unmatched input from 3861.55 to 743.87 us versus the verified bulk-copy binary,
over four alternating release pairs. Other cases are broadly unchanged with
small shifts in both directions. All 190 networking tests and fourteen real KVM
checks with 128-binding policies pass. One-time policy sorting, full HTTP/TLS
latency, fleet performance and competitors remain unmeasured; no across-the-board
win is claimed. Accepted VM benchmark inputs remain unchanged.

[Large-body owned HTTPS timing fixture](benchmarks/2026-10-03/secret-large-http/README.md)
passes fifteen KVM checks and validates every byte of twelve timed 1,035,000-byte
raw requests, plus three warm-ups. Each substitutes 15,000 tokens under a
128-binding policy into 180,000 bytes with correct framing. Guest curl time_total
is recorded on the debug verification binary; this is measurement preparation,
not a matched release result or proof that component gains improve service
latency. Runtime and accepted benchmark inputs remain unchanged.

[Matched release KVM HTTPS rewriting](benchmarks/2026-10-03/secret-https-release/README.md)
observes median-of-run-medians 17.340 -> 14.844 ms (-14.4%) over two alternating
pairs for a verified 1,035,000-byte raw request with 128 bindings and 15,000
replacements. All fifteen correctness checks pass in every cohort. The server
uses TCP_NODELAY in both variants, and source catalogs differ only in the
replacement module. An initial identical-artifact comparison is excluded; the
runner now rejects identical binaries before execution. This is one synthetic
local workload, not service P99, fleet throughput or a competitor result.
Accepted VM benchmark inputs remain unchanged; across-the-board wins are unproven.

[Eight-client release KVM HTTPS comparison](benchmarks/2026-10-03/secret-https-concurrent/README.md)
uses the same release inputs and observes median-of-run-medians 114.349 ->
113.806 ms (-0.5%), with mixed pair results: essentially unchanged. All fifteen
checks pass in each cohort; 96 timed requests per cohort and HTTP overlap peaks
5–7 verify concurrent execution. The sequential 14.4% improvement does not carry
over to this workload. Bottleneck profiling remains required. This is one guest
with eight clients, not eight VMs, service P99, throughput or competitor evidence.

### Concurrent secret HTTPS CPU accounting (2026-10-03)

Four additional eight-client cohorts passed all correctness and cleanup checks. Median request latency was 113.961 to 112.242 ms (-1.5%); this small difference does not establish a concurrent gain. The measured 12-batch windows used 0.112–0.130 CPU seconds in the Python driver and owned HTTPS server, compared with 2.85–3.11 daemon user-plus-system CPU seconds (user already includes guest). Batch wall was 1.745–1.775 seconds. These counters favor investigating daemon/guest work next but do not identify a bottleneck. [Frozen CPU accounting evidence](benchmarks/2026-10-03/secret-https-cpu/README.md) records scope, tick quantization, input identity, all reports, and limitations. Managed competitors remain unmeasured.

### Concurrent secret HTTPS thread accounting (2026-10-03)

Four matched eight-client cohorts passed all checks and cleanup. Median request latency was 112.948 to 112.980 ms, effectively unchanged. The vCPU thread used 1.70–1.74 user-plus-system CPU seconds per measured cohort, more than half of daemon CPU. This includes guest execution and vCPU host work and supports investigating guest execution/VM exits next; it does not identify a causal bottleneck. [Frozen thread accounting table](benchmarks/2026-10-03/secret-https-threads/README.md) records group totals, per-thread reports, identity checks, quantization, and limitations. Managed competitors remain unmeasured.

### Guest CPU scaling for concurrent secret HTTPS (2026-10-03)

An unchanged optimized release daemon was tested with one/two/two/one guest vCPUs and eight concurrent curl processes. All four cohorts passed all 15 correctness/lifecycle checks and cleanup, including verified guest CPU counts. Median of request run medians fell from 115.217 to 45.574 ms (60.4%); measured batch wall fell from 1.800 to 0.973 seconds. This is an extra-resource configuration result, not a code speedup or equal-resource competitor win. Prepared template sizes may override node defaults. [Frozen CPU scaling table](benchmarks/2026-10-03/secret-https-cpu-scaling/README.md) includes reports, binary identities, runner, and limits. Evaluate two vCPUs for similar parallel workloads; no default change or general performance claim follows.

### Equal-resource two-vCPU secret HTTPS comparison (2026-10-03)

Original versus optimized release daemons at equal two-vCPU allocation passed four complete eight-client KVM cohorts. Median latency was 48.070 to 48.652 ms (+1.2%) with opposing pair directions, so no concurrent latency gain is established. Daemon user-plus-system CPU fell in both measured windows (2.65 to 2.49 and 2.63 to 2.45 seconds); this remains a short local accounting observation. [Frozen equal-resource table](benchmarks/2026-10-03/secret-https-two-cpu/README.md) separates these results from the verified extra-vCPU scaling gain and single-client code latency improvement.

### Deployment guest sizing validation (2026-10-03)

The chart already exposes node CPU/memory defaults. Schema now refuses zero, negative, fractional, mistyped, and representationally overflowing values; rendered argument checks cover one/two/four vCPUs. All 12 chart tests and lint passed. [Frozen sizing checks](benchmarks/2026-10-03/guest-sizing-chart/README.md) record exact coverage and limits. Operator guidance includes the measured two-vCPU configuration without changing defaults or claiming capacity/rollout verification.

### Direct daemon guest sizing validation (2026-10-03)

The standalone CLI now rejects zero guest CPU/memory and checked GiB conversion refuses overflow before startup. All 44 daemon unit tests and 17 direct invalid-argument invocations passed in the isolated accepted-core checkout. [Frozen CLI sizing evidence](benchmarks/2026-10-03/daemon-guest-sizing/README.md) includes source, build/test logs, binary identity, and exact errors. This closes the direct-launch validation gap alongside Helm validation without changing valid resource defaults or claiming backend capacity.

### Secret policy updates on established HTTPS connections (2026-10-03)

Owned KVM checks now prove next-request rotation and revocation on one established guest HTTPS connection. Three transfers asserted curl connection counts 1/0/0 and upstream original/updated/placeholder payloads with matching framing. Current debug (128 bindings, one vCPU) and immutable release (one binding, two vCPUs) cohorts each passed all 15 checks and cleanup. [Frozen keepalive evidence](benchmarks/2026-10-03/secret-https-keepalive/README.md) states the completed-reload boundary and separates guest connection reuse from upstream connection behavior. In-flight and fleet-wide atomicity remain unproven.

### Volume metadata persistence (2026-10-03)

Volume creation now flushes metadata/data and publication directories before success, with private 0600 metadata. Failure handling preserves published volumes rather than removing their data directory after rename. All 45 daemon tests plus the two-daemon 16-request/restart fixture passed. [Frozen flush evidence](benchmarks/2026-10-03/volume-metadata-sync/README.md) includes four injected sync-stage failures and explicit filesystem/power-loss limits. This improves local persistence handling; it does not establish managed block-volume or network-storage parity.

### Volume content upload persistence (2026-10-03)

Content uploads now sync the completed file and every containing directory to the data root before success. All 45 daemon tests and the expanded two-daemon binary-upload/replacement/token-refusal/restart fixture passed. [Frozen upload evidence](benchmarks/2026-10-03/volume-content-sync/README.md) records safe descriptor opening and boundaries. Interrupted replacements remain in-place and can be partial; power-loss, network storage, guest-write guarantees and flush performance remain unverified.

### Opt-in atomic volume content uploads (2026-10-03)

`atomic=true` stages and syncs the replacement before pinned-parent rename publication. All 46 daemon tests and both two-daemon atomic/default API cohorts passed. The atomic cohort verifies an incomplete HTTP request returns 400, preserves previous bytes, and removes its staging file. [Frozen atomic-upload evidence](benchmarks/2026-10-03/volume-atomic-upload/README.md) includes exact source, input hashes, compatibility checks and inode/error semantics. Hard-kill staging recovery, power-loss and network-filesystem guarantees remain incomplete.

### Verified volume upload timing (2026-10-03)

The same dev-profile daemon completed four alternating in-place/atomic upload cohorts, each with three warm-ups and twelve timed 1,048,586-byte PUTs followed by exact readback on both nodes. All correctness/restart/cleanup checks passed. Atomic mode was slower in both pairs (19.660 to 22.905 ms and 9.975 to 15.083 ms); large time drift prevents a stable overhead estimate. [Frozen timing evidence](benchmarks/2026-10-03/volume-upload-timing/README.md) includes sample arrays and scope. No release, fleet, network storage or competitor performance claim follows.

### Atomic upload redundant-flush removal (2026-10-03)

Atomic uploads now flush their pinned immediate parent once and continue with ancestors; the default path still flushes its whole chain. All 46 daemon tests and four two-daemon upload cohorts passed, including new root-level atomic publication coverage and incomplete-request preservation. [Frozen single-parent-sync evidence](benchmarks/2026-10-03/volume-single-parent-sync/README.md) records current timings without claiming a before/after gain.

### Volume client and endpoint boundary (2026-10-03)

A streamed Python operator upload helper now defaults to atomic replacement and passes exact binary readback on two owned nodes. Source inspection confirms volume routes are node-only; control-plane volume management/content routing remains absent. The current matrix and storage guide now state this limitation rather than implying full control-plane volume compatibility. [Frozen client evidence](benchmarks/2026-10-03/volume-upload-client/README.md) records the local check and unverified TLS/size/fault limits.

### Volume upload client TLS validation (2026-10-03)

All seven owned upload-client tests pass, including custom-CA HTTPS, untrusted/wrong-hostname refusal before HTTP arrival, no platform API-key forwarding, redirects, invalid/oversized input, empty uploads and timeout/response errors. [Frozen TLS client evidence](benchmarks/2026-10-03/volume-client-tls/README.md) records scope and cleanup. This verifies the direct-node helper's transport behavior; control-plane routing, public CA/proxy operation and maximum-size success remain incomplete.

### Connected volume upload timeout (2026-10-03)

The upload helper now enforces the remaining connected-transfer deadline with socket shutdown, refusing slowly arriving responses rather than extending each read indefinitely. All eight owned client tests and the real two-daemon upload workflow pass. [Frozen deadline evidence](benchmarks/2026-10-03/volume-client-deadline/README.md) states DNS/local-I/O and post-publication uncertainty limits.

### Correction: sandbox control-plane volume routes

The earlier node-only conclusion inspected the general `hv2-api` server rather than the sandbox control plane in `hv2-cluster/src/control.rs`. The sandbox control plane already routes `/volumes`, `/volumes/{id}` and `/volumecontent/{id}/{file,dir,path}`. Management is API-key protected; content uses the volume bearer token checked by the selected node. Rendezvous placement is over volume ID, and list merges duplicate IDs from live nodes. The current matrix and volume guide are corrected. Older journal entries and frozen client archives preserve the mistaken conclusion as historical evidence and must not be used for current routing status.

The [focused control-plane volume test](benchmarks/2026-10-03/control-volume-routing/README.md) passes actual HTTP management/content routing to two protocol nodes, stable placement, binary/query preservation, key/bearer/cluster credential boundaries and duplicate-list merging. This is routing evidence; actual shared-storage failover and control-plane-to-daemon filesystem verification remain incomplete.

### Real control-plane volume forwarding (2026-10-03)

The opt-in integration test now passes against two real daemons sharing a local volume root. It verifies authenticated create, 1 MiB atomic binary upload and on-disk bytes, routing eligibility changes to each peer with unchanged token/content, both daemon restarts, bearer refusal, deduplicated listing and deletion cleanup. [Frozen real-daemon evidence](benchmarks/2026-10-03/control-real-volumes/README.md) records before/after input hashes, source and test logs. This uses the actual control router with MemoryStore in the test process; Redis, shipped control-plane process, network storage, TLS/mTLS and abrupt in-flight failures remain unverified.

### Selected-node kill during atomic volume upload (2026-10-03)

The real-daemon control-plane test now waits for partial private staging, kills the selected node, verifies a 502 response and exact old destination bytes on the eligible peer, then verifies both daemon restarts and cleanup. An orphan staging file remains mode 0600 as expected after hard termination. [Frozen crash evidence](benchmarks/2026-10-03/control-volume-crash/README.md) records the controlled pre-publication boundary. Automatic orphan repair, post-rename crash, power loss, Redis health failover and network storage remain incomplete.

### Create-only volume uploads (2026-10-03)

`overwrite=false` and client `--no-clobber` now provide exclusive creation. Atomic publication uses RENAME_NOREPLACE, while in-place creation uses O_EXCL. All 46 daemon/eight client tests pass; two-node 16-upload cohorts have one winner and fifteen conflicts in each mode, with exact bytes and client refusal verified. [Frozen create-only evidence](benchmarks/2026-10-03/volume-create-only-upload/README.md) records filesystem and interrupted-write limits. No compare-and-swap, network-storage or competitor win is claimed.

### Integrated volume management CLI (2026-10-03)

`hm sandbox vm volume create/list/inspect/delete` now uses authenticated bounded API requests and validates volume names/IDs. The shipped-binary volume test, all 12 existing VM CLI protocol tests, and real control-router/two-daemon management operations pass. [Frozen CLI evidence](benchmarks/2026-10-03/volume-management-cli/README.md) records exact inputs/source and compatibility. Streaming volume content remains in the separate helper; Redis/new-command TLS and fleet storage guarantees remain incomplete.

### Integrated streamed volume upload (2026-10-03)

`hm sandbox vm volume upload` now streams regular files with atomic default, create-only and force options using the volume bearer token and no platform API key. Both shipped volume tests, 12 existing CLI regressions and actual control-router/real-storage uploads pass, including a repeated no-clobber refusal with unchanged bytes. [Frozen streamed upload evidence](benchmarks/2026-10-03/volume-cli-stream-upload/README.md) records source/input identity and scope. Integrated download, new-command TLS, full-size transfer and fleet storage guarantees remain incomplete.

### Integrated streamed volume download (2026-10-03)

`hm sandbox vm volume download` now stages, syncs and publishes a streamed local file without replacing an existing destination. Four volume CLI tests and 12 existing VM CLI tests pass; actual control-router/real-storage roundtrips verify exact bytes and repeated download refusal. [Frozen download evidence](benchmarks/2026-10-03/volume-cli-stream-download/README.md) includes truncated-response cleanup and input/source identity. New-command TLS, full-size/race injection and power-loss/fleet storage guarantees remain incomplete.

### Integrated volume CLI TLS (2026-10-03)

Five owned shipped-CLI transport tests pass for upload/download: custom-CA HTTPS with a proper leaf, exact bytes, bearer-only authorization, trust/hostname and redirect refusal, slow-response deadline, truncated/advertised-oversized download cleanup. [Frozen CLI TLS evidence](benchmarks/2026-10-03/volume-cli-tls/README.md) records the checked binary and scope. Public CA, shipped control-plane HTTPS/mTLS, full-size transfer and managed storage guarantees remain incomplete.

## Broader CLI and cluster regression verification

The accepted isolated checkout passes the full CLI and cluster suites: 286 tests, zero failures and one ignored real-daemon test. [Frozen regression evidence](benchmarks/2026-10-03/cli-cluster-regressions/README.md) records exact scope and isolated source context. The ignored test passed separately with owned daemon inputs in the directory-sync fixture. These checks support the current CLI/routing changes; they do not prove competitor parity or validate protected root core modifications.

## UDP implementation and node-level KVM evidence

Framed UDP now has guest/host methods, authenticated node/control routes and a bounded per-peer loopback CLI. Separate protocol fixtures verify routing and CLI peers. [Real node-level KVM verification](benchmarks/2026-10-03/udp-kvm/README.md) passes empty/binary/maximum IPv4-size payloads, credential refusal and pause/resume/delete session handling. Combined real control-plane/CLI/TLS integration remains outstanding, so this is partial UDP implementation rather than completed networking parity.

## Current API source verification correction

A full comparison verifies that current root API/cluster Rust sources and manifests match the accepted isolated checkout. [Full API regression and matching-source catalog](benchmarks/2026-10-03/current-api-regressions/README.md) supersedes earlier UDP archive statements about mismatched API TLS sources. Isolated core/lock provenance still limits this to the stated test context rather than whole-root workspace validation.

### Local UDP write optimization follow-up (2026-10-03)

| Workload | Baseline average round trips/sec | Combined-write average | Change | Evidence |
|---|---:|---:|---:|---|
| Two concurrent peers, 64-byte datagrams | 1,515.4 | 1,825.3 | +20.4% | [Matched ABBA](benchmarks/2026-10-03/udp-combined-write/README.md) |
| Two concurrent peers, 4 KiB datagrams | 1,427.9 | 1,647.3 | +15.4% | [Matched ABBA](benchmarks/2026-10-03/udp-combined-write-4k/README.md) |
| Eight concurrent peers, 4 KiB datagrams | 3,763.5 | 4,159.5 | +10.5% | [Matched ABBA, median tradeoff](benchmarks/2026-10-03/udp-eight-peer-4k/README.md) |

These compare preserved HyperMachine CLI binaries through fresh HTTPS/mTLS/Redis/KVM stacks, with 1,000 exact replies per peer per cohort and passing correctness/lifecycle checks. Two-peer median latency is mixed; with eight peers the mean of per-peer sample medians rises 5.1% (slower), and P99 is mixed. These short local development-build measurements establish an internal completion-rate improvement; Boxd and exe.dev endpoints were unavailable, so their UDP performance remains unmeasured. Sustained capacity, resource tradeoffs and an across-the-board competitor win remain unproven.

The subsequent [UDP peer-capacity cleanup](benchmarks/2026-10-03/udp-peer-recovery/README.md) fixes panic/cancellation slot leaks and verifies EOF reconnection. Its [matched development-build comparison](benchmarks/2026-10-03/udp-peer-recovery-performance/README.md) shows a 5.6% completion-rate regression and 6.5% higher mean per-peer sample medians versus the preceding combined-write binary. Earlier optimization numbers concern preserved binaries; they do not demonstrate that the latest binary improves every metric. Release-build evaluation remains pending.

A [matched release-CLI follow-up](benchmarks/2026-10-03/udp-peer-recovery-release/README.md) reduces the measured cleanup rate regression to 1.3%, with 0.7% higher mean per-peer sample medians and generally higher P99. The fixed daemon/control remain development builds. Release EOF recovery passes; release panic=abort terminates the process, so prior panic-cleanup injection evidence applies to unwinding profiles. Production overhead and an across-the-board performance win remain unproven.

[IPv6 source alignment](benchmarks/2026-10-03/udp-ipv6-source-alignment/README.md) resolves the initial nineteen unrelated byte differences as line endings/formatting. All six IPv6 crates now match current root Rust source/tests/manifests exactly in the accepted checkout; rebuilt regressions and fresh IPv4/IPv6 TLS/mTLS/KVM runs pass. Accepted core and lockfile limitations still prevent a whole-root-workspace claim.

[Root lockfile alignment](benchmarks/2026-10-03/udp-root-lock-alignment/README.md) establishes that the previous root/isolated lockfile difference was CRLF/LF only, with identical parsed dependencies. Exact root lockfile/workspace manifest bytes now match the accepted checkout, and offline locked compilation passes. The protected accepted-core boundary and uncataloged workspace scope still limit whole-root verification.

### Matched guest IPv4/IPv6 configuration (2026-10-03)

| Guest destination | Average round trips/sec | Mean per-peer sample medians (ms) | Evidence |
|---|---:|---:|---|
| IPv4 loopback | 3,916.1 | 1.9881 | [Same-binary ABBA](benchmarks/2026-10-03/udp-ipv6-family-performance/README.md) |
| IPv6 loopback | 3,912.3 | 1.9897 | [Same-binary ABBA](benchmarks/2026-10-03/udp-ipv6-family-performance/README.md) |

Eight peers, 4 KiB payloads, identical development binaries, IPv4 local peers, one guest vCPU/1 GiB and fresh HTTPS/mTLS/Redis/KVM stacks. IPv6's sampled rate is 0.10% lower; cohort latency directions are mixed. This compares guest-family configurations and establishes no statistical equivalence, production overhead or competitor superiority.

### Raw-port contract recheck (2026-10-03)

Boxd documents managed public raw ports with stable allocation, optional TCP/UDP on one port, owner-controlled management and reboot persistence ([official documentation](https://docs.boxd.sh/llms-full.txt), Port forwarding). exe.dev documents authenticated additional HTTP proxy ports 3000–9999; only one selected target can be public ([official documentation](https://exe.dev/docs/all), HTTPS proxy / Additional Ports). The reviewed exe.dev page does not establish a native UDP service. These documented capabilities are distinct from HyperMachine's verified local framed tunnels.

A durable allocation and ownership model is required before native listener wiring can establish parity. Verification must include allocation concurrency, stable restart/reboot identity, protocol updates without port changes, scope/owner refusals, exact datagrams, bounded peer resources, removal and guest-deletion cleanup. An ephemeral loopback listener alone would leave the documented managed-port contract incomplete.

A [deferred native UDP reply-buffer candidate](benchmarks/2026-10-03/native-udp-buffer-reuse/README.md) passes eight counterbalanced KVM profiles and five targeted tests. Development-profile mean native rates rose 1.77% with two IPv4 peers and 0.38% with eight IPv6 peers; mean peer median/P99 values fell slightly. These small differences lack release-profile and retained-memory evidence. The candidate retains up to 65,507 bytes per peer and was not promoted; production source is unchanged. No competitor win follows.

The [release reply-buffer experiment](benchmarks/2026-10-03/native-udp-buffer-reuse-release/README.md) fills the earlier release/memory evidence gap but remains deferred. Native rates changed +12.89% with two IPv4 peers and −12.44% with eight IPv6 peers; eight-peer mean P99 rose 84.10% and gateway PSS after traffic rose 6.85%. The unchanged CLI reference rates also shifted +14.43%/−6.09%, leaving causal attribution unresolved. All eight profiles passed 23 checks and cleanup. Production source is unchanged; no across-metric or competitor win is established.

The subsequent [bounded reply-buffer release experiment](benchmarks/2026-10-03/native-udp-bounded-reuse-release/README.md) passed eight profiles but did not justify promotion: native rates fell 16.96%/0.78% for two IPv4/eight IPv6 peers, while sampled gateway PSS rose 5.36%/2.33%. The two-peer unchanged CLI reference fell 34%, leaving strong confounding. Production remains unchanged; dropping large Vec capacity does not guarantee lower process memory.

A fresh [CPU-affinity-matched bounded reuse comparison](benchmarks/2026-10-03/native-udp-bounded-reuse-affinity/README.md) also leaves the candidate deferred: native rate −15.45%/−2.35%, mean peer P99 +85.20%/+14.00%, gateway PSS +2.68%/+1.26% for two IPv4/eight IPv6 peers. The unchanged CLI reference slowed too. All eight 23-check profiles passed; production remains unchanged. CPU placement control does not eliminate host contention.

[Resume ownership lookup cancellation](benchmarks/2026-10-03/resume-ownership-lookup-cancellation/README.md) now restores shared paused metadata when the ownership lookup is cancelled. All 49 daemon tests pass, including synchronized cancellation and completed-error cleanup checks. This covers the pre-startup store wait, not VM startup/registration cancellation, in-memory-only pauses or machine-crash recovery; updated KVM verification remains pending.

The [updated ownership-lookup guard daemon](benchmarks/2026-10-03/resume-ownership-lookup-kvm/README.md) now passes both real KVM migration profiles, 27 checks each, preserving owner, execution, fork and the same both-protocol public port across nodes. Cleanup leaves zero guests and reaps all owned processes. These verify normal lifecycle regression; deterministic lookup cancellation remains separately proven by the daemon test, not by an HTTP disconnect.

[In-memory resume lookup rollback](benchmarks/2026-10-03/resume-local-lookup-cancellation/README.md) retains local paused state during authoritative ownership lookup and restores it on cancellation when no shared store exists. All 51 daemon tests pass, including synchronized rollback and completion checks. Updated KVM verification, startup/registration cancellation and machine-crash recovery remain pending or incomplete.

The [current paused-state rollback daemon](benchmarks/2026-10-03/resume-local-lookup-kvm/README.md) passes four KVM profiles: two shared-store migration profiles with 27 checks each and two in-memory paused-state profiles with 22 checks each. All verify exact traffic, lifecycle and complete cleanup. Cancellation remains separately proven by deterministic tests; startup/crash recovery remains incomplete.

[Pre-startup network decision rollback](benchmarks/2026-10-03/resume-network-decision-cancellation/README.md) now protects paused state while resume resolves an egress proxy. All 51 daemon tests pass with the existing cancellation/completion helper checks. Real networked resume verification remains pending; VM startup/registration cancellation and machine-crash recovery remain incomplete.

[Direct proxy-resolution guard tests](benchmarks/2026-10-03/resume-proxy-resolution/README.md) bring the daemon suite to 53 passing tests: actual localhost resolution verifies default private-address refusal and explicit operator allowance while preserving the paused value. Networked KVM/cancellation injection and startup/crash recovery remain unverified or incomplete.

[Fresh-daemon network reconstruction](benchmarks/2026-10-03/network-fresh-daemon-resume/README.md) passes 16 real KVM/owned HTTPS checks: a paused guest resumes from shared metadata in a replacement daemon without cached network state, preserving exact scoped substitution and final revocation/deletion cleanup. This request has no egress proxy; DNS cancellation and crash/startup recovery remain unverified or incomplete.

[Unnamed registration uncertainty](benchmarks/2026-10-03/unnamed-registration-uncertainty/README.md) now retains a pending marker for clustered unnamed creation/resume until shared publication succeeds, returns 503 on uncertainty and supports authenticated reconciliation of the preserved guest. All 53 existing daemon tests pass; targeted Redis failure/cancellation and updated KVM verification remain pending.

[Unnamed resume publication-fault recovery](benchmarks/2026-10-03/unnamed-resume-publication-fault/README.md) passes both 23-check KVM profiles. Owned Redis SET refusal yields 503 and preserves the paused record and live guest; pause mutation/unauthorized reconciliation refuse, then authenticated recovery publishes the same running owner record and exact forwarding succeeds. Cancellation, event-only faults, initial-creation failure and machine-crash recovery remain unverified or incomplete.

[Committed-record/event-only resume failure](benchmarks/2026-10-03/unnamed-resume-event-fault/README.md) passes two 23-check KVM profiles: owned Redis XADD refusal retains the committed running owner record while returning 503, blocks pause, then authenticated reconciliation and forwarding recover the same guest. Exactly-once events, cancellation, initial creation faults and crash recovery remain unverified or incomplete.

[Administrator explicit-node registration recovery](benchmarks/2026-10-03/registration-recovery-cli/README.md) is now available through the control plane and shipped CLI, including when a failed initial write leaves no sandbox record. 88 cluster tests and 38 filtered CLI tests pass; end-to-end forwarding/scoped authorization and runtime response-bound verification remain pending.

[Shipped administrator recovery CLI](benchmarks/2026-10-03/registration-recovery-cli-kvm/README.md) passes four 24-check KVM profiles covering SET/XADD failures across both ingress families. Legacy/scoped administrators recover the same guest; observer, inventory and sandbox scopes, unknown nodes and replay refuse. Descriptor identity/token and exact forwarding persist. Initial creation faults, response-bound runtime refusal, cancellation and crash recovery remain unverified or incomplete.

[Initial unnamed creation recovery](benchmarks/2026-10-03/initial-registration-cli-recovery/README.md) passes four 25-check KVM profiles. SET failure leaves no shared record and XADD failure leaves a committed record; node-local inventory identifies the preserved guest, then explicit-node administrator CLI recovery restores the same owner and exact execution before deletion. Initial creation uses trusted node API; automatic missing-registration discovery, cancellation and crash recovery remain incomplete.

[Administrator pending-registration discovery](benchmarks/2026-10-03/pending-registration-discovery/README.md) now exposes bounded paginated node-local IDs through API/CLI without guest access tokens, including when shared records are missing. 54 daemon, 88 cluster and 39 focused CLI tests pass; real KVM discovery/scoped authorization and crash recovery remain pending or incomplete.

[Administrator CLI discovery-to-recovery](benchmarks/2026-10-03/pending-discovery-kvm/README.md) passes four 27-check KVM profiles for initial/resumed SET and XADD failures. Missing-record guests are discoverable by ID without capability fields; unauthorized scopes/unknown nodes/cursor exclusions refuse, and recovery clears entries while preserving execution and forwarding. Multi-page behavior remains unit-tested; automatic reconciliation and crash/startup recovery remain incomplete.

[Discovery schema enforcement](benchmarks/2026-10-03/pending-discovery-schema/README.md) now validates node pages before public forwarding: only ID/kind fields, bounded ordered rows and consistent cursors are accepted; capability fields and malformed pages refuse. All 89 cluster tests pass. Updated KVM and malformed remote-response fixtures remain pending.

[Strict discovery runtime verification](benchmarks/2026-10-03/pending-discovery-schema-runtime/README.md) passes 13 owned HTTP boundary checks plus four 27-check KVM discovery/recovery profiles. Capability fields, malformed/oversized pages and invalid cursors refuse; client API keys are stripped, and valid KVM recovery remains functional. Automatic reconciliation, cancellation and crash recovery remain incomplete.

[Discovery transport verification](benchmarks/2026-10-03/pending-discovery-http-transport/README.md) passes 17 owned HTTP checks, including valid/oversized chunked pages, truncated bodies and invalid chunk framing. The [operator guide](NATIVE_PORT_GATEWAY.md#recovering-uncertain-guest-registration) now documents discovery, cursor pagination and same-guest reconciliation after uncertain registration. Production code is unchanged; automatic and crash recovery remain incomplete.

The [additional publisher-ownership guard](benchmarks/2026-10-03/registration-publisher-ownership/README.md) is withdrawn: source inspection shows initial creation already holds the same transition lock as reconciliation. The redundant atomic guard and its two tests were removed. Current serialization is verified below.

[Initial publication/reconciliation serialization](benchmarks/2026-10-03/registration-publication-serialization/README.md) passes four 28-check KVM profiles. A held Redis write exposes the live guest while both requests remain pending; after release, original creation succeeds and serialized reconciliation refuses the cleared marker. This supersedes the earlier incorrect race diagnosis. Automatic recovery, startup cancellation and machine-crash recovery remain incomplete.

Opt-in automatic local registration reconciliation is implemented with bounded rotating batches and per-attempt deadlines. [Worker source and 56 passing isolated daemon tests](benchmarks/2026-10-03/automatic-registration-worker/README.md) cover interval bounds and selection fairness. Runtime automatic recovery under publication faults remains unverified; this adds no competitor performance claim.

[Automatic registration recovery in real KVM guests](benchmarks/2026-10-03/automatic-registration-kvm/README.md) passed four profiles with 30 checks each: initial creation and resume recover automatically after Redis SET/XADD refusal, with IPv4 two peers and IPv6 eight peers. Failed retries preserve pending guests; restoration clears pending state without manual reconciliation, retains resumed access tokens, and restores exact native TCP/UDP traffic on stable ports. All fixture processes and guests are cleaned up. This supersedes the runtime-unverified worker note above; daemon-crash durability and competitor performance remain unproven.

[Repeated cluster-store timeout recovery](benchmarks/2026-10-03/registration-store-timeout-kvm/README.md) passed two owned KVM profiles with 32 checks each. Initial and resumed guests remain pending through multiple store timeouts during an owned Redis write stall, then recover automatically with exact execution/native traffic and complete cleanup. The store deadline preempts the worker’s outer five-second deadline; the outer timeout branch and daemon-crash recovery remain unverified.

[Current same-binary cold-start stage diagnostics](benchmarks/2026-10-03/current-cold-start-stages/README.md) pass 116/116 guests per engine with complete cleanup. Guest-agent connection wait dominates both profiles; VM build and blocking-worker queue intervals are smaller. At C100, a large first-backend-call wall interval directs further investigation toward guest execution and vCPU scheduling. Tracing changes timing, the interval is not CPU time, and no causal fix or performance gain is established. The scored current-release comparison above remains authoritative for rankings.

[Current first-KVM-call CPU/wall diagnostics](benchmarks/2026-10-03/current-cold-first-call-cpu/README.md) pass 108/108 guests per engine. At C8 the median per-guest thread-CPU/wall ratio is 98.5%; at C100 it is 7.46%. This is consistent with substantial scheduling/waiting effects under oversubscription and directs the next scored experiment toward existing cold-start admission limits. It does not establish a cause or justify a default change; tracing and one-pair cohorts remain excluded from rankings.

[Retained current cold-failure symbol mapping](benchmarks/2026-10-03/current-cold-failure-symbols/README.md) verifies all 229 timeout snapshots against an independently booted identical kernel: 202 runnable at varied kernel locations and 27 halted in default_idle. The halted samples have nonzero deadlines later than their sampled TSC, so these records do not prove expired deadlines or missed timer delivery. This separates failure groups without establishing a causal fix; the cold-start reliability gap remains.

[Unregistered startup cancellation cleanup](benchmarks/2026-10-03/unregistered-startup-cancellation/README.md) now carries VM/network ownership through bring_up and the registration-lock wait, disarming only after local registry insertion. Cancellation stops the unregistered VM and aborts its network bridge; uncertain shared publication still preserves the registered guest. The final daemon passes 58 ordinary tests, two explicit KVM guard tests, two 32-check publication profiles and 16 real-network checks. Runtime shutdown, complete HTTP-disconnect cancellation and atomic shared-claim crash fencing remain unverified. Earlier optimized performance cohorts use their archived source/binary and do not measure this guard change.

[Pending-registration idle-eviction starvation is fixed](benchmarks/2026-10-03/pending-registration-idle-eviction/README.md): the selector now skips uncertain guests and can pause an eligible idle guest to admit a replacement. A controlled baseline fails while the fixed daemon passes two 34-check KVM profiles and 58 ordinary tests, preserving pending guests and subsequent recovery. The final fixed profiles verify full cleanup and unchanged input hashes; this is functional availability evidence, not a competitor timing result.

[Matched private/standard receiving TCP costs](benchmarks/2026-10-04/private-transport-comparison/README.md) pass 30 functional checks and 128/128 scored operations on the same KVM target. Private median setup costs an additional 1.54 ms at 64 B and 1.82 ms at one MiB; one-MiB payload echo throughput is approximately equal at 10.35 MiB/s. This measures two HyperMachine receiving paths at concurrency one, excluding the source guest gateway. It adds no competitor ranking or across-the-board performance claim.

The [unchanged-input transport repeat](benchmarks/2026-10-04/private-transport-repeat/README.md) independently passes another 30 checks and 128/128 scored operations. Private setup P50 remains higher by 1.14 ms at 64 B and 1.61 ms at one MiB; echo throughput remains close. Both cohorts support investigating setup overhead without attributing it to a component or claiming a competitor win.

[Concurrent receiving authorization lookups](benchmarks/2026-10-04/private-transport-parallel/README.md) retain both fresh setup checks while overlapping independent membership/node reads. All 64 ordinary daemon tests and two 30-check KVM candidate cohorts pass, including 256/256 scored operations and full cleanup. In matched dev-profile cohorts the private-minus-standard setup P50 gap decreases from 1.14–1.54 to 0.93–1.04 ms at 64 B, and from 1.61–1.82 to 1.16–1.28 ms at one MiB. Separate shared-host cohorts limit attribution; private setup remains slower. An initial release candidate cohort is retained but excluded from before/after timing comparisons. The archived baseline was a dev build, correcting earlier release wording. No competitor win is established.

[Benchmark failure evidence preservation](benchmarks/2026-10-04/private-transport-journal/README.md) now journals completed operations and saves the summary before scored-failure assertions. A 30-check normal KVM run passes 128/128 scored operations; an isolated injected failure retains all 136 rows, reports 127/128 scored success, emits no success report and reaps all 23 tracked children. This strengthens reproducibility without adding a performance claim.

[Actual source guest membership stream revocation](benchmarks/2026-10-04/private-source-stream-revocation/README.md) passes 31 owned two-daemon KVM checks. A distinct-marker established source guest TCP stream closes after source membership removal; numeric reconnect refuses while removed, and source rejoin preserves old-address refusal while fresh DNS restores exact binary traffic. All guests/processes are cleaned up. This is one functional observation on a shared host, without an SLA or competitor timing claim; active local VM/pending/owner changes, half-close, saturation, UDP and crash recovery remain unfinished or unverified.

[Interleaved dev-profile baseline/candidate comparison](benchmarks/2026-10-04/private-transport-abba/README.md) verifies 512/512 scored operations across ABBA cohorts, with 32 functional checks and complete cleanup each. Median paired private-minus-standard setup overhead is 1.26–1.29 versus 0.91–0.93 ms at 64 B, and 1.51–1.53 versus 1.18–1.28 ms at one MiB. Both candidate cohorts improve this local metric; private setup remains slower, and release, high-concurrency, guest-origin and competitor comparisons remain unmeasured.

[Matching release-mode ABBA transport comparison](benchmarks/2026-10-04/private-transport-release-abba/README.md) also passes 512/512 scored operations and 32 KVM checks per cohort, with complete cleanup. Median paired setup overhead falls from baseline 0.44–0.64 to candidate 0.29–0.34 ms at 64 B, and from 0.64–0.66 to 0.38–0.44 ms at one MiB. Source catalogs differ only in lookup ordering, and both builds use release mode. This verifies a local setup improvement; private remains slower than standard, and high-concurrency, guest-origin and competitor results remain unmeasured.

[Private receiving UDP](benchmarks/2026-10-04/private-udp-receiving-kvm/README.md) passes 64 ordinary daemon tests and 34 owned KVM checks, including empty/binary/65,507-byte exact datagrams, framing/context refusal and source-generation stream closure. All guests/processes are cleaned up. Source guest UDP interception, connector/router integration and full lifecycle parity remain unfinished; this adds no competitor win.

[Private UDP source connector](benchmarks/2026-10-04/private-udp-source-connector/README.md) adds generation-bound mTLS transport with shared TCP lifecycle checks. The full owned Redis cluster suite passes 119 tests, including framed UDP bytes, protocol/authentication/setup race refusal and no-contact stale bindings. Source guest UDP gateway/router integration and real connector-to-KVM delivery remain pending; this does not complete private UDP parity.

[Committed-binding private UDP source router](benchmarks/2026-10-04/private-udp-source-router/README.md) now dispatches UDP through the generation-bound connector with the same committed address, source lease and authorization gates as TCP. The full owned Redis suite passes 120 tests, including DNS-to-UDP mTLS framed transport and Memory/Redis stale UDP binding refusal. Source guest Ethernet/session integration and actual router-to-KVM UDP delivery remain unfinished or unverified.

[Private UDP gateway session foundation](benchmarks/2026-10-04/private-udp-gateway-session/README.md) adds bounded framing/idle/lifetime/cancellation handling and a daemon UDP hook to the fixed-source router. All 197 network and 64 daemon tests pass. Non-DNS UDP packets are still dropped until Ethernet socket/session admission and reply emission are integrated; guest-origin KVM UDP remains unverified.

[Private IPv4 UDP Ethernet gateway](benchmarks/2026-10-04/private-udp-ethernet-gateway/README.md) now routes claimed private destinations through bounded per-peer sessions and returns datagrams with the original private endpoint. All 200 network and 64 daemon tests pass, including real smoltcp guest empty/binary/1,280-byte packets, no fallback, shared TCP/UDP admission and gateway teardown. Actual cluster/KVM guest UDP delivery and lifecycle revocation remain unverified; IPv4 fragmentation and private IPv6 remain unfinished.

[Actual source guest UDP](benchmarks/2026-10-04/private-udp-source-guest-kvm/README.md) passes 37 KVM checks through production routing on two owned daemon nodes with complete cleanup and an identical fixture image rebuild. This supersedes the earlier guest-UDP integration-unverified notes for within-MTU IPv4 datagrams. Full UDP lifecycle, fragmentation, IPv6, independent hosts and competitor performance remain unverified or incomplete.

[Private IPv4 fragmentation/reassembly](benchmarks/2026-10-04/private-udp-ipv4-fragmentation/README.md) is implemented with bounded buffer counts/size and expiry. All 201 network/64 daemon tests pass; maximum UDP payloads preserve bytes across multiple valid-size Ethernet fragments in both directions, including delayed first-fragment delivery. Maximum-size KVM and reassembly exhaustion/expiry schedules remain unverified, and this adds no performance or competitor win.

[Maximum-size actual source guest UDP](benchmarks/2026-10-04/private-udp-max-source-guest-kvm/README.md) passes 37 KVM checks on two owned daemon nodes sharing one host, including exact 65,507-byte guest UDP, owner/stale-binding refusal and DNS recovery, complete cleanup and a byte-identical image rebuild. This supersedes prior maximum-size KVM-unverified notes. IPv6, full UDP lifecycle, reassembly pressure/reorder/expiry, independent hosts and competitor performance remain incomplete or unverified.

[Active guest UDP membership coverage](benchmarks/2026-10-04/private-udp-active-membership-kvm/README.md) passes 41 two-daemon KVM checks on one owned host, including established same-socket refusal after source/target membership removal, stale-address refusal and fresh-DNS maximum-payload recovery. A negative control rejects replies continuing beyond the grace window; all guests/processes are cleaned up and the image rebuilds identically. Full UDP lifecycle, IPv6, independent-host and competitor performance evidence remains incomplete.

[Active guest UDP target pause/resume](benchmarks/2026-10-04/private-udp-target-pause-kvm/README.md) passes 43 two-daemon KVM checks on one owned host: established same-socket target-pause refusal, paused numeric/DNS refusal and exact maximum-size resume recovery with preserved binding. Updated negative control, byte-identical image rebuild and complete cleanup pass. Other full UDP lifecycle, IPv6, independent-host and competitor performance evidence remains incomplete.

[Active guest UDP lookup-outage coverage](benchmarks/2026-10-04/private-udp-store-outage-kvm/README.md) passes 45 two-daemon KVM checks on one owned host: established UDP, new numeric access and DNS fail closed during verified Redis GET refusal; restored lookups recover maximum-size delivery. Independent owned mTLS node exec provides observation and full cleanup passes. Other outage/lifecycle modes, IPv6, independent hosts and competitor performance remain incomplete or unverified.

[Active guest UDP target deletion](benchmarks/2026-10-04/private-udp-target-delete-kvm/README.md) passes 47 two-daemon KVM checks on one owned host: dedicated target deletion stops established UDP, refuses numeric/DNS access, removes the record and preserves exact maximum-size delivery to another target. Earlier lifecycle gates repeat and complete cleanup passes. Active source UDP pause/delete, other lifecycle cases, IPv6, independent hosts and competitor performance remain incomplete or unverified.

[Active guest UDP source lifecycle](benchmarks/2026-10-04/private-udp-source-lifecycle-kvm/README.md) passes 50 two-daemon KVM checks on one owned host: source pause/delete removes exact identified target relay sockets before natural timeout, records reflect lifecycle state, and source resume restores exact maximum-size delivery. Before/after identities and complete cleanup are verified. This supersedes earlier source UDP pause/delete-unverified notes; other lifecycle races, IPv6, independent hosts and competitor performance remain incomplete or unverified.

[Private UDP reassembly bounds](benchmarks/2026-10-04/private-udp-reassembly-bounds/README.md) pass 204 networking tests: maximum-size reverse-order exact echo, configured two-buffer exhaustion without incomplete private admission, and expiry/refusal/recovery. Only test code changes. These are owned Ethernet/duplex-hook gates; actual KVM reassembly stress, adversarial overlap/corruption, IPv6, independent hosts and competitor performance remain unverified or incomplete.

[Release private versus standard UDP transport](benchmarks/2026-10-04/private-udp-release-comparison/README.md) now has two matched 51-check owned KVM cohorts with 256/256 scored operations. Fresh-TLS private setup has paired median overhead of about 0.29–0.34 ms; echo differences are smaller and vary.

| Payload | Private setup P50 (ms) | Standard setup P50 (ms) | Paired setup overhead P50 (ms) | Paired echo difference P50 (ms) |
|---|---:|---:|---:|---:|
| 64 B | 2.278–2.309 | 1.957–1.992 | 0.311–0.329 | 0.005–0.027 |
| 65,507 B | 2.192–2.193 | 1.830–1.886 | 0.294–0.337 | 0.007–0.025 |

These are separate shared-host cohort medians for internal HyperMachine transports. Source guest latency, sustained throughput, resources, independent hosts and competitor UDP performance remain unmeasured; private setup remains slower.

[Combined receiving private authorization snapshot](benchmarks/2026-10-04/private-route-live-snapshot/README.md) passes 120 cluster tests with owned Redis and 64 ordinary daemon tests. Redis now uses one atomic route/live-node read per fresh barrier, retaining both setup barriers and all authorization gates. This reduces command count; candidate release KVM and latency evidence remains outstanding, so no measured improvement or competitor win is claimed.

[Combined private authorization ABBA comparison](benchmarks/2026-10-04/private-route-live-snapshot-release-abba/README.md) passes four 51-check release KVM cohorts and 512/512 scored operations. Exact source snapshots, raw rows, independently recomputed statistics and cleanup are verified.

| Payload | Baseline paired setup overhead P50 (ms) | Candidate paired setup overhead P50 (ms) |
|---|---:|---:|
| 64 B | 0.267–0.286 | 0.282–0.323 |
| 65,507 B | 0.296–0.358 | 0.250–0.370 |

No consistent latency improvement is established; fewer commands do not prove a speedup. Candidate KVM lifecycle coverage is now verified, while private setup remains slower and broader performance/feature parity remains incomplete.

[Private/standard setup stage diagnostics](benchmarks/2026-10-04/private-setup-stage-profile/README.md) retain exact tagged per-request timings from a separate instrumented release daemon: 51 KVM checks, 128 scored operations and full cleanup pass.

| Payload / path | Fresh authorization total P50 (ms) | Guest-port open P50 (ms) | Loopback pair P50 (ms) | Server setup P50 (ms) |
|---|---:|---:|---:|---:|
| 64 B / private | 0.287 | 0.876 | 0.092 | 1.260 |
| 64 B / standard | 0.000 | 0.861 | 0.091 | 0.961 |
| 65,507 B / private | 0.280 | 0.760 | 0.091 | 1.123 |
| 65,507 B / standard | 0.000 | 0.743 | 0.092 | 0.839 |

Instrumentation is excluded from production benchmark claims. Guest-port opening and loopback-pair creation are sequential independent stages; overlap is a proposed next experiment with both fresh authorization barriers and cleanup preserved. No measured optimization or competitor win follows yet.

[Concurrent tunnel setup release ABBA](benchmarks/2026-10-04/private-setup-overlap-release-abba/README.md) passes 204 KVM checks and 512/512 scored operations with full cleanup. Both candidate setup medians are lower than both baseline medians in all four tested payload/path groups: private 64 B 2.273–2.290 vs 2.344–2.364 ms; private 65,507 B 2.121–2.151 vs 2.250–2.271 ms. The receiver overlaps guest-port opening with socket-pair creation while retaining both fresh authorization barriers and stream cleanup ownership. Tail/echo/total metrics and the private-standard gap do not improve uniformly. This supports local setup latency only; no competitor or throughput/resource win is claimed.

[Real private receiving capacity](benchmarks/2026-10-04/private-receiving-capacity-kvm/README.md) passes 53 KVM checks and 128 scored operations with full cleanup. The configured shared 128-slot budget refuses excess requests with 503 while 125 additional private UDP tunnels remain held alongside fixture routes; exact traffic survives on all held tunnels, an existing private route and a fresh standard route. One closed slot recovers on the first attempt and refilling restores 503. This proves bounded serialized admission and recovery in the owned fixture, not concurrent setup/worker throughput, mixed TCP capacity, resource or competitor superiority.

[Concurrent private receiving traffic](benchmarks/2026-10-04/private-capacity-concurrent-kvm/README.md) passes 54 KVM checks, 128 regression benchmark operations and 3,200/3,200 exact datagrams from 32 workers at receiving saturation. The same workload initially dropped at the owned echo fixture: 14 kernel receive-buffer errors match 14 echo-socket drops. An explicit requested 8 MiB fixture buffer (Linux actual 16 MiB) yields zero error/drop deltas and 21.98 MiB/s aggregate echoed payload throughput in one local cohort. The derived image changes only the echo binary; guest agent/client and production daemon are unchanged. This is fixture-corrected host-to-target transport evidence, not source-guest throughput, a production performance improvement, resource efficiency or competitor superiority.

[Matched concurrent private/standard UDP](benchmarks/2026-10-04/private-capacity-comparison-kvm/README.md) runs 32 workers in private-standard-standard-private order with both tunnel sets live throughout: 12,800/12,800 exact scored datagrams, zero guest UDP error/echo-socket drop deltas, 55 KVM checks and full cleanup pass. Private payload throughput is 21.771–21.790 MiB/s; standard is 22.069–22.209 MiB/s. This retains a small local steady-traffic gap, not throughput parity or a competitor win. Identical buffered guest fixture and workload are used; CPU/PSS attribution and source-guest/independent-host/managed comparisons remain unmeasured.

[Concurrent UDP resource repeat](benchmarks/2026-10-04/private-capacity-resource-kvm/README.md) independently verifies stable-process CPU/PSS snapshots for four matched 32-worker blocks, 12,800 exact scored datagrams, zero UDP drop/error deltas, 55 KVM checks and full cleanup. Throughput now overlaps: private 21.728–22.051 and standard 21.723–22.472 MiB/s; target CPU cost also overlaps (private 320.53–321.72 vs standard 319.34–331.82 ms/payload MiB). The earlier small throughput gap is not consistent across repeats. Whole target CPU dominates observed process costs and warrants thread profiling; whole-process PSS with both route sets held does not establish per-route memory efficiency or competitor superiority.

[Target thread diagnostics](benchmarks/2026-10-04/private-capacity-thread-kvm/README.md) verify 351 stable target threads in every matched concurrent block: Tokio-named workers use 19.48–20.60 CPU-seconds, vCPU threads 1.16–1.24. All 12,800 matched datagrams and 55 KVM checks pass with zero UDP drop/error deltas and full cleanup. Device-wide vsock progress wakes all stream waiters and is a concrete candidate for targeted wakeups; thread names do not prove causality. No production optimization or performance saving is claimed.

[Focused repeat-resume audit](benchmarks/2026-10-04/vsock-resume-cycles/README.md) reproduces a final resume timeout on the unchanged baseline after five added successful cycles; the targeted-wake candidate completes eight total resumes with exact UDP/full cleanup. The seven-vCPU-exit stall can occur without the wake change. Permitted VM code marks pause before any owner acknowledgement; that quiescence gap requires correction/testing, while timeout causality remains unproven. The targeted-wake CPU gain remains provisional because the original full ABBA lifecycle gates did not all pass.

Acknowledged-pause follow-up: the isolated candidate passed 47 selected core, 531 agent and 64 daemon tests, but the unchanged-deadline KVM audit timed out on the original main-target resume before additional cycles. Pause acknowledgement completed; the restored vCPU logged 6 exits before teardown. This does not validate a lifecycle fix or the provisional vsock CPU reduction. [Raw failure archive](benchmarks/2026-10-04/vsock-pause-owner-ack/README.md).

A separate readiness diagnostic reproduced the acknowledged-pause failure: vsock connection accepted in 9.69 ms, restore request unanswered for 15 seconds, clock fallback timed out, then HTTP 503. Owner sample showed a halted vCPU; interrupt/timer cause remains unproven. [Diagnostic evidence](benchmarks/2026-10-04/vsock-pause-owner-ack-readiness-failure/README.md).

Serialized MMIO IRQ follow-up: deterministic tests expose both status/line races under the old ordering and pass after serialization. The isolated release passes one original-deadline 20-resume cohort (21 KVM checks, exact UDP, prior closure and full cleanup). This does not establish elimination of the intermittent stall or accept the provisional CPU gain; independent repeats remain required. [Verified evidence](benchmarks/2026-10-04/mmio-irq-order/README.md).

Two additional independent original-deadline serialized-IRQ cohorts also pass: three cohorts total cover 60 main-target resumes, each with exact UDP/prior closure, 21 KVM checks and full cleanup. The transport change has red/green concurrent regression proof. Intermittent-stall elimination and CPU superiority remain unproven; earlier failures remain preserved. [Repeated-run evidence](benchmarks/2026-10-04/mmio-irq-order-resume-repeats/README.md).

Full combined-candidate release ABBA now passes 220 KVM checks, 512 scored setup/echo operations and 51,200 exact concurrent datagrams with zero UDP drops/errors and complete cleanup. Independently recomputed target CPU falls from 316.70 to 35.11 ms/MiB (88.91% lower); aggregate throughput rises 3.62%. Candidate measured P50/P95 setup/echo metrics are lower in these cohorts. This accepts a scoped improvement against the owned HyperMachine baseline, not a competitor win or permanent elimination of the lifecycle stall. [Raw comparison and limitations](benchmarks/2026-10-04/mmio-irq-order-release-abba/README.md).

PCI transport follow-up: three deterministic IRQ/ISR-read/reset tests fail under old ordering and pass with serialization, alongside 16 PCI, 47 selected core, 531 agent and 64 daemon regressions. Real Linux PCI-vsock remains unverified: the guest reaches userspace and starts its agent, but Linux reports no PCI configuration-space access function and the first ping times out. Configuration discovery is the next gap; no PCI runtime win is claimed. [Failure evidence](benchmarks/2026-10-04/pci-guest-discovery/README.md).

PCI configuration discovery follow-up: a minimal root host bridge allows Linux to select native type-1 access and enumerate virtio-vsock without a PCI kernel override. 84 PCI and 47 selected core regressions pass. Actual operation still fails because the guest cannot find the PCI INT A route and its vsock probe fails; source changes remain unstaged. [Verified discovery and routing evidence](benchmarks/2026-10-04/pci-host-bridge/README.md).


PCI guest functional acceptance (2026-10-04): the combined host bridge, bus-zero INTx routing, IRQ ordering, and initial queue-capacity corrections pass three fresh owned KVM guests: 192 pings and 192 exact commands, all followed by clean shutdown. The new queue-discovery regression fails against the prior implementation; 106 PCI-filtered library tests, 47 VM tests (2 ignored), and 531 agent tests pass. This supersedes the earlier PCI discovery/routing failures for this fixture only. Broad kernel compatibility, MSI-X, hotplug, migration and competitor performance remain unverified. [Raw evidence and candidate sources](benchmarks/2026-10-04/pci-queue-discovery/README.md).

Host MMIO reset correctness: reset now releases outstanding level interrupts under the status lock. A regression fails against the prior implementation; 17 MMIO and 47 VM tests pass (2 ignored), plus a real KVM/MMIO guest command smoke check. The smoke does not directly test host reset. This establishes no new performance win. [Evidence](benchmarks/2026-10-04/mmio-host-reset/README.md).

PCI checkpoint/restore gap (2026-10-04): cold PCI guest acceptance does not extend to snapshots. A working template captures a checkpoint, but a fresh restored PCI guest times out on its first 15-second ping; VM code explicitly omits PCI transport state. Both guests and snapshot outputs are cleaned up. Default MMIO lifecycle evidence remains separately scoped; PCI lifecycle parity is incomplete. [Reproducible gate and raw failure](benchmarks/2026-10-04/pci-snapshot-failure/README.md).

PCI checkpoint acceptance (2026-10-04) supersedes the preceding fixture failure: three fresh owned checkpoint restores pass 48 restored pings and 96 exact commands, including saved guest data, with full cleanup. PCI registers, queues/cursors/counters, bus-zero standard-function configuration and CONFIG_ADDRESS are captured and validated; PCI snapshots use version 3, MMIO-only snapshots retain version 2. Five existing MMIO restores also pass timer/clock/arithmetic/RNG health checks after correcting their proven IRQ-0-only timer parser. 110 PCI, 14 snapshot-file, 47 VM (2 ignored), 531 agent and 64 daemon (2 ignored) regressions pass. This is functional parity for the owned fixture; other kernels/backends, arbitrary PCI topologies and managed migration remain unverified, with no performance claim. [Raw evidence and verification](benchmarks/2026-10-04/pci-snapshot-state/README.md).

PCI sibling/descendant lifecycle verification: three fresh direct AgentVM/VM runs restore nine guests, pass 174 pings and 378 exact commands, and preserve distinct sibling markers across 30 explicit pause/resume pairs and a second-generation checkpoint. All guests and snapshot outputs are cleaned up; the unchanged default restore profile passes on the same frozen executable. Production sources match the preceding accepted PCI snapshot candidate. This tests direct checkpoint clones, not the sandbox API fork route, managed migration or competitor performance. [Gate and raw evidence](benchmarks/2026-10-04/pci-sibling-descendant/README.md).

PCI sandbox API integration: `hv2-sandboxd --guest-transport pci` selects PCI vsock consistently in the shared template/create/restore builder, with distinct template cache identities. Selected PCI and unchanged default MMIO each pass 27 authenticated owned API checks, including actual two-child/descendant fork, exact write isolation, three disk pause/resume cycles, four deletions and empty inventory. 66 daemon tests pass (2 ignored). Earlier harness failures and recovery/deletion of their paused children are retained. Network-enabled PCI nodes, mixed-transport migration/scheduling and managed competitor performance remain unverified. [Operator guide](GUEST_TRANSPORT.md) and [API evidence](benchmarks/2026-10-04/pci-daemon-api/README.md).

Network-enabled PCI sandbox acceptance: two fresh authenticated API profiles pass 56 checks each with PCI vsock plus the MMIO NIC. Fourteen total NIC HTTP requests reach an owned exact-/32 host fixture with unique exact responses and matching receipt, while 28 separate guest-proxy HTTP requests preserve markers across forks and disk resumes. Each profile deletes four sandboxes to an empty inventory. Proxy forwarding uses vsock and is not counted as NIC delivery. No production change or performance win is claimed; mixed-transport migration, other kernels and broader network/policy gates remain unverified. [NIC and proxy evidence](benchmarks/2026-10-04/pci-network-api/README.md).

Matched release transport performance (prepared create, C1): one frozen daemon passes MMIO–PCI–PCI–MMIO with 72 exact/create/delete gates and 64 scored operations. Pooled API-create P50/P95 is 22.50/31.40 ms for MMIO versus 19.87/24.31 ms for PCI; mean sampled process CPU is 36.88 versus 31.56 ms per create+command, while held daemon PSS P50 is 75,337 versus 76,685 KiB. PCI is faster in pooled observations but uses more held PSS; cohort tails overlap. Same owned WSL/KVM host, guest, fixed eight-CPU affinity; configured boot arguments differ. This is an internal mode comparison, not cold boot, throughput, high concurrency or a managed competitor win. [Raw ABBA samples and independent verification](benchmarks/2026-10-04/pci-mmio-release-abba/README.md).

Matched cold release transport comparison: --no-template MMIO–PCI–PCI–MMIO passes 72 exact/create/delete gates and 64 scored samples, with 18 Linux loads and zero restores per cohort. MMIO/PCI API-create P50 is 425.68/753.93 ms and P95 2114.16/1346.20 ms; sampled CPU 978.75/570.63 ms and immediately held PSS P50 193.63/163.72 MiB. PCI has slower median cold creation despite faster prepared creation in the separate gate. Host caches are warm, boot arguments differ and these short same-host samples establish no overall or competitor win. [Raw cold samples and verification](benchmarks/2026-10-04/pci-mmio-cold-release-abba/README.md).

Current frozen release versus Firecracker C1 refresh: both engines pass 8/8 matched cold-create-to-exact-command attempts with the same immutable output-drain guest, kernel and eight-CPU affinity. HM/FC readiness P50 is 477.35/445.64 ms; P95/P99 1315.94/1495.04 ms; five-second held PSS medians 151.98/85.88 MiB and incremental PSS 84.92/85.88 MiB. HM retains more process memory and has higher median latency; its lower observed maximum does not establish tail superiority with eight samples. Higher-concurrency October 3 results remain scoped to their older binary. [Fresh matched-engine evidence](benchmarks/2026-10-04/current-release-firecracker-c1/README.md).

Allocator exploration, current release C1: with MALLOC_ARENA_MAX=2 for the owned HM daemon only, both engines pass 8/8 attempts and clean up. HM/FC five-second held PSS medians are 123.81/85.85 MiB; readiness P50 441.27/470.32 ms and P95/P99 3346.86/1740.96 ms. HM holds less PSS than the separate default-allocator run (151.98 MiB), but a sequential run with observed unrelated host compilation does not isolate causality; maximum latency worsens. No allocator default change or overall win is accepted. Interleaved allocator trials, concurrency and throughput gates remain necessary. [Raw exploratory evidence and verification](benchmarks/2026-10-04/current-release-arena2-c1/README.md).

Allocator C1 ABBA follow-up: four fresh equal-length cohorts (default/2/2/default, four pairs each) pass all 32 exact engine attempts and cleanup. Default/limited HM held PSS is 114.42/121.47 MiB, readiness P50 423.26/461.67 ms and P95/P99 574.70/1395.42 ms. The earlier sequential apparent memory benefit does not repeat; no allocator default change is accepted. Per-pair empty/held/post-cleanup measurements expose variable retained memory, but do not establish its cause or longer-run density. [Interleaved evidence and independent verification](benchmarks/2026-10-04/allocator-c1-abba/README.md).

Fixed mmap-threshold C1 ABBA candidate: default/1MiB/1MiB/default fresh cohorts pass all 32 exact engine attempts. Default/fixed HM held PSS medians are 130.06/93.88 MiB, repeated in both candidate cohorts; empty baseline medians 31.84/10.35 MiB. Readiness P50 is 400.98/396.24 ms and P95/P99 489.57/778.21 ms. Lower memory repeats for this fixture but the maximum worsens; product defaults remain unchanged pending longer lifecycle, concurrency, prepared restore and network CPU/throughput gates. FC still has lower memory and median readiness. [Raw evidence, glibc rationale and independent verification](benchmarks/2026-10-04/mmap-threshold-c1-abba/README.md).

Current-release C8 mmap-threshold ABBA: default/fixed/fixed/default fresh cohorts pass all 256 exact engine attempts (64 per engine per setting) and cleanup. Default/fixed HM held PSS medians are 888.01/680.16 MiB for eight guests; empty baseline medians 200.74/14.80 MiB. Lower memory repeats in both fixed cohorts, but readiness P50 worsens 487.07 to 547.33 ms and P95 529.44 to 892.35 ms. FC held PSS remains about 671 MiB and median readiness about 428-437 ms; its tails vary. This proves a fixture-specific memory/latency tradeoff, not an overall optimization; defaults remain unchanged. [Current C8 raw evidence and independent verification](benchmarks/2026-10-04/mmap-threshold-c8-abba/README.md).

PCI cold-start improvement: the one-UART/headless keyboard boot bundle reduces matched PCI create P50/P95 from 654.58/765.21 to 378.10/428.37 ms in baseline/candidate/candidate/baseline, with all 72 exact lifecycle gates passing. Sampled CPU is 417.50/390.31 ms per operation; held PSS slightly increases. An explicit PCI template discriminator prevents identical boot arguments from sharing MMIO cache identity; MMIO identity bytes remain stable. Full daemon tests pass 67 (2 ignored); prepared PCI/MMIO APIs pass 28 checks each and network-enabled PCI passes 56. Default remains MMIO; broader kernel, concurrency and managed competitor performance are unverified. [Sources, raw evidence and independent verification](benchmarks/2026-10-04/pci-fastboot/README.md).

PCI boot-change store adoption: old-release-to-candidate PCI and default-MMIO profiles pass 21 checks each on the same owned host/store path. Saved guest memory marker, sandbox ID/access token, fork isolation and two further disk resume cycles per profile survive. PCI keeps its old base alongside the new key; MMIO actually reuses its existing template (zero candidate Linux loads). Both profiles delete two guests, empty inventory, stop both daemon phases normally and remove their owned stores. This closes this exact old-store adoption gate only; general version/host/path migration, network/volume stores and crash recovery remain unverified. [Raw upgrade evidence and verification](benchmarks/2026-10-04/pci-fastboot-store-upgrade/README.md).

Current PCI release versus Firecracker: all 144 matched C1/C8 cold attempts pass. HM/FC readiness P50 is 413.50/381.93 ms at C1 and 520.63/448.77 ms at C8; C8 P95 is 620.84/521.00 ms. Five-second held PSS medians are 145.35/85.87 MiB at C1 and 860.56/670.87 MiB at C8. Actual accepted PCI boot-argument tokens match the FC configuration; both use the same immutable kernel/output-drain guest and eight-CPU affinity, with no allocator experiment or admission budget. Improved PCI still has higher median readiness and held memory; no competitive superiority is established. [Current raw C1/C8 comparison and verification](benchmarks/2026-10-04/current-pci-firecracker/README.md).

PCI C8 allocator investigation: glibc/jemalloc/jemalloc/glibc passes all 256 exact engine attempts with a frozen local jemalloc 5.3.0 library and child-only background-purge/one-second-decay settings. HM held PSS is 813.29/688.80 MiB pooled, and both candidate cohorts hold less than either glibc cohort. Latency varies substantially by time window; candidate pooled P50 is worse (1233.41 vs 721.82 ms), while FC reference timings also shift. Defaults remain unchanged; no speed, throughput or overall win is accepted. [Raw combined-allocator memory candidate and independent verification](benchmarks/2026-10-04/jemalloc-pci-c8-abba/README.md).

Durable sharing prerequisite: proxy requests now await an admission hook before backend open/guest wakeup, with default delegation preserving synchronous policies. A delayed-denial HTTP/gRPC regression proves pending and denied admission resolve no backend; all 1070 API library and 31 control-plane integration tests pass, including legacy private web policy checks. No KVM/performance or self-service sharing completion is claimed. Owner-bound Memory/Redis grants, management API/CLI, fail-closed store-backed admission and persistence/real-guest gates remain open. [Compatible source extension and test evidence](benchmarks/2026-10-04/async-proxy-admission/README.md).

Browser sharing model prerequisite: five isolated tests pass for strict owner-bound grants, exclusive expiry, canonical revisions, duplicate rejection, revocation records and exact sandbox-incarnation checks. Node movement is identity-neutral in the model only. Storage CAS, owner management, proxy enforcement and restart/real-guest gates remain unimplemented; no self-service feature or performance win is claimed. [Model and test evidence](benchmarks/2026-10-04/web-sharing-model/README.md).

Browser sharing storage prerequisite: owner-checked Memory/Redis reads, atomic record/grant snapshots and byte-fenced revision CAS pass seven sharing tests against a fresh owned Redis, followed by 127 cluster library tests (one ignored; ACL fixture test skipped). Exact replay succeeds; stale requests cannot revive revoked grants, and deleted/recreated records deny admission. Sharing rows deliberately retain revisions after deletion. Active grants are visible across connections; disk persistence was disabled, so restart/crash durability is unverified. Owner management and proxy enforcement remain open, with no feature-completion or performance claim. [Store sources and raw tests](benchmarks/2026-10-04/web-sharing-store/README.md).

Owner sharing API prerequisite: owner-only GET/PUT grant replacement supports exact revision retries and empty-list revocation with strict 64 KiB JSON and five-second store-call bounds. Memory and owned Redis HTTP contracts verify key permissions, rotated credentials, single-winner concurrency, stale replay refusal and deletion. The full cluster library passes 129 tests (one ignored; ACL fixture skipped), followed by ten final sharing tests including eight fault modes, four cancelled calls and two exact retry recoveries after committed errors/timeouts. Browser enforcement, CLI and restart/real-guest gates remain open; no self-service completion or performance claim is made. [API sources and fault evidence](benchmarks/2026-10-04/web-sharing-api/README.md).

Owner sharing proxy and CLI: explicitly delegable browser credentials use atomic store-backed grants after existing operator scopes, with five-second fail-closed reads and post-await credential rotation/expiry/delegation revalidation. The cluster library passes 132 tests (one ignored; ACL fixture skipped), all 31 control-plane integrations pass including HTTP/1+HTTP/2 TLS grant admission, expiry/revocation, credential stripping and no-open denials, and two CLI tests verify bounded payloads, exact file retries and credential-safe errors. The CLI binary also compiles; its initial targeted run selected zero tests and is not counted as test evidence. Owner grant management and proxy enforcement are verified in local fixtures; Redis restart/crash, real KVM owner sharing, SSO and verified identities remain open. No performance win is claimed. [Sources and scoped verification](benchmarks/2026-10-04/web-sharing-proxy/README.md).

Owner sharing AOF restart gate: one explicitly invoked owned Redis 8.0.2 test passes five hard process restarts with appendonly=yes/appendfsync=always. Six distinct processes and five AOF reloads preserve active grants, exact u64 incarnation, revocation, deletion, replacement and owner-change semantics through async admission. Corrupt JSON and wrong key types deny without mutation. The first run’s old-connection broken pipe is retained; corrected bounded read-only reconnection passes, with no production change. Independent verification confirms test-only source scope, raw hashes and process/directory cleanup. Hardware power loss, managed failover, shipped control-plane restart and real KVM owner sharing remain open; no performance or overall superiority claim is made. [Sources, initial failure and final verification](benchmarks/2026-10-04/web-sharing-aof-restart/README.md).

Real KVM owner sharing (2026-10-05): ten cases pass through owner API/CLI, verified API/proxy TLS, node mTLS and a real local guest plus fork. Exact CLI retries, guest identity/credential stripping, custom domains, fork exclusion, revoked paused-guest no-wake, current regrant auto-resume, delegation reload and expiry are verified. Two shipped control-plane restarts preserve active and revoked state while Redis stays available. Both guests are deleted to empty inventory, all five registered processes stop, private fixture files are removed and input hashes remain unchanged. The initial CA key-usage verification failure and corrected strict-TLS driver are retained. Current isolated control/CLI release builds have accepted protected hash guards and eleven matching selected source/dependency hashes, not a full source closure. Live Redis outage, power loss, managed failover, SSO and tenant/team isolation remain open; no performance or overall competitive win is claimed. [Release context, initial failure and final evidence](benchmarks/2026-10-05/owner-sharing-kvm/README.md).

Live Redis outage owner-sharing gate (2026-10-05): thirteen real KVM cases pass with unchanged frozen binaries, repeating ten owner API/CLI/TLS cases plus three outage/recovery cases. During an owned Redis hard outage, two browser requests deny and independent node mTLS state remains paused; closed-connection and five-second-timeout paths meet the functional deadline. Two AOF-always reloads preserve active and revoked revisions, stale replay returns 409, and current regrant permits real auto-resume. Both guests are deleted to empty inventory, seven processes stop and private fixture files are removed. Redis 8.0.2 logs show three processes and two AOF reloads; hashes and cleanup are independently verified. Fleet-scale outages, managed failover, power loss, SSO and tenant/team isolation remain open. Failure-path timings are not performance scores or competitor wins. [Driver and raw outage evidence](benchmarks/2026-10-05/owner-sharing-redis-outage/README.md).


### 2026-10-05: creator-bound sandbox API release gate

Non-administrator scoped keys with a configured principal now receive owner-filtered inventories and cannot dispatch sandbox-ID operations against another owner or ownerless records. Administrator and unassigned-key compatibility is preserved; operator policies with admin scope remain administrators. Global resources and already admitted operations remain outside this boundary.

The isolated locked release build passed protected-source hash guards. The [owned KVM report](benchmarks/2026-10-05/creator-bound-api-kvm/report.json) passed all 14 functional cases, repeating grant/revocation, TLS guest identity, control-plane restart, and live Redis outage gates alongside creator API admission. The archive retains the initial execute-permission failure, raw logs, source and release hashes, independent cleanup checks, and a manifest verifier. All seven processes stopped; administrator inventory was empty; private fixture files were removed. The HTTP integration suite passed 32 tests. No new performance measurement or hosted competitor result is claimed.


### 2026-10-05: current creator policy and filtered pagination

The [expanded HTTP integration evidence](benchmarks/2026-10-05/creator-policy-pagination/integration.txt) passes all 32 tests. The ownership fixture now verifies old-key revocation, replacement-key access, denial after a stored owner changes, and updated principal assignment on the same key. Interleaved records from two owners paginate without duplicates or foreign records in ascending and descending order; a foreign cursor restarts within the filtered inventory. These checks use an owned MemoryStore fixture, not a managed multi-tenant deployment. No production behavior changed in this follow-up.
