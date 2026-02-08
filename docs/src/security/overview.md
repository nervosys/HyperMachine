# Security Overview

HyperMachine is designed with security as a first-class concern.

## Security Model

```
┌─────────────────────────────────────────────────────────┐
│                    API Layer                             │
│     Authentication • Rate Limiting • Input Validation   │
├─────────────────────────────────────────────────────────┤
│                 Access Control                           │
│       Capabilities • Resource Quotas • Policies         │
├─────────────────────────────────────────────────────────┤
│                  VM Isolation                            │
│      Memory • Network • Filesystem • Devices            │
├─────────────────────────────────────────────────────────┤
│                 Cryptography                             │
│     FIPS 140-3 • Key Management • Secure Boot          │
├─────────────────────────────────────────────────────────┤
│                  Audit System                            │
│       Logging • Monitoring • Compliance                 │
└─────────────────────────────────────────────────────────┘
```

## Key Security Features

| Feature                     | Description                        |
| --------------------------- | ---------------------------------- |
| **FIPS 140-3 Cryptography** | Government-grade encryption        |
| **Seccomp Filtering**       | Restrict syscalls available to VMs |
| **Capability-based Access** | Fine-grained permission control    |
| **Memory Isolation**        | EPT/NPT hardware isolation         |
| **Network Isolation**       | Per-VM network namespaces          |
| **Audit Logging**           | Complete operation history         |

## Threat Model

### Protected Against

- **VM Escape** - Hardware-assisted isolation (EPT/NPT)
- **Side-channel Attacks** - CPU isolation, memory randomization
- **Network Attacks** - Isolated virtual networks, firewall
- **DMA Attacks** - IOMMU/VT-d isolation
- **Privilege Escalation** - Minimal host kernel exposure

### Trust Boundaries

1. **API Layer** - Authentication, input validation
2. **Hypervisor Core** - VM isolation
3. **Hardware** - CPU virtualization extensions

## Security Profiles

```bash
# High security (production)
hm t2 create --name secure-vm --security-profile high

# AI sandbox (isolated but network-enabled)
hm t2 create --name ai-vm --security-profile ai-sandbox
```

### Profile Definitions

**high:**
- Seccomp enabled (strict)
- No network access
- Read-only root filesystem
- Audit all operations

**ai-sandbox:**
- Seccomp enabled
- Network allowed (monitored)
- CPU/memory limits
- Full audit logging

## Next Steps

- [Cryptography](./cryptography.md) - Encryption details
- [Access Control](./access-control.md) - Permission system
- [Audit Logging](./audit-logging.md) - Compliance logging
