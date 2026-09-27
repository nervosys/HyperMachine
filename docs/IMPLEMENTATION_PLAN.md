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

- [ ] **Merge #99** (`fix/ci-protoc-and-msrv`). *S.*
  Done when: on `master`, Deploy's *Build Container Image* and *Build Windows
  Binaries* jobs pass, and *MSRV Check* passes. (MSRV already passes on the
  PR at 1.95 — confirmed, not assumed.)
- [ ] **Rebase and merge #100** (IronCrypto 0.1.2) after #99. *S.*
  It fails MSRV only because it branches from a `master` without #99's fix.
  Done when: all checks on the rebased PR pass.
- [ ] **Fix *HV1 Multiboot Image*.** *S.*
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
- [ ] **Fix *Unikernel Guest Build*.** *S.* Two causes, reproduced locally:
  - [ ] `hv2-unikernel` defines `memset`/`memcpy`/`memmove`/`memcmp` in
    `*mut u8`; the compiler calls them as `*mut c_void`. Same fix as
    `fce2264` applied to `hv1-multiboot`.
  - [ ] The same `RUSTFLAGS` override as above.

  Done when: the job builds with `-D warnings` in effect.
- [ ] **Push the Phase 3–6 commits, then merge `fix/tenant-reserved-egress`.**
  🔒 *push approval.* *S.* The branch closes a hole found reviewing Phase 4:
  a tenant's `allowOut: ["0.0.0.0/0"]` reached private addresses, which in a
  cluster means the unauthenticated state store holding every sandbox's
  envd access token. Before the fix the cloud metadata address was reachable
  too. Reproduced on real KVM guests; `tools/e2e-egress.sh` fails on the old
  code and passes on the branch.
- [ ] **Pin `dtolnay/rust-toolchain@master`** to a commit. *S.*
  It is unpinned, and it was a silent upstream behaviour change that exposed
  the MSRV defect. Done when: no workflow references an action at `@master`.
- [ ] **Triage the eight Dependabot PRs.** *M.*
  `#91` (base64 0.22→0.23) and `#92` (tower-http 0.6→0.7) are semver-major;
  `#98` bundles fourteen updates at once. Done when: each is merged or closed
  with a reason.
- [ ] **Decide the branch-protection policy.** 🔒 *decision.*
  On 2026-09-23 a direct push bypassed "changes must be made through a pull
  request". Either require it for everyone or record who may bypass and when.

**Exit criterion:** every job on `master` green for three consecutive runs.

---

## Phase B — Make the documentation true 🔓

The recurring defect in this project has been prose that claims more — or
less — than the code does. These are the known instances still outstanding.

- [ ] **Reconcile `security/EXPORT_CONTROL_AUDIT.md` §7.** *S.*
  Four of its five "known issues" are already resolved:
  - [ ] **K-1** custom RSA encrypt/decrypt — deleted; it computed wrong
    results and had no callers.
  - [ ] **K-2** AES-CTR+HMAC fallback — gone; the only remaining mention is a
    comment saying there should not be one.
  - [ ] **K-3** "PQC modules implement API stubs" — false; `pqc.rs` is backed
    by the `ml-kem`, `ml-dsa` and `slh-dsa` crates.
  - [ ] **K-4** vTPM PCR extend uses XOR — fixed in `922761f`; now a SHA-2
    hash chain with property tests.

  Done when: each is marked resolved with the commit that resolved it.
- [ ] **`SECURITY_AUDIT.md` recommendation #3** ("retire the `rsa` timing
  advisory") — done; mark it. *S.*
- [x] **Parity roadmap, Phase 4**: "`hv2-runtime`'s 8 tests are the thinnest
  coverage" — it has 279. Gone in the Phase 4 rewrite, which also records
  why the cluster was not built on `hv2-runtime`.
- [ ] **Upstream: correct the `ic-mldsa` and `ic-mlkem` crates.io
  descriptions.** 🔒 *IronCrypto repository.* *S.*
  They read "incomplete, no signature scheme yet" and "experimental, not
  vector-tested". Both are false — `ic-mldsa` has sign, verify, both prehash
  variants and a deterministic variant, with 55 NIST ACVP vectors; `ic-mlkem`
  has 50. The descriptions cost a recommendation in this repository.
- [ ] **Add a claim-versus-caller check to the sweep.** *M. Optional, high
  leverage.* A script flagging module headers that say "wires", "consults" or
  "enforces" where the named subject has no caller outside its own file.
  Five such headers were found by hand on 2026-09-22.
  Done when: the script fails on a planted false claim and passes on the tree.

**Exit criterion:** the reconciliation list above is empty and
`tools/check-docs-against-code.py` is clean.

---

## Phase C — Crypto consolidation 🔓

One library for all cryptography, and none of it hand-rolled.

- [ ] **Adopt `ic-fips`** for FIPS module policy, self-tests and service
  indicators. *M.*
  This is the layer the repository hand-rolled and got wrong: `kat_aes_gcm`
  was an encrypt-then-decrypt round trip, and `kat_hmac_sha256` discarded its
  MAC. Done when: `FipsMode` self-tests delegate to `ic-fips`, and the
  published-vector tests still pass.
- [ ] **Evaluate `ic-rustls`** as the rustls `CryptoProvider`, replacing
  `ring`. *M.*
  Done when: `cargo tree -i ring` and `cargo tree -i aws-lc-sys` both find
  nothing, TLS tests pass, and `sandbox_proxy.rs` installs the IronCrypto
  provider.
- [ ] **Migrate ML-DSA to `ic-mldsa`.** *M.* It covers 44, 65 and 87 —
  everything `hv2-core` advertises. Done when: the PQC signature tests pass
  against `ic-mldsa`.
- [ ] **ML-KEM: wait.** `ic-mlkem` has 768; `hv2-core` advertises 512, 768 and
  1024. Migrating now would narrow the published API.
- [ ] **SLH-DSA: stay on RustCrypto** — IronCrypto has no FIPS 205 crate.
- [ ] **Replace the `slh-dsa = "=0.2.0-rc.5"` pin** with a released version
  when one exists. *S.* A release candidate in a cryptographic path.
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
- [ ] **Land the tenant/operator split for reserved addresses** (see Phase A).
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
- [ ] **Real secure-boot signature verification.** 🔓 *M.* Define an encoding
  for `Certificate::public_key` — SubjectPublicKeyInfo DER via `ic-pkix` is the
  natural choice — and verify against the **trusted database entry's** key,
  never the one travelling with the signature. Done when: a valid signature
  is admitted, and a forged one with a matching subject string is refused.
- [ ] **Forward audit logs to a tamper-evident store.** *M.* Audit
  recommendation #4.
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
