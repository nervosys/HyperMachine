# Native TCP/UDP gateway

`hv2-native-gateway` runs the native port supervisor against the cluster's Redis reservations. It opens TCP, UDP or both on each eligible reservation's stable public port and relays to the current sandbox node through mutual TLS. It consumes authorized reservations created through the opt-in owner-only control API described below.

Configure `HV2_STORE_URL` and `HV2_CLUSTER_TOKEN` in the process environment. The store URL accepts Redis, Redis TLS or an owned Unix socket. Redis TLS uses the existing `HV2_STORE_CA`, `HV2_STORE_CERT` and `HV2_STORE_KEY` settings. The cluster token must match the node configuration. The gateway requires the operator's CA, client certificate and private key; node certificates are checked against `hv2-node` or `--mtls-node-name`.

```sh
hv2-native-gateway --bind-ip 127.0.0.1 --namespace default \
  --mtls-ca /etc/hypermachine/ca.pem \
  --mtls-cert /etc/hypermachine/gateway.pem \
  --mtls-key /etc/hypermachine/gateway.key \
  --max-ports 256 --max-sessions 128 --tcp-connections 64 --udp-peers 64
```

`--bind-ip` is mandatory and chooses the exact native listener interface. An operator chooses an externally reachable interface for public ingress and supplies its DNS/firewall routing. Upstream node API addresses must be HTTPS origins. Redirects are refused; the gateway sends its configured cluster token and never an ingress API key. Native listener IPv4/IPv6 family is independent of the guest destination: current UDP node negotiation selects the IPv4 guest relay.

Add `--check` to validate arguments, store URL syntax, node TLS identity and node credential format without contacting the store or opening native listeners. It does not verify store reachability, store TLS environment files or node connectivity. Startup bounds store connection to five seconds. SIGINT and SIGTERM stop the Unix gateway and await owned relay cleanup; other platforms use Ctrl-C.

Defaults are 256 total reservations, 128 aggregate sessions, 64 TCP connections or UDP peers per listener, 250 ms polling, a one-hour absolute TCP connection lifetime and 30 seconds of UDP outbound inactivity. `--max-ports` applies to the complete namespace reservation inventory before lifecycle filtering, so it must cover paused reservations too. `--max-sessions` is shared across all TCP connections and UDP peer sessions. Each UDP peer queues at most eight framed datagrams. These are count bounds, not byte/RSS or fleet quotas.

The gateway preserves unchanged listeners and stops changed/removed listeners and their sessions before rebinding. Failed ports are retried. Pause or node loss releases listeners while retaining durable reservations; resume can rebind the same port. Invalid snapshots or store failure close listeners. Changes take effect when polling observes them, not through an atomic management-API acknowledgement barrier. Removing a reservation or sandbox must update authoritative store state; the gateway never deletes those records itself.

An [owned executable fixture](benchmarks/2026-10-03/native-gateway-redis-outage/README.md) passes thirteen checks for native TCP/UDP through Redis and mutual TLS, protocol changes, pause/resume, corrupted snapshot recovery, signals, gateway restart and recovery after an owned Redis AOF restart without restarting the gateway. TCP clients reconnect after outage closure; UDP clients establish new peer sessions when delivery resumes. It seeds an owned fixture's records directly and does not establish owner-management API behavior. [Real KVM delivery](benchmarks/2026-10-03/native-gateway-kvm/README.md) passes fourteen checks each for IPv4/two-peer and IPv6/eight-peer ingress, including exact native TCP/UDP payloads, protocol updates, guest pause/resume and deletion cleanup. Those earlier runs do not establish owner-management API behavior; the API evidence below adds that verification. Guest reboot, Internet operation, sustained resource measurements and competitor performance remain unverified.

A [matched internal UDP comparison](benchmarks/2026-10-03/native-cli-abba/README.md) runs CLI/native/native/CLI blocks against the same KVM guest port with both processes live. Native completion rate is 25.4% higher with two IPv4 peers and 12.6% higher with eight IPv6 peers; mean peer sample medians are 20.5% and 11.6% lower. These short unpinned development-build runs compare distinct forwarding paths and provide no competitor performance result. Raw per-peer samples and input hashes are archived.

The subsequent [optimized release comparison](benchmarks/2026-10-03/native-cli-release-abba/README.md) records one vCPU and 1,024 MiB per guest and uses frozen release binaries. Native completion rate is 23.8% higher with two IPv4 peers and 13.0% higher with eight IPv6 peers; mean peer medians are 19.3% and 12.6% lower. One eight-peer P99 block ranking reverses, so an improvement across every metric is not established. The report records the operator-declared build profile, guest inventory resources and host environment. These remain internal short-run measurements, with competitor performance unmeasured.

[Trusted creator attribution](benchmarks/2026-10-03/sandbox-owner-context/README.md) now supplies a durable VM owner for newly created sandboxes whose scoped policy configures `principal_id`. It is assigned by authenticated control-to-node creation, preserved through fork/pause/resume, and cannot be chosen through public owner metadata or headers. Legacy VMs remain ownerless. Owner-only reservation/list/removal APIs are now implemented and verified below; explicit legacy adoption and ownership transfer remain incomplete. See [API key policy guidance](API_KEY_ROTATION.md).

Start the control plane with `--native-port-range 40000-40031` to enable owner-only public-port management. The window must contain 1–4,096 nonzero ports; all replicas should use the same operator allocation policy. Configure `principal_id` on scoped credentials and create the VM through authenticated clustered creation. Legacy admin/unlabelled credentials and ownerless legacy VMs cannot use these APIs. The gateway must share the namespace and have the configured ports available on its bind address. Public DNS/firewall/NAT routing remains operator-managed.

| Request | Body | Result |
|---|---|---|
| `PUT /sandboxes/{id}/public-ports/{machinePort}` | `{}` for TCP; `{"protocol":"both"}` for TCP and UDP; `"udp"` is also accepted | 202 with `machinePort`, `publicPort`, `protocol`; replay/protocol update keeps the port |
| `GET /sandboxes/{id}/public-ports` | none | 200 with the owner's reservation array |
| `DELETE /sandboxes/{id}/public-ports/{machinePort}` | none | 204 when removed; 404 when absent |

Each request needs a scoped `X-API-Key` whose configured principal matches the stored VM creator and whose role/scopes permit sandbox operations. The control API checks VM ownership atomically with reservation access in Memory/Redis. Wrong, observer, inventory-only and unassigned principals are refused. Client owner headers/body labels cannot choose ownership. Responses contain no owner ID. PUT rejects unknown body fields and invalid protocol values. There are at most sixteen reservations per VM, verified over HTTP and both stores. Full pools return 503; the VM cap returns 409. Deleted ports can be recycled.

A 202 response confirms the reservation is stored; it does not guarantee the public listener is already bound. Removal is also observed asynchronously by the gateway, so 204 is not a synchronous connection-revocation barrier. Store operations time out after five seconds; a mutation may already have committed, so retry the same destination/protocol request. Existing team-wide VM lifecycle/execution scopes remain unchanged. Legacy ownership adoption, transfer and organization identity provisioning remain incomplete. Native CLI commands are documented below.

[Owner API verification](benchmarks/2026-10-03/owner-public-port-api/README.md) passes 86 library tests, 30 HTTP tests and the explicitly owned Redis/AOF fixture. Two actual KVM profiles pass twenty checks each for IPv4/two-peer and IPv6/eight-peer ingress using real API reservations/protocol updates. They verify first-exposure authorization, key rotation, removal of live native TCP, TCP/UDP reexposure, fork allocation exclusion and pause/resume/delete preservation/cleanup. These profiles publish one reservation per VM; sixteen simultaneous public listeners are not established by them. They leave zero guests and reap all owned processes. No competitor performance claim follows.

The shipped CLI now supports native management with the same configured principal/API credentials. Set `HV2_SANDBOX_URL` and `HV2_API_KEY` through your environment; use `--api-ca-cert PATH` when the API requires an additional trust root. Create the VM with that credential and use its returned `sandboxID`:

```sh
hm sandbox vm create --template base
hm sandbox vm public-ports expose VM_ID --port 18082 --protocol both
hm sandbox vm public-ports list VM_ID
hm sandbox vm public-ports remove VM_ID --port 18082
```

Omitting `--protocol` selects TCP; `tcp`, `udp` and `both` are accepted. Ports must be 1–65,535. Expose/list print reservation JSON; successful removal prints nothing and exits zero. The commands use bounded 16 KiB responses and omit server error bodies from refusal messages. They do not accept owner override flags. Existing reservations keep their public port through protocol updates. Exposure acknowledgements retain the asynchronous publication semantics described above.

[CLI creation and management verification](benchmarks/2026-10-03/owner-public-port-cli/README.md) passes all 148 CLI library tests and twenty-two checks each through real Redis/mTLS/KVM with IPv4/two-peer and IPv6/eight-peer ingress. The managed VM is created by the shipped CLI V2 command, with trusted creator identity verified before exposure. The profiles verify owner/role refusal, key rotation, empty-output removal, default TCP, protocol updates and native TCP/UDP reexposure, as well as fork/pause/resume/delete handling. Logs exclude generated API/cluster credentials and the CLI-created workload token; zero guests remain and all owned processes are reaped. Guest reboot and public Internet operation remain unverified.

[Sixteen-port UDP capacity verification](benchmarks/2026-10-03/native-sixteen-port-capacity/README.md) passes twenty-six checks in each ingress profile. Sixteen distinct tagged guest UDP services each complete 100 concurrent round trips (1,600 per profile); a seventeenth reservation returns 409. Pause unbinds all sixteen UDP listeners, resume preserves allocations and restores delivery, and deletion clears them. This verifies owned loopback forwarding capacity with IPv4 and IPv6 ingress to IPv4 guest destinations. These UDP profiles do not establish sixteen simultaneous TCP services; the subsequent combined capacity profile below covers TCP. Competitor performance superiority remains unverified.

[Redis record preflight verification](benchmarks/2026-10-03/native-port-row-preflight/README.md) rejects mutually agreeing malformed reservation records before update or deletion. Four corruption cases leave both indexes unchanged through owner-authorized and privileged internal operations; 86 library tests and the explicit owned Redis/AOF fixture pass. Earlier KVM evidence retains its original binary/source context.

[Post-fix KVM verification](benchmarks/2026-10-03/native-port-preflight-kvm/README.md) rebuilds the node/control/gateway with that Redis preflight fix and passes all 26 capacity/lifecycle checks in each ingress profile, plus 30 control-plane HTTP tests. Earlier binary archives remain unchanged.

[Sixteen TCP/UDP publications](benchmarks/2026-10-03/native-sixteen-tcp-udp-capacity/README.md) pass 30 checks per ingress profile. Sixteen distinct both-protocol destinations verify 1,600 UDP exchanges followed by 1,600 TCP exchanges while all listeners and peers coexist. TCP connections carry destination tags; all sixteen TCP destinations recover after pause/resume and close on deletion. Aggregate session caps are 36/42 for IPv4/IPv6; these are functional capacity checks, not a mixed-traffic throughput benchmark. Guest reboot and public Internet operation remain unverified.

[Mixed TCP/UDP verification](benchmarks/2026-10-03/native-mixed-tcp-udp-capacity/README.md) adds 32 synchronized workers across those sixteen reservations. Each ingress profile passes 31 checks, including an additional 1,600 TCP and 1,600 UDP exchanges under concurrent traffic without retries. This establishes functional concurrency, not timed performance or public operation.

[Legacy adoption store foundation](benchmarks/2026-10-03/legacy-owner-adoption-store/README.md) verifies atomic competing-owner adoption, reservation refusal and owned Redis/AOF persistence with 87 library tests plus the explicit Redis fixture. That archive captures the store foundation before API/CLI and node lifecycle wiring; the verified workflow below supersedes those implementation gaps.

Administrator adoption is now available for existing ownerless VMs:

```sh
hm sandbox vm --endpoint "$HV2_SANDBOX_URL" --api-ca-cert /path/control-ca.pem adopt-owner sbx-ID --principal-id team.user-1
```

Use an administrator credential in `HV2_API_KEY`; the target label must match the intended user's configured stable principal. POST `/sandboxes/{id}/owner` accepts only `{"principalId":"team.user-1"}` and returns 204 with no body for adoption or same-owner replay. Operator/admin-scoped policies and configured legacy administrators may use it; observers, sandbox-only keys and anonymous mode cannot. Cluster authentication must be configured. Existing different owners and legacy port reservations return 409; this is adoption, with no ownership transfer. A store/metadata failure can return 503 after commitment: retry the same principal.

[Full adoption verification](benchmarks/2026-10-03/legacy-owner-adoption-workflow/README.md) passes 87 library, 31 HTTP, 48 node and 149 CLI tests plus the owned Redis/AOF fixture. Running and paused adoption coordinate node state and shared paused descriptions; cancellation releases the paused claim. Fork/resume resolve shared ownership and stale ownerless lifecycle writes retain it. Two final KVM ingress profiles pass 33 checks each on an actual CLI-created then adopted VM, including sixteen both-protocol ports and simultaneous mixed traffic. The two-node adoption/resume races are verified below; broader failover, crash recovery of claimed descriptions, guest reboot and public Internet operation remain unverified.

[Two-node adoption/migration verification](benchmarks/2026-10-03/legacy-owner-adoption-cross-node/README.md) passes 25 checks per ingress profile. A resume-first race refuses the competing adoption; a controlled held-adopter-first case refuses concurrent resume until the shared claim is returned. An actual guest migrates to the second KVM node with the assigned owner and verifies execution, 1 MiB TCP, UDP, fork inheritance and reservation deletion. This covers the two observed claim orderings, with one migrated reservation; managed failover, machine-crash claim recovery, guest reboot and public Internet operation remain unverified.

[Stable public-port migration](benchmarks/2026-10-03/native-public-port-migration/README.md) passes 27 checks per ingress profile. An already published both-protocol VM pauses on node two and resumes on node one with byte-identical allocation and unchanged owner listing. Pause closes TCP and unbinds UDP; the same gateway process republishes the same public port and verifies another 1 MiB TCP plus exact UDP payloads. This covers one reservation across real KVM nodes, not reboot persistence, sixteen-port migration or managed failover.

## Recovering uncertain guest registration

Clustered creation or resume can return 503 after the guest is already running locally. The daemon retains that guest and marks registration pending when record publication or lifecycle-event publication fails. Conflicting lifecycle mutations are refused until recovery succeeds. Fix the store outage or permission problem first; recreating the guest can leave an extra running instance.

Use a legacy administrator credential or an operator key with admin scope in `HV2_API_KEY`. The control plane must have cluster authentication configured, and the node retaining the guest must still be registered and running. Use its configured node ID. With the existing trusted control endpoint and CA, discover pending IDs:

```sh
hm sandbox vm --endpoint "$HV2_SANDBOX_URL" --api-ca-cert /path/control-ca.pem   pending-registrations --node-id "$NODE_ID"
```

Discovery returns `registrations` containing `sandboxID` and `kind`, plus `nextCursor`. Each page has at most 32 entries and contains no guest access token. If the cursor is non-null, pass it as `--after CURSOR` to retrieve the next page. Pages reflect current node state; registrations may change between requests. A pending entry can also be a publication still in progress, so use recovery after a failed operation or after confirming that publication remains uncertain.

Recover the selected guest on that same node:

```sh
hm sandbox vm --endpoint "$HV2_SANDBOX_URL" --api-ca-cert /path/control-ca.pem   reconcile-registration "$SANDBOX_ID" --node-id "$NODE_ID"
```

Success returns the existing guest descriptor, preserving its identity and access token, and clears the pending entry. Treat the descriptor as credential-bearing output. Discovery should no longer list that ID; confirm the application command succeeds before continuing normal operations. The explicit-node path also works when the initial record write failed and the control-plane sandbox inventory has no entry.

Observers and inventory/sandbox-only keys cannot discover or reconcile registrations. An unknown node returns 404; a guest without a pending registration returns 409. If a reply is lost, rediscover before retrying: a completed recovery may have cleared the entry. Retrying an uncertain publication may duplicate a lifecycle event; exactly-once delivery is not provided. This workflow repairs a preserved local guest, not a guest lost to a node crash, and does not reclaim a stranded shared snapshot claim.

[Four real KVM discovery/recovery profiles](benchmarks/2026-10-03/pending-discovery-schema-runtime/README.md) verify initial and resumed record/event failures, scoped refusals, exact guest execution/forwarding and cleanup. [Seventeen HTTP boundary checks](benchmarks/2026-10-03/pending-discovery-http-transport/README.md) verify token-free page schema, byte limits, chunked/truncated response refusal and authentication boundaries. Machine-crash recovery and automatic reconciliation remain incomplete.

Nodes may opt into automatic local registration retries with `--registration-reconcile-interval SECS` (1–3600). This requires a cluster store and nonempty cluster token. Each pass attempts up to 32 pending registrations with rotating selection and a five-second deadline per attempt, using the same transition lock as manual reconciliation. Retries preserve the guest and pending marker on failure. Pending state remains process-local; automatic recovery under Redis SET/XADD publication faults has passed four owned KVM profiles with 30 checks each. See [runtime evidence](benchmarks/2026-10-03/automatic-registration-kvm/README.md). Daemon-crash recovery remains unverified.

[Repeated cluster-store timeout recovery](benchmarks/2026-10-03/registration-store-timeout-kvm/README.md) also passed real KVM checks for initial and resumed guests. The shorter store deadline preserves pending state during stalled writes; recovery follows write release. This does not exercise the worker’s outer five-second timeout branch.

[Automatic named registration recovery](benchmarks/2026-10-03/named-automatic-registration-kvm/README.md) passes two real KVM profiles with 32 checks each. Control-API creation preserves the same operation reservation and guest under failed record or event publication, refuses duplicate named creation, and reconciles automatically after store recovery. Deletion releases the name and a fresh guest can reuse it. Named reservations have no automatic expiry; process-local pending state still does not establish daemon-crash recovery.

[Startup cancellation ownership](benchmarks/2026-10-03/unregistered-startup-cancellation/README.md) now stops guests and aborts network bridges that have not reached the local registry. After local insertion, the guard is disarmed and publication uncertainty continues to preserve the guest for recovery. Cleanup requires a live runtime; shared-claim crash fencing remains unverified.
