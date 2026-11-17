# HyperMachine Security Audit, 2026-11-16

**Scope.** This audit covers code, not only dependencies. The previous audit,
[SECURITY_AUDIT_2026-09-16.md](SECURITY_AUDIT_2026-09-16.md), covered only dependencies. Since then
about 200,000 lines have been added. This audit covers the three surfaces an attacker can reach:

- **The control plane** (`crates/hv2-cluster`). Attackers here are API clients, browsers using SSO, and
  compromised nodes.
- **The sandbox node** (`crates/hv2-sandboxd`, `crates/hv2-net`). Attackers here are API clients,
  compromised guests, and hostile images, archives and repositories.
- **The guest-to-host device boundary** (`crates/hv2-core` devices and VM exits, and the 9P server). The
  attacker here is a guest kernel that is root in its VM.

**Method.**
- Three independent read-only code reviews, one per surface. Each was told to report only what it had
  traced end to end. The reviewing session then checked the Critical and High findings against the
  source before any fix.
- Every fix has a test that fails without it. That was shown by reverting each fix, apart from three
  cases where the old code aborts the test process; those are argued, not run.
- After the fixes, these were rebuilt and run on KVM: the network and disk pause checks, the command
  line, the key API, and firmware machine boot (see [Verification](#verification)).
- Scanners: `cargo audit`, `cargo deny check advisories bans licenses sources`, and `gitleaks git` over
  the full history.

**Severity.** The release profile sets `overflow-checks = true` and `panic = "abort"`, and one
`hv2-sandboxd` process hosts every VM on a node. Any arithmetic overflow, out-of-bounds index or
`unwrap` that a guest can reach is therefore a node-wide outage, not a single-VM crash. Findings are
rated with that in mind.

## Scanners

| Check | Result |
|---|---|
| `cargo audit` (807 crates, 1,509 advisories) | 0 vulnerabilities; 12 unmaintained-crate warnings, all justified in `deny.toml` |
| `cargo deny check advisories` | ok. One warning: yanked `async-executor 1.14.0`, which only the desktop GUI uses |
| `cargo deny check bans` / `licenses` / `sources` | ok / ok / ok |
| `gitleaks git` (2,002 commits, 1.07 GB) | no leaks |

## Fixed

### Critical

**N1. A template image could write any file on the host.**
- Where: `hv2-sandboxd/src/machines.rs` `build_root`.
- The node writes a template's file tree onto the host before `mkfs.ext4 -d` packs it. It used
  path-based calls (`create_dir_all`, `fs::write`, `set_permissions`), and those follow symlinks that
  earlier entries in the same image had made.
- A registry image the client chose, containing `a -> /etc/cron.d` and then the file `a/x`, made the
  node write `/etc/cron.d/x` as the node's user. A trailing-slash twin (`a/` after the symlink `a`)
  could `chmod` the target instead.
- **Fix:** `write_tree` opens every component from its parent with `O_NOFOLLOW` and makes files with
  `O_CREAT|O_EXCL|O_NOFOLLOW`. It sets modes through the descriptor and never looks up a whole path. A
  symlink, or a file where a directory belongs, refuses the template.
  - Cpio names are normalised, so `a` and `a/` are the same entry.
  - The scratch tree is mode 0700.
  - The in-memory image builder resolves its own placements through the image's symlinks, so images
    with `bin -> usr/bin` still work.
- **Tests:** `machines::tree_tests` (7). The three attack tests wrote outside the tree on the old code.

### High

**C1. Any key with the `templates` scope could send any PUT or DELETE to every node, with the cluster
token attached.**
- Where: `share_template` in `control.rs`.
- The `{team}` parameter was decoded and then pasted into the node URL.
  `DELETE /templates/<own snapshot>/shares/x%2F..%2F..%2F..%2F..%2Fsandboxes%2F<victim>` reached the node
  as `DELETE /sandboxes/<victim>`.
- Every verbatim relay had the same flaw: `/volumes/%2e%2e` reached the node as `/`.
- **Fix:**
  - The team is parsed as a `TeamId`, and `.` and `..` are no longer valid team or principal ids.
  - Node URLs are built from percent-encoded segments (`node_url`).
  - Any relayed path containing a `.` or `..` segment, in any encoding, or a `\`, gets 400
    (`forwards_verbatim`).
- **Tests:** `tests/request_confinement.rs`. A fake node records every request it receives; on the old
  code it received the traversal.

**C2. The SSO guest hand-off accepted any domain.**
- Where: `sso_guest.rs` `route_of_url`.
- Only the first label of a guest URL was read, so `https://cp/auth/guest?url=https://8080-S.evil.example/`
  sent a signed-in member's hand-off token to the attacker's host. The real proxy then redeemed that
  token into a 12-hour guest pass, giving access to private guest apps as that member.
- **Fix:**
  - New repeatable `--guest-domain D`, required with `--sso-guest-urls`.
  - A guest URL must be one label plus a configured domain, or a bound custom domain.
  - The proxy's hand-off and guest-pass checks apply the same rule.
- **Tests:** in `tests/sso_login.rs`. Seven malformed URLs are refused, and a forged hand-off and pass for
  another domain are refused.

**C3. A compromised node could mint an administrator API key.**
- Where: `store.rs`, `control/key_api.rs`.
- Managed keys, added 2026-11-15, were stored in the Redis store that nodes also write, with no integrity
  protection.
- **Fix:**
  - Each record carries an HMAC-SHA256 over every field, keyed by `--managed-key-secret-file`, which only
    control planes hold.
  - An unsealed or altered record gets 503, never access.
  - Without the secret, managed keys are off.
  - Keys made before this change must be re-created.
- **Tests:** `a_record_this_control_plane_did_not_seal_never_authenticates`,
  `without_a_managed_key_secret_managed_keys_are_off`.

**H1-H3. A guest could abort the node daemon.**
- **H1:** ring addresses near 2^64 overflowed the ring arithmetic in `virtio_queue.rs`.
- **H2:** checksum offload, which a guest could enable because driver features were not masked, summed a
  frame of about 131 KB into a `u32`, which overflowed.
- **H3:** a virtio-blk status descriptor at the top of the address space overflowed `addr + remaining`.
- **Fix:**
  - Rings are validated on enable: aligned, sized and wholly in RAM. A refused enable sets NEEDS_RESET.
  - All ring arithmetic is checked.
  - `walk_chain` refuses any descriptor whose `addr + len` wraps or leaves RAM.
  - Driver features are masked to the offered ones.
  - The checksum is summed in `u64`, and over-limit frames are dropped before they are summed.
- **Tests:** `devices/virtio_guest_abuse_tests.rs` (18). Fifteen failed on the old code, most with an
  overflow panic at the lines named above.

**H4. One notify could make the host do terabytes of work.**
- `avail.idx` was trusted, so 65,535 phantom chains of up to 64 MiB each were all served on the vCPU
  thread while it held the device lock.
- **Fix:**
  - A queue whose `avail.idx - last_avail` exceeds the ring size is marked broken (NEEDS_RESET).
  - Each kick has a budget of one chain per ring slot plus a byte cap.
  - Per-queue chain limits: net 256 KiB, vsock 1 MiB.

**N2. The node API and its sandbox proxy were open on `0.0.0.0` without a cluster token.**
- Anyone who could reach a standalone node could run commands in its sandboxes, read their files and
  create machines. They could also reach published `proxy:true` machine ports, such as noVNC desktops,
  by guessable ids.
- **Fix:**
  - New `--bind`. Without a cluster token or mTLS the default is `127.0.0.1`.
  - A non-loopback bind without authentication refuses to start unless `--allow-unauthenticated` is
    given, and that flag warns loudly.
  - The deploy charts already set a token and are unchanged.
  - docs/MACHINES.md no longer implies that noVNC is behind a login.

**N3. Registry content could exhaust the node's memory.**
- Tar headers sized allocations: a header claiming 2^40 bytes aborted the process.
- Decompression was uncapped (gzip and zstd bombs).
- Blobs were buffered in full before their length was checked.
- Registry requests had no overall timeout.
- **Fix:**
  - Streaming with hard caps: 4 GiB unpacked per image, 2 GiB per file, and 1,000,000 entries.
  - Blob size and digest are checked as the bytes arrive.
  - Per-request and client timeouts.
  - Template uploads are capped the same way.
- **Tests:** in `oci` and `builds`.

**N5. A guest could exhaust the node's file descriptors through 9P.**
- There was no limit on fids or open files.
- **Fix:**
  - Per mount: 4,096 fids and 2,048 open files. Per node: 16,384 open files.
  - Directory listings stream instead of being built whole.
  - setuid and setgid bits are dropped on create, mkdir and chmod, in 9P and in the volume API.

### Medium

- **C4.** The managed-key limit was global, so one team-admin could block every team. It is now per
  team. One undecodable record no longer breaks listing and revocation.
- **C5.** `/cluster/nodes` showed any `inventory` key the node URLs, signing keys, and every team's private
  template names. Callers who are not administrators now get a redacted view.
- **C6.** Relayed requests carried the caller's `Authorization` header to nodes, including SSO session
  bearers, which a node could replay. It is now forwarded only on `/volumecontent/`, and never a
  session bearer.
- **N4.** A request-triggered panic in `build_from_image`, which sliced a client-supplied digest at byte
  16.
- **M1.** Guest-triggered warn logs (unmapped MMIO, unknown exits, bad queue kicks) had no rate limit. The
  first 8 per process are warnings; the rest are logged at debug.
- **N6.** `etc/hv2-env` keys were written unescaped. Invalid names are now dropped.

## Fixed in a follow-up the same day

Six of the open findings were fixed after the first commit. Each has a test that fails without its fix.

- **O6 (Medium): a guest could reach the host's own addresses.** Under an allow-default policy the
  gateway refused only the reserved ranges, so the node's public IP was reachable. The gateway now
  reads the host's interface addresses (`getifaddrs`) at start and treats them as reserved, also
  after a sandbox's policy is replaced. An interface added after start is not covered.
- **O8 (Medium): template-build uploads.**
  - An upload is published with a hard link, which fails if the name exists, so no caller can
    replace another's upload, and two racing uploads cannot overwrite each other.
  - Uploads are capped at 4 GiB each (413) and 256 files or 64 GiB per node (507).
  - A refused upload used to be published under its name anyway; it no longer is.
  - The content hash is checked only with `--verify-upload-hash`: E2B's SDK names an upload by a
    hash of its file list, not of the bytes, so a default-on check would refuse every E2B build.
  - The namespace is still node-wide; per-team scoping remains a design decision.
- **O9 (Low):** errors from the store and from nodes are logged on the control plane and clients get
  a generic message.
- **O10 (Low):** `hm sandbox vm login` sends a random `cliState`, the control plane seals it in the
  login cookie and echoes it, and the CLI's listener refuses a callback without it. A new CLI cannot
  sign in through an older control plane.
- **O11 (Low):** the webhook filter refuses `198.18.0.0/15`, 6to4, Teredo and NAT64 prefixes other
  than `64:ff9b::/96`, where the embedded IPv4 address decides. Each team may have 100 webhooks.
- **O13 (Low):** `docs/SANDBOXES.md` explains that `allowOut` alone does not restrict egress when the
  policy default is allow.

Verified with unit tests only (hv2-cluster, hm-cli, hv2-net and hv2-sandboxd, on Windows and Linux),
not on KVM.

## Fixed in a second follow-up

- **O1 (High, plausible): e2fsprogs parsed guest-written images unconfined.** `debugfs`, `e2fsck` and
  `resize2fs` now run in a child confined with Landlock: it may write only the scratch directory
  (for `rdump`) or the one image (for `e2fsck` and `resize2fs`), and read only the image, system
  binaries and libraries, and its own mount table. The child also gets `no_new_privs`, limits on file
  size, address space and CPU, no core dumps, and a wall-clock timeout. Before an `rdump` result is
  archived, sockets, FIFOs and device nodes are deleted, and links are never followed. A host
  without Landlock refuses these operations unless `--allow-unconfined-e2fs-tools` is given. There
  is no seccomp filter and no network namespace.
- **O2 (Medium): registry pulls reached internal addresses.**
  - **Address check:** every connection (registry, bearer realm and each redirect hop) resolves its
    host and refuses addresses that are not public. That includes loopback, private ranges,
    link-local, the cloud metadata address, CGNAT and ULA. The connection is pinned to the checked
    address.
  - **Redirects:** up to five are followed, each checked again. A redirect from https to http is
    refused, and credentials go only to the original origin.
  - **Realm:** it must use https when the registry does.
  - **Flags:**
    - `--allow-private-registries` turns the address check off.
    - `--registry-allow HOST` (repeatable) admits only the listed registry and realm hosts.
- **O12 (Low): partly fixed.** Revoking a managed key now also writes a sealed revocation record that
  is never removed by normal operations. Authentication and listing refuse a key that has a valid
  one, so writing the old record back no longer revives the key. What remains is in the open table.

## Fixed in a third follow-up

- **O3 (Medium): the node's sandbox proxy had no authentication.**
  - **What the node requires:** a node with a cluster token accepts a proxy connection only if it
    begins with `HV2-PROXY <hmac> <nonce> <unix-time>`. The HMAC is HMAC-SHA256 under the cluster
    token.
  - **Checks on the line:** it is compared in constant time, must be within 60 s of the node's
    clock, and is refused if its nonce has been seen before.
  - **The control plane:** it sends the line on every connection it relays.
  - **Opt-out:** `--proxy-open` keeps the old behaviour and logs a warning.
  - **What remains:** the line is not bound to its connection. An on-path observer could replay a
    captured line within 60 s, before the original arrives, so mTLS or a private network still
    matters.
  - **Evidence:** a KVM check first found that the node made a new nonce memory for every
    connection, so replays were accepted. That is fixed:
    [proxy-auth-kvm](../benchmarks/2026-11-16/proxy-auth-kvm/README.md).
- **O7 (Medium): a guest could fill the host disk through a volume.**
  - **Disk guard:** 9P operations that grow storage, and the HTTP volume content API, are refused
    with `ENOSPC` (507 over HTTP) when the volume's filesystem would fall below a reserve. The
    reserve is 5% of its space, capped at 2 GiB, or 1% of its inodes. `--volume-reserve-percent N`
    changes it, and 0 turns it off.
  - **Optional quotas:** `--volume-max-bytes` and `--volume-max-inodes` set per-volume quotas, off by
    default. Hitting one returns `EDQUOT`.
  - **Verified on KVM** (2026-11-17), from inside a guest over 9P:
    - An 8 MiB byte quota and a 24-entry inode quota stopped `dd`, `touch`, `mkdir` and `ln -s` with
      "Disk quota exceeded" exactly at the limit, and deleting freed both.
    - On a 64 MiB filesystem the guard stopped a write with "No space left on device" just above
      the 5% reserve. With `--volume-reserve-percent 0` the same write filled it.
    - Evidence: [volume-quota-kvm](../benchmarks/2026-11-17/volume-quota-kvm/README.md).

## Fixed in a fourth follow-up (owner decisions)

The owner chose how to resolve O4 and O5.

- **O4 (Medium): the control plane ran open with no key source.**
  - **No key source:** `hv2-control-plane` now refuses to start, with an error naming
    `--allow-unauthenticated`. Key sources are `--api-key`/`HV2_API_KEY`, `--api-keys-file` and
    `--sso-issuer`.
  - **Opt-in:** with `--allow-unauthenticated` it runs open as before and logs a warning at each
    start.
  - **Managed keys:** `--managed-key-secret-file` alone does not count as a key source, because
    managed keys are minted by an administrator.
  - **Callers updated:** the CI FIPS step and `check-domain-verification-startup.py` now pass a key.
  - **Verified:** on Linux, the release binary exits 1 with no key, and keeps running with the flag.
- **O5 (Medium): keys with a principal and no team saw every team's resources.**
  - **Personal scope:** such a key, unless it is an administrator, now acts as a team of one. Its
    label is `principal.` followed by a hash of the principal. That label is recorded on whatever
    the key creates, and every existing team check applies to it.
  - **What the key sees:** events, webhooks and webhook deliveries, machines, volumes, snapshots,
    templates, shares and telemetry are limited to its own.
  - **Other rules:** the cluster event log refuses such a key, and a real team may not use the
    `principal.` prefix. Sandboxes keep their existing owner scoping.
  - **Records made before the change** carry no scope, so these keys cannot see them.
  - **Template builds** are not scoped; see O14.
  - **Verified:** seven new tests, six of which fail without the change. `verify-owner-sharing-kvm.py`,
    which uses such keys, passes all 11 cases on KVM.

## Fixed in a fifth follow-up (owner decision)

- **O14 (Low): template builds by a key with a principal and no team went to the node-wide
  namespace.** As the owner chose, such builds are now scoped to the principal.
  - **The name:** a build is stored on the node as `scope.<16 hex of the principal hash>--<name>`.
    The control plane adds the prefix, resolves the plain name for the owning key, and reserves
    `scope.` names for their owner and administrators.
  - **Build ids:** before a build start or status call, the control plane asks the node for the
    build and requires the build's template to be the caller's own.
  - **Other principals and team keys:** they can neither list, use, build over nor delete such a
    template.
  - **Verified on KVM:** through a real control plane and node, alice's `app` built as
    `scope.…--app` and a sandbox from it ran. Bob's 9 calls on her template, by plain name,
    canonical name and template id, all got 404. His own `app` got its own scope, and an
    administrator saw both. Evidence:
    [principal-templates-kvm](../benchmarks/2026-11-17/principal-templates-kvm/README.md).
  - **Remaining:**
    - Content-addressed upload hashes, pulled images and cached build steps are still shared
      node-wide by identical input.
    - Builds made before this change remain visible to everyone.
    - A scoped name leaves 40 of the node's 64 characters.

## Open: found, not fixed

| Id | Severity | Finding | Why not now |
|---|---|---|---|
| O12 | Low | Partly fixed (see below). A node with write access to Redis can delete a key's sealed revocation record and then replay the key's old record. It can also delete records outright (denial of service). Keys revoked before the fix have no revocation record. | Redis cannot stop a writer from deleting. A key's own expiry bounds the damage. |

## Checked and found sound

Each reviewer listed what it checked and found correct. The main items:

- **Control plane: authentication.**
  - Constant-time comparison of every key and token.
  - Expired keys are refused.
  - `permits` and axum route the same path.
  - Every per-sandbox route checks team access, including tunnels and listings.
  - Owner and team headers from clients are stripped.
  - The client's API key is never sent to nodes.
- **Control plane: SSO.**
  - State, nonce and PKCE.
  - RS256 and ES256 only.
  - `iss`, `aud`, `azp`, `exp` and `email_verified` are checked.
  - `__Host-` cookies.
  - The origin is checked on cookie-authenticated changes.
- **Control plane: outbound requests.** Webhooks pin DNS, do not follow redirects, and accept only
  global addresses.
- **Node: shared directories and repositories.**
  - The 9P server resolves every path with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`.
  - Volume, checkpoint and machine file APIs never touch host paths.
  - Git clones are hardened: no hooks, no redirects, no submodules, an allow-list, and credentials kept
    out of argv.
  - Sandbox ids are random.
  - Published machine ports default to loopback.
- **Gateway egress.**
  - Reserved and metadata addresses are denied before any name rule.
  - SNI and Host are pinned to the gateway's own DNS answers.
  - Rebinding to private addresses is blocked.
  - IPv6 and ICMP are dropped.
- **Devices.**
  - `GuestMemory::translate_range` is checked and refuses accesses that cross regions.
  - Descriptor walking is bounded (indices, cycles, indirect tables, chain bytes).
  - virtio-blk bounds checks and read-only enforcement are correct.
  - vsock length handling is correct, and its queues are bounded.
  - PCI and MMIO register decode is correct, and BARs cannot move over RAM.
  - The pause delivery gate's queues are bounded.

## Verification

- **Unit and integration tests.** The tests of every changed crate pass on Windows (1.98.1) and Linux
  (WSL, 1.100.0, `RUSTFLAGS=-D warnings`), and the Redis store tests run against a real Redis. Clippy
  with `-D warnings` and `rustfmt` are clean.
- **KVM.** The binaries were rebuilt from the fixed tree, then these were rerun on KVM in WSL2 with
  nested KVM:
  - networked pause and standby under traffic (virtio-net and vsock);
  - disk pause (virtio-blk);
  - the `hm sandbox vm` command line;
  - the managed key API, with the new secret, across two control planes and one Redis;
  - a firmware-booted Ubuntu machine (virtio-pci).

  Four of the five passed in full, each on KVM with no TCG fallback. The firmware check booted Ubuntu
  to its login prompt, then stopped at a login written for a CirrOS fixture that no longer exists.
  Details: [security-fixes-kvm](../benchmarks/2026-11-16/security-fixes-kvm/README.md).

## Not covered

- `hv2-net` modules not read: `sniff`, `dns`, `socks`, `mitm`, `private_udp`, `nat`, `vswitch`.
- Other sandboxd modules not read: `disks`, `seed`, `env_vars`.
- Exit decoding for WHPX and HVF beyond MMIO and PIO.
- The userspace IOAPIC.
- Guest-agent protocol parsing beyond framing.
- A fuzzing campaign. None was run; the virtio queue code is an obvious target.
