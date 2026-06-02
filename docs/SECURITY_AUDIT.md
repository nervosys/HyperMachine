# HyperMachine — Security & Compliance Audit

**Scope:** HyperMachine agentic hypervisor (Type‑1/Type‑2) and agent runtime
**Audit date:** 2026‑06‑02
**Audit type:** Internal posture self‑assessment (source + dependency review)
**Frameworks:** CVE / RustSec · NIST FIPS (140‑3 / SP 800‑series) · MITRE ATT&CK · CMMC 2.0

> **Important — read first.** This is a **self‑assessment of implemented security
> posture**, not a formal accreditation. HyperMachine is **not** CMVP/FIPS 140‑3
> validated, **not** CMMC‑certified, and has not undergone third‑party penetration
> testing or an Authorization to Operate (ATO). Statements below describe what the
> code implements and how it *maps to* each framework, with gaps called out
> explicitly. Formal compliance requires external assessment by an accredited lab
> (FIPS), a C3PAO (CMMC), and an authorizing official (ATO). Do not represent this
> document as evidence of certification.

---

## 1. Executive Summary

| Framework | Posture | One‑line verdict |
|-----------|---------|------------------|
| **CVE / supply chain** | 🟢 Good | `cargo deny` advisories pass; 5 accepted advisories, all unmaintained/yanked or a non‑exploitable‑in‑context timing issue, each documented with rationale. |
| **NIST FIPS** | 🟡 FIPS‑*approved algorithms*, not FIPS‑*validated* | Uses FIPS‑approved primitives (AES‑GCM, SHA‑2, HMAC, HKDF, RSA, ECDSA) behind a FIPS‑mode gate with power‑up self‑tests, plus NIST PQC (FIPS 203/204/205). No CMVP certificate. |
| **MITRE ATT&CK** | 🟡 Strong primitives, residual surface | Isolation (IOMMU, capability access), audit, tenancy, TLS materially reduce several techniques; a hypervisor remains a high‑value escape target and one timing‑sidechannel risk is accepted. |
| **CMMC 2.0** | 🟡 Supporting controls present | The software supplies building blocks for AC, AU, IA, SC, SI domains; CMMC is an *organizational* certification and cannot be met by software alone. |
| **Dual‑use fitness** | 🟢 Suitable with caveats | Architecturally appropriate for commercial **and** government/defense pilots, *provided* the FIPS‑validation, memory‑encryption‑activation, and advisory‑tracking caveats below are honored. |

**Bottom line for dual‑use deployment:** HyperMachine is built with
defense‑relevant primitives (validated‑algorithm crypto, post‑quantum readiness,
hardware isolation, capability‑based access, audit logging, multi‑tenant
isolation). It is **fit for dual‑use pilots and commercial production today**, and
**fit for accredited/classified environments only after** completing the formal
steps in §6.

---

## 2. CVE / Supply‑Chain Security (RustSec)

Dependency advisories are gated in CI via **`cargo deny`** against the RustSec
advisory DB (`deny.toml`). Current status: **`advisories ok`** — no
unacknowledged vulnerabilities. Five advisories are explicitly accepted with
written justification:

| Advisory | Crate | Nature | Why accepted | Risk |
|----------|-------|--------|--------------|------|
| RUSTSEC‑2023‑0071 | `rsa` | Marvin timing side‑channel (RSA private‑key ops) | Only pure‑Rust RSA with keygen; **no online RSA decryption oracle is exposed**; tracked for migration to a constant‑time release | Low (in context) |
| RUSTSEC‑2026‑0105 | `core2` | Unmaintained / yanked | Transitive via `arboard` clipboard (GUI); no fix exists; no known vuln | Low |
| RUSTSEC‑2024‑0436 | `paste` | Archived | Transitive via `wasmtime`/`cranelift`; no known vuln | Low |
| RUSTSEC‑2025‑0057 | `fxhash` | Unmaintained | Non‑cryptographic hashing only, transitive | Low |
| RUSTSEC‑2025‑0141 | `bincode` 1.x | Unmaintained | Build‑time dependency only | Low |

**Controls in place**
- License allow‑list and dependency bans enforced by `cargo deny` (`[licenses]`, `[bans]`).
- `#![forbid(unsafe_code)]`‑style discipline in business logic; `unsafe` confined to FFI/hardware shims with documented `# Safety` contracts (hypervisor backends, IOMMU, vCPU pinning, NUMA).
- Reproducible builds via pinned `Cargo.lock`; nightly Type‑1 isolated behind `build-std`.

**Gaps / recommendations**
- The accepted `rsa` timing advisory should be retired by migrating RSA keygen/sign to a constant‑time backend (e.g., `aws-lc-rs`) when one supporting keygen is available.
- Integrate scheduled `cargo audit`/`cargo deny` runs (not only PR‑gated) and SBOM generation (CycloneDX) for downstream consumers.
- No third‑party SAST/DAST or fuzz‑at‑scale results are included here; a fuzz harness exists (`fuzz/`) but coverage is not quantified.

---

## 3. NIST FIPS Alignment

### 3.1 What is implemented
- **FIPS‑mode gate** (`crypto::fips::FipsMode` = `Enabled`/`Disabled`) with an
  **approved‑algorithm allow‑list** and **power‑up known‑answer self‑tests**
  (`run_self_tests`) — the architectural pattern FIPS 140‑3 requires.
- **Approved symmetric/hash primitives** via the audited `ring` backend:
  AES‑128/256‑GCM, SHA‑256/384/512, HMAC‑SHA‑2, HKDF.
- **Approved asymmetric primitives:** RSA (FIPS key sizes), ECDSA on NIST curves
  (P‑256/384/521).
- **NIST Post‑Quantum (CNSA 2.0‑relevant):** ML‑KEM (FIPS 203), ML‑DSA
  (FIPS 204), SLH‑DSA (FIPS 205) via RustCrypto — directly relevant to defense
  long‑term‑confidentiality requirements.

### 3.2 Honest limitations
- **Not CMVP‑validated.** Using FIPS‑*approved algorithms* is **not** the same as
  being a FIPS 140‑3 **validated module**. `ring` is not itself on the CMVP
  validated‑module list. A validated deployment requires either linking a
  validated module (e.g., a FIPS build of BoringSSL/OpenSSL/aws‑lc) and running it
  in its validated configuration, or pursuing module validation.
- **PQC implementations** (RustCrypto) are standards‑conformant but not CAVP‑
  certified.
- The **memory‑encryption** layer (AMD SME/SEV/SEV‑ES/SEV‑SNP, Intel TDX) is
  present as a framework/enumeration and is **not fully activated** end‑to‑end;
  treat confidential‑compute as roadmap, not a guarantee.

### 3.3 Mapping to SP 800‑series
- **SP 800‑57 (key management):** key sizes/curves enforced; HKDF for derivation. Key *storage/rotation lifecycle* is application‑responsibility.
- **SP 800‑131A (transitions):** legacy/weak algorithms are not in the approved list; PQC available for quantum‑resistant transition.
- **SP 800‑52 (TLS):** server TLS via `rustls` (modern‑only, no SSLv3/TLS 1.0/1.1).

---

## 4. MITRE ATT&CK Mapping

The hypervisor + agent runtime both **defends against** and (as any compute
substrate) **could be targeted by** ATT&CK techniques. Mapping of implemented
mitigations:

| Tactic | Technique (ID) | Control in HyperMachine | Coverage |
|--------|----------------|--------------------------|----------|
| Privilege Escalation / Defense Evasion | Exploitation for Priv. Esc. (T1068), Escape to Host (T1611) | Hardware isolation: VT‑d / AMD‑Vi **IOMMU** + interrupt remapping; EPT/NPT nested paging; `unsafe` confined + `# Safety`‑documented | Partial — reduces DMA/escape surface; a hypervisor remains a high‑value target |
| Credential Access | Network Sniffing (T1040), AiTM (T1557) | TLS (`rustls`) for API; HMAC payload signing & replay protection middleware | Partial |
| Defense Evasion | Impair Defenses / Disable Logging (T1562.x) | Append‑only, **bounded** MCP audit log + HTTP audit middleware; tamper window minimized | Partial — logs are in‑process; ship to external SIEM for non‑repudiation |
| Discovery / Lateral Movement | within tenant boundary | **Capability‑based access control** (`AgentCapability` least‑privilege) + **multi‑tenant isolation** (`X‑Tenant‑Id`, owner‑only release) | Good for agent layer |
| Initial Access / Execution | Exploit Public‑Facing App (T1190) | Rate limiting, circuit breakers, request‑replay protection, schema validation, bearer auth (defense‑in‑depth middleware stack) | Partial |
| Collection / Exfiltration | timing side‑channels | **Accepted residual:** `rsa` Marvin timing (T1040‑adjacent) — not exposed as an online oracle | Accepted risk |
| Impact | Resource Hijacking (T1496) | Per‑tenant/per‑session quotas, rate limits, capacity reservations, session reclamation | Partial |

**Residual attack surface (be explicit):** a Type‑2 hypervisor inherits host
trust; bare‑metal Type‑1 reduces TCB but is nightly‑only. Guest‑to‑host escape,
side‑channels (Spectre‑class), and supply‑chain remain the dominant categories
and require host hardening, microcode currency, and the confidential‑compute
activation in §3.2.

---

## 5. CMMC 2.0 Mapping

CMMC certifies an **organization’s** handling of FCI/CUI, not a software product.
HyperMachine supplies **technical controls that support** several CMMC domains; the
*People/Process* practices and assessment remain the deploying organization’s
responsibility.

| CMMC Domain | Representative Practices | HyperMachine support | Status |
|-------------|--------------------------|----------------------|--------|
| **AC** Access Control | AC.L2‑3.1.1/.2/.5 (authorized access, least privilege) | Capability‑based agent access, tenant isolation, bearer auth, RBAC primitives | Supporting controls present |
| **AU** Audit & Accountability | AU.L2‑3.3.1/.2 (audit events, traceability) | MCP audit log (session/tool/params/outcome), HTTP audit middleware, W3C trace propagation | Supporting controls present; export to SIEM required |
| **IA** Identification & Auth | IA.L2‑3.5.1/.2 (identify/authenticate) | Bearer‑token auth, per‑agent/session identity, API‑key middleware | Partial — no built‑in MFA/IdP federation |
| **SC** System & Comms Protection | SC.L2‑3.13.8/.11 (encryption in transit, FIPS crypto) | TLS (`rustls`), FIPS‑approved algorithms, PQC; IOMMU isolation | Partial — see FIPS‑validation caveat (§3.2) |
| **SI** System & Info Integrity | SI.L2‑3.14.1 (flaw remediation) | `cargo deny` advisory gating, pinned deps, bounded resource use | Supporting controls present |
| **CM / RA / others** | config mgmt, risk assessment | Reproducible builds, deny/ban policy; this audit | Partial / org‑responsibility |

**Level applicability:** the technical controls above are consistent with **CMMC
Level 1–2** *technical* expectations for FCI and basic CUI handling. **Level 2/3
certification** additionally requires NIST SP 800‑171/172 organizational
practices, a System Security Plan (SSP), POA&M, and a C3PAO assessment — none of
which a codebase can satisfy on its own.

---

## 6. Dual‑Use Deployment Assessment

**Strengths for dual use**
- Cryptographic agility incl. **NIST PQC** (FIPS 203/204/205) — a differentiator for long‑term‑confidentiality (defense/government) use.
- **Hardware isolation** (VT‑d/AMD‑Vi IOMMU, interrupt remapping, nested paging) and a **smaller Type‑1 TCB** option.
- **Least‑privilege + multi‑tenant** agent runtime with audit and automatic resource reclamation.
- Permissive‑plus‑commercial dual licensing (AGPL‑3.0 **or** commercial) supports both open and proprietary/government distribution.

**Pre‑deployment requirements (gating for accredited environments)**
1. **FIPS:** link/run a CMVP‑validated crypto module in its validated config, or pursue validation; do not represent current state as FIPS 140‑3 validated.
2. **Confidential compute:** complete activation/testing of SEV‑SNP / TDX paths before relying on memory encryption for CUI/classified data.
3. **Supply chain:** retire the `rsa` timing advisory; publish an SBOM; add scheduled advisory scans.
4. **Audit:** forward audit logs to an external, tamper‑evident store (SIEM) for non‑repudiation.
5. **Assessment:** independent pen‑test + SSP/POA&M + ATO for government use.

**Verdict:** **Fit for dual‑use deployment** in commercial production and
government/defense **pilots/evaluation** as‑is; **fit for accredited or classified
production after** items 1–5. No export‑controlled functionality (e.g., classified
algorithms) is present; standard NIST/open cryptography only — but deployers
remain responsible for their own EAR/ITAR determination.

---

## 7. Methodology & Limitations

- **Methodology:** source review of the crypto, IOMMU, security, middleware, MCP,
  and agent‑runtime modules; `cargo deny` advisory/license/ban evaluation;
  mapping of implemented controls to each framework’s published practices.
- **Not performed:** third‑party penetration testing, dynamic analysis, formal
  verification, side‑channel measurement, CMVP/CAVP testing, or organizational
  (CMMC People/Process) assessment.
- **Validity:** reflects the repository state on the audit date; re‑audit on
  dependency or architecture changes. This document is **informational** and is
  **not** a certificate, warranty, or authorization to operate.
