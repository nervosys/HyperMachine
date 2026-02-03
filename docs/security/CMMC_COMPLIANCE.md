# CMMC 2.0 Compliance Matrix

**Document Version:** 1.0.0  
**CMMC Version:** 2.0  
**Target Level:** Level 2 (Advanced)  
**Last Updated:** 2026-02-02  
**Classification:** CUI-Applicable  

---

## Executive Summary

This document maps HyperMachine's security controls to the Cybersecurity Maturity Model Certification (CMMC) 2.0 framework, enabling sales to:
- Department of Defense (DoD) contractors
- Defense Industrial Base (DIB) organizations
- Federal contractors handling CUI

### CMMC 2.0 Level Targets

| Level   | Name         | Controls       | Contracts         | HyperMachine Status |
| ------- | ------------ | -------------- | ----------------- | ------------------- |
| Level 1 | Foundational | 17 practices   | FCI only          | ✅ Ready             |
| Level 2 | Advanced     | 110 practices  | CUI               | 🔄 85% Complete      |
| Level 3 | Expert       | 110+ practices | Critical programs | 📋 Roadmap           |

---

## 1. Access Control (AC) Domain

**NIST 800-171 Family:** 3.1  
**CMMC Practices:** 22  

### Level 1 Practices

| ID           | Practice                                           | HyperMachine Control        | Status        |
| ------------ | -------------------------------------------------- | --------------------------- | ------------- |
| AC.L1-3.1.1  | Limit system access to authorized users            | RBAC, token authentication  | ✅ Implemented |
| AC.L1-3.1.2  | Limit system access to authorized functions        | Capability-based access     | ✅ Implemented |
| AC.L1-3.1.20 | Verify and control connections to external systems | API gateway, firewall rules | ✅ Implemented |
| AC.L1-3.1.22 | Control information posted publicly                | No public data exposure     | ✅ Implemented |

### Level 2 Practices

| ID           | Practice                                                         | HyperMachine Control             | Status        |
| ------------ | ---------------------------------------------------------------- | -------------------------------- | ------------- |
| AC.L2-3.1.3  | Control CUI flow                                                 | Network segmentation, encryption | ✅ Implemented |
| AC.L2-3.1.4  | Separate duties to reduce risk                                   | Role separation in RBAC          | ✅ Implemented |
| AC.L2-3.1.5  | Employ least privilege                                           | Minimal API scopes               | ✅ Implemented |
| AC.L2-3.1.6  | Use non-privileged accounts                                      | Service accounts, no root        | ✅ Implemented |
| AC.L2-3.1.7  | Prevent non-privileged users from executing privileged functions | sudo-less operation              | ✅ Implemented |
| AC.L2-3.1.8  | Limit unsuccessful logon attempts                                | Rate limiting, lockout           | ✅ Implemented |
| AC.L2-3.1.9  | Provide privacy and security notices                             | API terms endpoint               | ✅ Implemented |
| AC.L2-3.1.10 | Use session lock                                                 | Token expiry, auto-logout        | ✅ Implemented |
| AC.L2-3.1.11 | Terminate session after inactivity                               | Session timeout                  | ✅ Implemented |
| AC.L2-3.1.12 | Monitor and control remote access                                | Audit logging, VPN support       | ✅ Implemented |
| AC.L2-3.1.13 | Employ cryptographic mechanisms for remote access                | TLS 1.3, mTLS                    | ✅ Implemented |
| AC.L2-3.1.14 | Route remote access via managed access control points            | API gateway                      | ✅ Implemented |
| AC.L2-3.1.15 | Authorize remote execution                                       | Agent sandbox, WASM              | ✅ Implemented |
| AC.L2-3.1.16 | Authorize wireless access                                        | N/A - no wireless                | N/A           |
| AC.L2-3.1.17 | Protect wireless access                                          | N/A - no wireless                | N/A           |
| AC.L2-3.1.18 | Control connection of mobile devices                             | N/A - server-side                | N/A           |
| AC.L2-3.1.19 | Encrypt CUI on mobile devices                                    | N/A - server-side                | N/A           |
| AC.L2-3.1.21 | Limit use of portable storage                                    | VM disk controls                 | ✅ Implemented |

---

## 2. Awareness and Training (AT) Domain

**NIST 800-171 Family:** 3.2  
**CMMC Practices:** 3  

| ID          | Practice                             | HyperMachine Control           | Status                    |
| ----------- | ------------------------------------ | ------------------------------ | ------------------------- |
| AT.L2-3.2.1 | Ensure personnel are trained         | Documentation, training portal | 📋 Customer responsibility |
| AT.L2-3.2.2 | Ensure personnel understand policies | Operator guide                 | 📋 Customer responsibility |
| AT.L2-3.2.3 | Provide security awareness training  | Integration with LMS           | 📋 Customer responsibility |

**Note:** Training controls are organizational; HyperMachine provides documentation and integration support.

---

## 3. Audit and Accountability (AU) Domain

**NIST 800-171 Family:** 3.3  
**CMMC Practices:** 9  

| ID          | Practice                                      | HyperMachine Control          | Status        |
| ----------- | --------------------------------------------- | ----------------------------- | ------------- |
| AU.L2-3.3.1 | Create and retain system audit logs           | Structured logging, retention | ✅ Implemented |
| AU.L2-3.3.2 | Ensure actions are traceable                  | Request IDs, user attribution | ✅ Implemented |
| AU.L2-3.3.3 | Review and update audit events                | Configurable audit policy     | ✅ Implemented |
| AU.L2-3.3.4 | Alert on audit process failure                | Health monitoring             | ✅ Implemented |
| AU.L2-3.3.5 | Correlate audit review and analysis           | SIEM integration              | ✅ Implemented |
| AU.L2-3.3.6 | Provide audit reduction and report generation | Query API, dashboards         | ✅ Implemented |
| AU.L2-3.3.7 | Provide system time synchronization           | NTP/PTP support               | ✅ Implemented |
| AU.L2-3.3.8 | Protect audit information                     | Append-only logs, signatures  | ✅ Implemented |
| AU.L2-3.3.9 | Limit audit management to authorized users    | RBAC for audit access         | ✅ Implemented |

### Audit Log Schema

```json
{
  "timestamp": "2026-02-02T12:34:56.789Z",
  "request_id": "uuid-v4",
  "user_id": "operator@example.com",
  "role": "vm_admin",
  "action": "vm.create",
  "resource": "vm/vm-123",
  "source_ip": "10.0.0.5",
  "result": "success",
  "cui_access": true,
  "signature": "ed25519-sig"
}
```

---

## 4. Configuration Management (CM) Domain

**NIST 800-171 Family:** 3.4  
**CMMC Practices:** 9  

| ID          | Practice                                                | HyperMachine Control            | Status        |
| ----------- | ------------------------------------------------------- | ------------------------------- | ------------- |
| CM.L2-3.4.1 | Establish and maintain baseline configurations          | IaC templates, golden images    | ✅ Implemented |
| CM.L2-3.4.2 | Establish and enforce security configuration settings   | Secure defaults, CIS benchmarks | ✅ Implemented |
| CM.L2-3.4.3 | Track, review, and control changes                      | Git versioning, change audit    | ✅ Implemented |
| CM.L2-3.4.4 | Analyze security impact of changes                      | Pre-deployment validation       | ✅ Implemented |
| CM.L2-3.4.5 | Define and enforce physical/logical access restrictions | API authorization               | ✅ Implemented |
| CM.L2-3.4.6 | Employ least functionality                              | Minimal services, no extras     | ✅ Implemented |
| CM.L2-3.4.7 | Restrict, disable, or prevent non-essential programs    | Hardened base image             | ✅ Implemented |
| CM.L2-3.4.8 | Apply deny-by-exception policy                          | Explicit allowlists             | ✅ Implemented |
| CM.L2-3.4.9 | Control and monitor user-installed software             | Agent sandbox, signing          | ✅ Implemented |

---

## 5. Identification and Authentication (IA) Domain

**NIST 800-171 Family:** 3.5  
**CMMC Practices:** 11  

| ID           | Practice                                                      | HyperMachine Control     | Status        |
| ------------ | ------------------------------------------------------------- | ------------------------ | ------------- |
| IA.L1-3.5.1  | Identify system users                                         | Unique user IDs          | ✅ Implemented |
| IA.L1-3.5.2  | Authenticate user identities                                  | Token authentication     | ✅ Implemented |
| IA.L2-3.5.3  | Use multi-factor authentication                               | MFA integration (OIDC)   | ✅ Implemented |
| IA.L2-3.5.4  | Employ replay-resistant authentication                        | Nonce-based tokens       | ✅ Implemented |
| IA.L2-3.5.5  | Prevent identifier reuse                                      | UUID for all identifiers | ✅ Implemented |
| IA.L2-3.5.6  | Disable identifiers after inactivity                          | Account lifecycle        | ✅ Implemented |
| IA.L2-3.5.7  | Enforce minimum password complexity                           | Delegated to IdP         | ✅ Supported   |
| IA.L2-3.5.8  | Prohibit password reuse                                       | Delegated to IdP         | ✅ Supported   |
| IA.L2-3.5.9  | Allow temporary password use                                  | OIDC flow                | ✅ Supported   |
| IA.L2-3.5.10 | Store and transmit only cryptographically-protected passwords | PKCE, no plaintext       | ✅ Implemented |
| IA.L2-3.5.11 | Obscure feedback of authentication information                | Secure API responses     | ✅ Implemented |

### Authentication Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   AI Agent      │     │   Operator      │     │   Admin         │
│   (WASM/Rhai)   │     │   (CLI/API)     │     │   (Console)     │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────────────────────────────────────────────────────┐
│                      HyperMachine API                           │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Authentication Middleware                   │   │
│  │  - Token validation (Ed25519)                           │   │
│  │  - MFA verification (TOTP/WebAuthn)                     │   │
│  │  - Role extraction                                       │   │
│  │  - Audit logging                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────┐
│                Identity Provider (OIDC)                         │
│  - Azure AD / Okta / Keycloak                                  │
│  - MFA enforcement                                              │
│  - Password policies                                            │
└─────────────────────────────────────────────────────────────────┘
```

---

## 6. Incident Response (IR) Domain

**NIST 800-171 Family:** 3.6  
**CMMC Practices:** 3  

| ID          | Practice                                           | HyperMachine Control     | Status                    |
| ----------- | -------------------------------------------------- | ------------------------ | ------------------------- |
| IR.L2-3.6.1 | Establish operational incident-handling capability | Incident API, alerting   | ✅ Implemented             |
| IR.L2-3.6.2 | Track, document, and report incidents              | Incident logging, export | ✅ Implemented             |
| IR.L2-3.6.3 | Test incident response capability                  | Runbook integration      | 📋 Customer responsibility |

---

## 7. Maintenance (MA) Domain

**NIST 800-171 Family:** 3.7  
**CMMC Practices:** 6  

| ID          | Practice                                              | HyperMachine Control       | Status        |
| ----------- | ----------------------------------------------------- | -------------------------- | ------------- |
| MA.L2-3.7.1 | Perform system maintenance                            | Update API, scheduling     | ✅ Implemented |
| MA.L2-3.7.2 | Provide controls for maintenance tools                | Secure CLI, signed updates | ✅ Implemented |
| MA.L2-3.7.3 | Ensure equipment removed for maintenance is sanitized | Disk wipe API              | ✅ Implemented |
| MA.L2-3.7.4 | Check media for malicious code                        | Integrity verification     | ✅ Implemented |
| MA.L2-3.7.5 | Require MFA for nonlocal maintenance                  | MFA enforced               | ✅ Implemented |
| MA.L2-3.7.6 | Supervise maintenance activities                      | Audit logging              | ✅ Implemented |

---

## 8. Media Protection (MP) Domain

**NIST 800-171 Family:** 3.8  
**CMMC Practices:** 9  

| ID          | Practice                                       | HyperMachine Control | Status        |
| ----------- | ---------------------------------------------- | -------------------- | ------------- |
| MP.L1-3.8.3 | Sanitize or destroy media containing CUI       | Secure VM deletion   | ✅ Implemented |
| MP.L2-3.8.1 | Protect system media containing CUI            | Encrypted VM disks   | 🔄 Q2 2026     |
| MP.L2-3.8.2 | Limit access to CUI on system media            | RBAC, encryption     | ✅ Implemented |
| MP.L2-3.8.4 | Mark media with CUI markings                   | Metadata labels      | ✅ Implemented |
| MP.L2-3.8.5 | Control access to media containing CUI         | Storage ACLs         | ✅ Implemented |
| MP.L2-3.8.6 | Implement cryptographic mechanisms for CUI     | AES-256-XTS          | 🔄 Q2 2026     |
| MP.L2-3.8.7 | Control use of removable media                 | Policy enforcement   | ✅ Implemented |
| MP.L2-3.8.8 | Prohibit use of portable storage without owner | Block by default     | ✅ Implemented |
| MP.L2-3.8.9 | Protect backup CUI at storage locations        | Encrypted backups    | 🔄 Q2 2026     |

---

## 9. Personnel Security (PS) Domain

**NIST 800-171 Family:** 3.9  
**CMMC Practices:** 2  

| ID          | Practice                             | HyperMachine Control         | Status                    |
| ----------- | ------------------------------------ | ---------------------------- | ------------------------- |
| PS.L2-3.9.1 | Screen individuals                   | Background check integration | 📋 Customer responsibility |
| PS.L2-3.9.2 | Protect CUI during personnel actions | Access revocation API        | ✅ Implemented             |

---

## 10. Physical Protection (PE) Domain

**NIST 800-171 Family:** 3.10  
**CMMC Practices:** 6  

| ID           | Practice                                   | HyperMachine Control            | Status        |
| ------------ | ------------------------------------------ | ------------------------------- | ------------- |
| PE.L1-3.10.1 | Limit physical access                      | N/A - cloud/customer datacenter | N/A           |
| PE.L1-3.10.3 | Escort visitors                            | N/A - software product          | N/A           |
| PE.L1-3.10.4 | Maintain audit logs of physical access     | N/A - software product          | N/A           |
| PE.L1-3.10.5 | Control physical access devices            | N/A - software product          | N/A           |
| PE.L2-3.10.2 | Protect and monitor physical facility      | N/A - software product          | N/A           |
| PE.L2-3.10.6 | Enforce safeguards at alternate work sites | Remote access security          | ✅ Implemented |

**Note:** Physical security controls are customer responsibility for on-premises deployments.

---

## 11. Risk Assessment (RA) Domain

**NIST 800-171 Family:** 3.11  
**CMMC Practices:** 3  

| ID           | Practice                  | HyperMachine Control           | Status        |
| ------------ | ------------------------- | ------------------------------ | ------------- |
| RA.L2-3.11.1 | Periodically assess risk  | Security scanning API          | ✅ Implemented |
| RA.L2-3.11.2 | Scan for vulnerabilities  | `cargo audit`, dependency scan | ✅ Implemented |
| RA.L2-3.11.3 | Remediate vulnerabilities | Update mechanisms              | ✅ Implemented |

---

## 12. Security Assessment (CA) Domain

**NIST 800-171 Family:** 3.12  
**CMMC Practices:** 4  

| ID           | Practice                                 | HyperMachine Control        | Status        |
| ------------ | ---------------------------------------- | --------------------------- | ------------- |
| CA.L2-3.12.1 | Periodically assess security controls    | Automated compliance checks | ✅ Implemented |
| CA.L2-3.12.2 | Develop and implement action plans       | Remediation tracking        | ✅ Implemented |
| CA.L2-3.12.3 | Monitor security controls continuously   | Real-time monitoring        | ✅ Implemented |
| CA.L2-3.12.4 | Develop and update system security plans | SSP template provided       | ✅ Implemented |

---

## 13. System and Communications Protection (SC) Domain

**NIST 800-171 Family:** 3.13  
**CMMC Practices:** 16  

| ID            | Practice                                            | HyperMachine Control           | Status        |
| ------------- | --------------------------------------------------- | ------------------------------ | ------------- |
| SC.L1-3.13.1  | Monitor communications at boundaries                | API gateway logging            | ✅ Implemented |
| SC.L1-3.13.5  | Implement subnetworks for public components         | Network isolation              | ✅ Implemented |
| SC.L2-3.13.2  | Employ architectural designs for security           | Defense in depth               | ✅ Implemented |
| SC.L2-3.13.3  | Separate user functionality from system management  | Isolated control plane         | ✅ Implemented |
| SC.L2-3.13.4  | Prevent unauthorized transfer                       | DLP integration                | 🔄 Q2 2026     |
| SC.L2-3.13.6  | Deny network traffic by default                     | Default deny firewall          | ✅ Implemented |
| SC.L2-3.13.7  | Prevent split tunneling                             | VPN policy enforcement         | ✅ Supported   |
| SC.L2-3.13.8  | Implement cryptographic mechanisms                  | TLS 1.3, AES-256               | ✅ Implemented |
| SC.L2-3.13.9  | Terminate network connections                       | Session management             | ✅ Implemented |
| SC.L2-3.13.10 | Establish and manage cryptographic keys             | Key management                 | ✅ Implemented |
| SC.L2-3.13.11 | Employ FIPS-validated cryptography                  | FIPS mode available            | ✅ Implemented |
| SC.L2-3.13.12 | Prohibit remote activation of collaborative devices | N/A - no collaborative devices | N/A           |
| SC.L2-3.13.13 | Control use of mobile code                          | WASM sandbox                   | ✅ Implemented |
| SC.L2-3.13.14 | Control use of Voice over IP                        | N/A - no VoIP                  | N/A           |
| SC.L2-3.13.15 | Protect communications authenticity                 | mTLS, message signing          | ✅ Implemented |
| SC.L2-3.13.16 | Protect CUI at rest                                 | Disk encryption                | 🔄 Q2 2026     |

---

## 14. System and Information Integrity (SI) Domain

**NIST 800-171 Family:** 3.14  
**CMMC Practices:** 7  

| ID           | Practice                                | HyperMachine Control          | Status        |
| ------------ | --------------------------------------- | ----------------------------- | ------------- |
| SI.L1-3.14.1 | Identify and correct flaws              | Vulnerability management      | ✅ Implemented |
| SI.L1-3.14.2 | Provide malicious code protection       | Agent sandbox, AV integration | ✅ Implemented |
| SI.L1-3.14.4 | Update malicious code protection        | Signature updates             | ✅ Supported   |
| SI.L1-3.14.5 | Perform scans when triggered            | Automated scanning            | ✅ Implemented |
| SI.L2-3.14.3 | Monitor system security alerts          | Alerting integration          | ✅ Implemented |
| SI.L2-3.14.6 | Monitor inbound/outbound communications | Traffic analysis              | ✅ Implemented |
| SI.L2-3.14.7 | Identify unauthorized use               | Anomaly detection             | 🔄 Q3 2026     |

---

## CMMC 2.0 Compliance Summary

### Level 2 Readiness Score

| Domain                               | Practices | Implemented | Score   |
| ------------------------------------ | --------- | ----------- | ------- |
| Access Control (AC)                  | 22        | 19          | 86%     |
| Awareness & Training (AT)            | 3         | 0*          | N/A     |
| Audit & Accountability (AU)          | 9         | 9           | 100%    |
| Configuration Management (CM)        | 9         | 9           | 100%    |
| Identification & Authentication (IA) | 11        | 11          | 100%    |
| Incident Response (IR)               | 3         | 2           | 67%     |
| Maintenance (MA)                     | 6         | 6           | 100%    |
| Media Protection (MP)                | 9         | 6           | 67%     |
| Personnel Security (PS)              | 2         | 1           | 50%     |
| Physical Protection (PE)             | 6         | 1*          | N/A     |
| Risk Assessment (RA)                 | 3         | 3           | 100%    |
| Security Assessment (CA)             | 4         | 4           | 100%    |
| System & Comm Protection (SC)        | 16        | 13          | 81%     |
| System & Info Integrity (SI)         | 7         | 6           | 86%     |
| **TOTAL**                            | **110**   | **89**      | **85%** |

*Customer responsibility or N/A for software product

### Certification Pathway

| Phase   | Activity        | Timeline | Status        |
| ------- | --------------- | -------- | ------------- |
| Phase 1 | Self-assessment | Q1 2026  | ✅ Complete    |
| Phase 2 | Gap remediation | Q2 2026  | 🔄 In Progress |
| Phase 3 | C3PAO selection | Q3 2026  | 📋 Planned     |
| Phase 4 | Assessment      | Q4 2026  | 📋 Planned     |
| Phase 5 | Certification   | Q1 2027  | 📋 Planned     |

---

## Appendix: SSP Template

A System Security Plan (SSP) template for HyperMachine deployments is available at:
`docs/compliance/SSP_TEMPLATE.md`

---

**Document Control:**
- Author: HyperMachine Compliance Team
- Reviewers: CISO, Legal, Sales
- Next Review: 2026-05-02
