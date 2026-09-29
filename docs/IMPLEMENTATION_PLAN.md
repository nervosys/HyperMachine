# Implementation Plan to Completion

Status as of **2026-09-26**. This is the working checklist for finishing
HyperMachine. It draws on the project's own statements of what is unfinished
— [`CUBESANDBOX_PARITY_ROADMAP.md`](CUBESANDBOX_PARITY_ROADMAP.md),
[`SECURITY_AUDIT.md`](SECURITY_AUDIT.md),
[`security/EXPORT_CONTROL_AUDIT.md`](security/EXPORT_CONTROL_AUDIT.md) and
[`handoff.html`](handoff.html) — each checked against the code rather than
taken as written. Several of those documents turned out to be stale; where
they are, this plan says so.

## How to read this

- **Size** — S (hours), M (days), L (a week or more), XL (a phase in itself).
- **🔓 doable now** on the current machine. **🔒 blocked** on hardware, an
  administrator, or a decision — the blocker is named.
- **Done when** is the check that closes the item. Each one is written so
  that it **can fail**. That is deliberate: this repository has repeatedly
  shipped checks that could not — a sweep that counted lines and ignored
  cargo's exit status, an MSRV job that installed the wrong toolchain and
  passed, a Deploy workflow red for so long that new failures looked like the
  old one. A "done when" that cannot fail is not a completion criterion.

## Project-level definition of done

The project is complete when all of these hold:

1. Every CI job on `master` is green, and each one fails when it should.
2. Every security claim in the documentation matches the code.
3. The parity roadmap's Phases 3–5 have met their exit criteria.
4. Every component described as "has never run" has run on its target
   hardware — or is explicitly scoped out in writing.
5. A release has shipped: crates published, export notification filed,
   signed binaries built by a green pipeline.

## Where things stand

| Area | State | Evidence |
| --- | --- | --- |
| Local sweep | green | fmt, clippy 0 (exit 0), rustdoc 0, 5461 tests, 46 examples |
| CI on `master` | 19 of 22 green | three failures, all diagnosed below |
| Stubs in shipping code | **none** | all 7 `unimplemented!()` are a test double in `vm_host.rs` |
| Crypto | IronCrypto 0.1.1, from crates.io | published vectors pass; no private-repo credentials |
| Parity roadmap | Phases 0–5 built; 6 in progress | 3–6 are commits on a local `master`, not yet pushed (2026-09-26) |
| Hardware | hv1, hv1-arm, WHPX never run on target | see Phase E |
| Confidential computing | controls refuse honestly | no SEV/TDX backend exists |

---

## Phase A — Land what is in flight 🔓

Critical path. Everything else assumes a green, trustworthy CI.

- [x] **Merge #99** (`fix/ci-protoc-and-msrv`). *S.* Merged 2026-09-27.
  Done when: on `master`, Deploy's *Build Container Image* and *Build Windows
  Binaries* jobs pass, and *MSRV Check* passes. (MSRV already passes on the
  PR at 1.95 — confirmed, not assumed.)
- [x] **Rebase and merge #100** (IronCrypto 0.1.2) after #99. *S.* Merged
  2026-09-27; no rebase was needed.
  It fails MSRV only because it branches from a `master` without #99's fix.
  Done when: all checks on the rebased PR pass.
- [x] **Fix *HV1 Multiboot Image*.** *S.* Merged in #102. The fix that
  worked was `env -u RUSTFLAGS` on the build step: `RUSTFLAGS: ""` still
  counts as set, and still replaces the target flags.
  Cause, reproduced locally: `ci.yml` sets a workflow-wide
  `RUSTFLAGS: -D warnings`, and an environment `RUSTFLAGS` *replaces* — does
  not merge with — the crate's `[target.x86_64-unknown-none] rustflags`. That
  silently drops `relocation-model=static` and the `-Tlink.ld` linker script,
  giving 15 `R_X86_64_32 cannot be used against local symbol` errors. Clean
  without the variable.
  Fix: `RUSTFLAGS: ""` at job level. Warnings are still denied by that job's
  own clippy step.
  Done when: the job builds, and its existing "loadable Multiboot image"
  checks pass.
- [x] **Fix *Unikernel Guest Build*.** *S.* Merged in #102. Two causes,
  reproduced locally:
  - [x] `hv2-unikernel` defines `memset`/`memcpy`/`memmove`/`memcmp` in
    `*mut u8`; the compiler calls them as `*mut c_void`. Same fix as
    `fce2264` applied to `hv1-multiboot`.
  - [x] The same `RUSTFLAGS` override as above.

  Done when: the job builds with `-D warnings` in effect.
- [x] **Push the Phase 3–6 commits, then merge `fix/tenant-reserved-egress`.**
  Pushed, and merged as #103 on 2026-09-27.
  🔒 *push approval.* *S.* The branch closes a hole found reviewing Phase 4:
  a tenant's `allowOut: ["0.0.0.0/0"]` reached private addresses, which in a
  cluster means the unauthenticated state store holding every sandbox's
  envd access token. Before the fix the cloud metadata address was reachable
  too. Reproduced on real KVM guests; `tools/e2e-egress.sh` fails on the old
  code and passes on the branch.
- [x] **Pin `dtolnay/rust-toolchain@master`** to a commit. *S.* #108 pins
  every action (136 references) to a commit SHA, not only this one.
  It is unpinned, and it was a silent upstream behaviour change that exposed
  the MSRV defect. Done when: no workflow references an action at `@master`.
- [ ] **Triage the Dependabot PRs.** *M.* Triaged 2026-09-27. Every open
  one is green on the current `master`, including `#91` (base64 0.23) and
  `#92` (tower-http 0.7), both semver-major, and `#107`, the grouped update
  that replaced `#98` and `#105`. `#89` (Slack v4) would have silently
  stopped deployment notifications: v2 no longer reads the webhook from
  `env`, and the step has `continue-on-error`. It was fixed on its branch.
  `#85`/`#86`/`#88` bump actions only `deploy.yml` uses, which pull requests
  do not run, so the first deploy after merging is their real test.
  `#85`-`#89` and `#107` merged on 2026-09-28; `#91` and `#92` go one at a
  time, because each rebases the shared `Cargo.lock`.
  Done when: each is merged.
- [ ] **Decide the branch-protection policy.** 🔒 *decision.*
  On 2026-09-23 a direct push bypassed "changes must be made through a pull
  request". Either require it for everyone or record who may bypass and when.

**Exit criterion:** every job on `master` green for three consecutive runs.
The first fully green run was c4c6d0c, on 2026-09-27: CI, Security,
Deploy, Coverage and Benchmarks.

---

## Phase B — Make the documentation true 🔓

The recurring defect in this project has been prose that claims more — or
less — than the code does. These are the known instances still outstanding.

- [x] **Reconcile `security/EXPORT_CONTROL_AUDIT.md` §7.** *S.* Done
  2026-09-28, and further than §7: §1, §3, §5.4, §6, §8 and §9 described the
  `ring`-era implementation too. Classifications that rested on changed facts
  (the post-quantum algorithms' EAR99, RSA and ECDSA as "encryption") are
  marked for re-review rather than reassigned. Checking the inventory also
  found AES-128-GCM broken since 93abff0; that is now fixed and tested (K-7).
  Four of its five "known issues" are already resolved:
  - [x] **K-1** custom RSA encrypt/decrypt — deleted; it computed wrong
    results and had no callers.
  - [x] **K-2** AES-CTR+HMAC fallback — gone; the only remaining mention is a
    comment saying there should not be one.
  - [x] **K-3** "PQC modules implement API stubs" — false; `pqc.rs` is backed
    by the `ml-kem`, `ml-dsa` and `slh-dsa` crates.
  - [x] **K-4** vTPM PCR extend uses XOR — fixed in `922761f`; now a SHA-2
    hash chain with property tests.

  Done when: each is marked resolved with the commit that resolved it.
- [x] **`SECURITY_AUDIT.md` recommendation #3** ("retire the `rsa` timing
  advisory") — done in 814d0c0; marked 2026-09-28. The accepted-advisory
  table was also out of step with `deny.toml` (three entries for crates no
  longer in the build, two missing) and now mirrors it.
- [x] **Parity roadmap, Phase 4**: "`hv2-runtime`'s 8 tests are the thinnest
  coverage" — it has 279. Gone in the Phase 4 rewrite, which also records
  why the cluster was not built on `hv2-runtime`.
- [ ] **Upstream: correct the `ic-mldsa` and `ic-mlkem` crates.io
  descriptions.** 🔒 *IronCrypto repository.* *S.*
  They read "incomplete, no signature scheme yet" and "experimental, not
  vector-tested". Both are false — `ic-mldsa` has sign, verify, both prehash
  variants and a deterministic variant, with 55 NIST ACVP vectors; `ic-mlkem`
  has 50. The descriptions cost a recommendation in this repository.
- [x] **Add a claim-versus-caller check to the sweep.** *M. Optional, high
  leverage.* Done 2026-09-29 as `tools/find-unread-controls.py`, in the sweep
  and in CI. Beyond this item's claims-without-callers rule, it finds refusal
  variants nothing raises and mode variants nothing reads. On the tree before
  #112 it flags both of that week's bugs (`FipsMode::Strict` and
  `AlgorithmNotApproved`). On today's tree, 23 findings are reviewed with
  reasons. One corrected a real mismatch: `SecureBootMode::Audit` was
  documented as "log but don't enforce", and enforced. A script flagging module headers that say "wires", "consults" or
  "enforces" where the named subject has no caller outside its own file.
  Five such headers were found by hand on 2026-09-22.
  Done when: the script fails on a planted false claim and passes on the tree.

**Exit criterion:** the reconciliation list above is empty and
`tools/check-docs-against-code.py` is clean.

---

## Phase C — Crypto consolidation 🔓

One library for all cryptography, and none of it hand-rolled.

- [x] **Adopt `ic-fips`** for FIPS module policy, self-tests and service
  indicators. *M.* Done 2026-09-28. `Enabled` and `Strict` run `ic-fips`'s
  pre-operational self-tests and draw from `ic-drbg`'s HMAC_DRBG. `Strict`
  checks every operation and refuses SLH-DSA, the one post-quantum
  algorithm still from RustCrypto, as outside the boundary. Before this, `Strict` refused nothing. The
  module-wide approved mode (`ic_fips::set_mode`) is left to the operator,
  since a library switching process state behind its caller would be wrong.
  This is the layer the repository hand-rolled and got wrong: `kat_aes_gcm`
  was an encrypt-then-decrypt round trip, and `kat_hmac_sha256` discarded its
  MAC. Done when: `FipsMode` self-tests delegate to `ic-fips`, and the
  published-vector tests still pass.
- [x] **Evaluate `ic-rustls`** as the rustls `CryptoProvider`, replacing
  `ring`. *M.* Evaluated 2026-09-28. **Adopt later, after two blockers.**
  - *Suitable:* the published 0.1.3 is byte-identical to the repository. It
    covers TLS 1.3 and 1.2, AES-GCM and ChaCha20-Poly1305, X25519/P-256/P-384,
    and RSA PKCS#1/PSS, ECDSA and Ed25519 verification. That is enough for
    the egress gateway, which talks to arbitrary servers.
  - ~~*Blocker 1, `ic-rsa`:*~~ **Cleared 2026-09-29:** IronCrypto 0.2.2
    fixed the RSA-4096 panic in `from_primes`, which `ic-rustls` uses to load
    RSA keys.
  - *Blocker 2, `ring` cannot fully leave:* `rcgen`, which issues the
    egress gateway's per-sandbox CA and leaf certificates, has only `ring`
    and `aws-lc-rs` backends, and `ic-pkix` neither parses nor issues
    certificates. `reqwest`, `redis` and `hyper-rustls` would also need their
    no-provider features. The old "done when `cargo tree -i ring` is empty"
    is unreachable until IronCrypto can issue certificates.
- [ ] **Adopt `ic-rustls`.** *M.* Unblocked by IronCrypto 0.2.2. Done when:
  every rustls config in the workspace uses the IronCrypto provider, TLS
  tests pass, and `ring` remains only under `rcgen`, with that recorded.
- [x] **Migrate ML-KEM and ML-DSA to IronCrypto.** *M.* Done 2026-09-29 on
  IronCrypto 0.2.3, which added ML-KEM-512/1024 and ML-DSA-44/87 with ACVP
  vectors and `ic-fips` self-tests for all six sets. `ml-kem` and `ml-dsa`
  are now dev-dependencies only, kept for tests proving that keys and
  signatures from either implementation work with the other. `Strict` admits
  both algorithms.
- [ ] **SLH-DSA: stay on RustCrypto** — IronCrypto has no FIPS 205 crate.
- [ ] **Replace the `slh-dsa = "=0.2.0-rc.5"` pin** with a released version
  when one exists. *S.* A release candidate in a cryptographic path.
  Checked 2026-09-28: rc.5 (2026-04-28) is still the newest release, and the
  only stable one, 0.1.0, predates the final FIPS 205, so it would be a
  downgrade. Blocked upstream. The exact `=` pin is right until then: it
  stops cargo taking a later release candidate with breaking changes.
- [ ] **CMVP decision (export item R-3).** 🔒 *decision.* Pursue validation
  for IronCrypto, or link a validated module for regulated deployments.

**Exit criterion:** every primitive comes from one provider, and no
self-test in this repository is hand-written.

---

## Phase D — Parity roadmap Phase 3: network security 🔓

Largely built by the parity roadmap's Phase 3 (ddad478). `hv2-sandboxd
--network` gives each sandbox a virtio-net NIC whose far end is
`hv2_net::gateway`, a userspace TCP/IP stack that decides every connection and
DNS query against E2B's `network` policy. The enforcement point is the gateway
and `network_policy::NetworkPolicy`; the `Bridge` passes `EgressPolicy::allow_all()`.

- [x] **Put a network with enforced egress on the product path.** *L.*
  Done: on real KVM guests created through the E2B API, a sandbox with no
  network config is refused (`default deny`), and the gateway's decision
  log says so. The test is `tools/e2e-egress.sh` (branch
  `fix/tenant-reserved-egress` until merged).
- [x] **Land the tenant/operator split for reserved addresses** (see Phase A).
  Merged as #103.
  A tenant's `allowOut` may open reserved ranges only where the operator
  grants them (`--tenant-reserved-cidr`). The Helm chart also gains a store
  password and an egress NetworkPolicy on node pods. *S, written.*
- [ ] **Resolve `networking::filter`** (deprecated, wired to nothing): wire its
  connection tracking in beside `NatTable`, or delete it. *M.*
- [ ] **Wire or remove `permission_middleware`.** *M.*
  Its header claimed it "wires the graph-based permission system into the
  API"; no router installs it. Done when: a request lacking a permission gets
  403 in a test — or the module is gone.
- [ ] **Resolve `hm-cli`'s `gpu_passthrough` flag.** *S–M.*
  It is offered through the CLI, the MCP tool schema and the ontology, and
  drives nothing — `hm-cli` does not depend on `hv2-gpu`. Wire it or remove
  it from all three.
- [x] **Name-based egress.** Built in Phase 3. `allowOut` takes names and
  `*.name`, and the gateway's own DNS answers and TLS SNI tie a connection to
  a name. A name never opens a reserved address (DNS rebinding is tested).
- [x] **L7 egress proxy with credential injection.** Built in Phase 3.
  `network.rules[name].transform.headers` is injected by TLS interception
  against a per-sandbox CA, and verified with the unmodified E2B SDK.
  Known gap, from the roadmap: rustls-based clients stall after the
  ServerHello under interception, while OpenSSL-based clients work.

**Exit criterion:** a deployed sandbox has a network, and default-deny egress
is enforced on it and proven by a test.

---

## Phase E — Hardware verification 🔒

Things that compile and have never run. None can be finished on this
machine, but the first is one reboot away.

- [ ] **Run the WHPX backend.** 🔒 *administrator + reboot; hardware present.*
  *S* once enabled. WHP does not start here (`HRESULT 0x80370302`); enabling
  the *HypervisorPlatform* Windows feature fixes that. Exercises
  `kick_vcpu` and the Multiboot entry path, both currently compilation
  results only. Done when: a guest boots under WHPX.
- [ ] **Boot hv1 on bare metal** with a serial console. 🔒 *machine.* *L.*
  Covers the firmware path, the real memory map, AP bring-up and real
  devices — all hidden today by running nested under KVM.
- [ ] **Verify the NMI preemption path** on real hardware. 🔒 *same machine.*
  *M.* Under KVM the counter overflows and delivers nothing, which says
  nothing about a real PMU.
- [ ] **Finish `hv1-arm`'s exit path.** 🔓 *code now; run needs AArch64.* *L.*
  - [ ] MMIO dispatch (a data abort currently resumes without advancing the
    PC, so a guest would fault forever)
  - [ ] Undefined-instruction injection (the comment says so; the code does
    not)
  - [ ] PSCI (`HVC`/`SMC` are skipped; `CPU_ON` returns whatever was in x0)
  - [ ] Run on an AArch64 host. 🔒
- [ ] **Re-run the benchmarks on bare metal.** 🔒 *M.* Phase 0/1's numbers
  and VNNI. The requirement is a number, not an adjective: host drift well
  under 1% across the run. It was 45% within a single condition here.
- [ ] **GPU passthrough end to end** on a real GPU with an IOMMU. 🔒 *M.*
  The VFIO code is real and has never attached a device.

**Exit criterion:** each item has run on its target, or is struck through
with a written reason it is out of scope.

---

## Phase F — Confidential computing and DoD posture

Today these controls **refuse honestly** rather than claiming protection. That
is the precondition for this phase, not its completion. Guest isolation is
what KVM provides, plus boot-image admission, plus `hv2-sandbox`'s real OS
containment on the agent tool path. None of it protects a guest from the host.

- [ ] **`MemoryEncryptionBackend` for SEV-SNP.** 🔒 *EPYC hardware.* *XL.*
  Done when: `EncryptionManager::enable` succeeds only with a backend attached,
  and a guest's memory reads as ciphertext from the host.
- [ ] **Hardware-rooted attestation** (SNP report or TDX quote). 🔒 *XL.*
- [ ] **Make the vTPM reachable.** 🔓 *L.* No guest can use it today — there
  is no command dispatcher and no device model. Needs both before it can do
  anything for a guest.
- [x] **Real secure-boot signature verification.** 🔓 *M.* Done 2026-09-28:
  keys are DER SubjectPublicKeyInfo via `ic-pkix`, and the signed message is
  domain-separated and binds the component type. A forgery under a trusted
  name is refused, and so is the variant carrying the forger's own key. Both
  are mutation-checked. Original note: Define an encoding
  for `Certificate::public_key` — SubjectPublicKeyInfo DER via `ic-pkix` is the
  natural choice — and verify against the **trusted database entry's** key,
  never the one travelling with the signature. Done when: a valid signature
  is admitted, and a forged one with a matching subject string is refused.
- [x] **Forward audit logs to a tamper-evident store.** *M.* Audit
  recommendation #4. Done 2026-09-28 at the library level.
  `hv2_core::security::audit_chain` writes an HMAC-SHA256-chained JSON Lines
  file a SIEM collector can tail. It catches an edited, deleted, reordered or
  spliced record; the collector's copy catches tail truncation. The MCP log
  (`McpConfig::audit_chain`) and the HTTP audit (`AuditLogConfig::chain`)
  both feed it, and `AuditChain::from_env` opens one from `HV2_AUDIT_CHAIN`
  and `HV2_AUDIT_KEY_FILE`. `verify_audit_log` checks a file, and an
  independent Python writer built from the documentation interoperates.
- [x] **Open the audit chain in the shipped server.** *S.* Done 2026-09-28:
  `hv2 serve` opens it from `HV2_AUDIT_CHAIN` + `HV2_AUDIT_KEY_FILE`, turns
  HTTP audit logging on when they are set, and refuses to start if the chain
  cannot be opened. Verified with the real binary: two requests produced a
  chain that `verify_audit_log` passes, and a bad key file exits 1 with no
  listener. (This item first read "no shipped binary installs the HTTP audit
  middleware". That was wrong: a search cut off at eight lines missed the
  one production caller. `hv2 serve` runs `hv2_api::server::Server`, which
  applies the full stack.)
- [ ] **Independent penetration test, SSP/POA&M, ATO.** 🔒 *organisational.*
  Audit recommendation #5.

**Exit criterion:** the "What does not do what its name says" list in
`SECURITY_AUDIT.md` §3.4 is empty.

---

## Phase G — Parity roadmap Phases 4 and 5 🔓

Deliberately last: none of it matters unless Phases 0–3 make the platform
worth deploying.

- [x] **Multi-node control plane.** Built in Phase 4 (b4c0761) as
  `hv2-cluster`: stateless control planes over a Valkey store, where any
  instance serves any request. Its tests run the store contract against a
  real Valkey when `HV2_TEST_REDIS` is set.
- [ ] **Validate the Kubernetes and Terraform deploy path.** *L.* Partly done
  in Phase 5: the Helm chart passes `helm lint` and `kubeconform -strict`,
  `terraform validate` passes, both images build, and the Compose stack runs
  the SDK lifecycle test. Still never deployed to a real cluster. The old
  `deploy.yml` staging step still depends on `STAGING_KUBECONFIG`.
- [x] **WebUI.** Built in Phase 5: `/ui` on any control plane, rendering with
  `textContent` only. Its CSP still allows inline script
  (`script-src 'unsafe-inline'`). That is safe while nothing is rendered as
  HTML, but it is not the "strict CSP" the commit describes. Moving the script
  to its own route would let `'unsafe-inline'` go.
- [ ] **ARM64 as a supported platform** (depends on Phase E's `hv1-arm`
  work). *L.* The sandbox crates now build for aarch64, and the KVM backend
  refuses clearly on non-x86_64 hosts.

---

## Phase H — Release and compliance

- [x] **Publish `hv2-sandbox` to crates.io.** 1.1.0, published from 81bc883
  (its `.cargo_vcs_info.json` records that commit). Lit uses it (nervosys/Lit
  4762d32).
- [ ] **File the BIS/NSA notification (export item R-1)** before the next
  signed binary release. 🔒 *organisational.*
- [ ] **Commercial-licence distribution terms (export item R-2).** 🔒
- [ ] **A green `release.yml`** producing signed binaries. *M.*

---

## Sequencing

```text
A ──► B ─┬─► C ──► D ──► G
         │
         └─► H (hv2-sandbox publish, once A is green)

E ─ blocked on hardware; runs in parallel as machines appear
F ─ vTPM + secure boot now; SEV/attestation when EPYC is available
```

Phase A is the critical path. Phase B can run beside it. Phases E and F are
gated on hardware and proceed whenever it exists.

## Decisions needed

These are not engineering questions, and they block work:

1. **Branch protection** — enforce pull requests for everyone, or record the
   exceptions.
2. **CMVP** — validate IronCrypto, or link a validated module where required.
3. **Hardware** — a bare-metal x86 machine with a serial console, an AArch64
   host, and an EPYC system for SEV-SNP.
4. **Egress by name** — DNS-based or TLS-SNI-based, which trades trust in the
   guest's resolver against terminating TLS.
5. **Export filings** — who files R-1 and R-2, and when.

## Ground rules

Learned the hard way on this repository:

- **A check must be able to fail.** Before trusting a green result, confirm
  what a red one would look like.
- **Grep for callers before trusting a header.** Five module headers claimed a
  connection that did not exist; one audit claimed less than the code did.
- **Never conclude absence from a truncated view.** `head` and `tail` produced
  two confident wrong conclusions in one day. Count with `grep -c` first.
- **A registry description is a claim, not evidence.** The same applies to
  `README`s, commit messages and this document.
