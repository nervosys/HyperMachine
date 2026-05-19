# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.3.x   | :white_check_mark: |
| 0.2.x   | :white_check_mark: |
| 0.1.x   | :x:                |

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability in HyperMachine, please report it responsibly.

### How to Report

1. **Do not** open a public GitHub issue for security vulnerabilities.
2. Email your findings to **security@nervosys.ai**.
3. Include a detailed description of the vulnerability, steps to reproduce, and potential impact.
4. If possible, include a proof-of-concept or minimal reproduction.

### What to Expect

- **Acknowledgment**: We will acknowledge receipt within 48 hours.
- **Assessment**: We will assess the vulnerability and provide an initial response within 5 business days.
- **Resolution**: Critical vulnerabilities will be patched as quickly as possible. We aim to release fixes within 30 days of confirmation.
- **Disclosure**: We follow coordinated disclosure. We will work with you on timing and credit.

### Scope

The following are in scope:
- All code in the `crates/` directory
- CI/CD pipeline configurations
- Cryptographic implementations (`hv2-core` crypto module)

### Out of Scope

- Third-party dependencies (report upstream, but let us know)
- Issues in the `reference/` directory (external reference code)
- Denial of service via resource exhaustion in development/test configurations

## Security Measures

HyperMachine employs several security practices:

- **Dependency auditing**: Automated via `cargo-deny` ([deny.toml](deny.toml)) and Dependabot
- **FIPS 140-3 compliance**: Optional FIPS-validated cryptography via the `fips` feature flag
- **Seccomp filtering**: System call restrictions for sandboxed VMs
- **Memory safety**: Written in Rust with minimal `unsafe` usage, audited via `cargo-deny`
- **CI security checks**: Automated `cargo-audit` and `cargo-deny` in the security workflow
