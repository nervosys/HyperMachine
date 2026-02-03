# HyperMachine Deployment Guide

Production deployment guide for HyperMachine Type-2 Hypervisor.

## Table of Contents

1. [System Requirements](#system-requirements)
2. [Installation](#installation)
3. [Configuration](#configuration)
4. [Security Hardening](#security-hardening)
5. [High Availability](#high-availability)
6. [Monitoring](#monitoring)
7. [Troubleshooting](#troubleshooting)

---

## System Requirements

### Minimum Requirements

| Component | Requirement |
|-----------|-------------|
| CPU | x86_64 with VT-x/AMD-V |
| RAM | 8 GB (+ VM allocations) |
| Storage | 50 GB SSD |
| OS | Windows 10/11, Windows Server 2019+ |
| Features | Hyper-V Platform enabled |

### Recommended for Production

| Component | Requirement |
|-----------|-------------|
| CPU | 16+ cores with VT-d/AMD-Vi |
| RAM | 64 GB+ ECC |
| Storage | NVMe SSD, RAID configuration |
| Network | 10 Gbps, SR-IOV capable |
| GPU | NVIDIA/AMD with passthrough support |

### Hardware Virtualization Check

```powershell
# Check CPU virtualization support
systeminfo | Select-String "Hyper-V"

# Verify WHPX availability
Get-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform
```

---

## Installation

### From Binary Release

```powershell
# Download latest release
Invoke-WebRequest -Uri "https://github.com/nervosys/HyperMachine/releases/latest/download/hypermachine-windows-x64.zip" -OutFile hypermachine.zip

# Extract
Expand-Archive hypermachine.zip -DestinationPath C:\HyperMachine

# Install as service
C:\HyperMachine\hypermachine.exe install --service
```

### From Source

```powershell
# Clone repository
git clone https://github.com/nervosys/HyperMachine.git
cd HyperMachine

# Build release
cargo build --release -p hv2-core -p hv2-api

# Copy binaries
Copy-Item target\release\*.exe C:\HyperMachine\
```

### Docker Deployment

```yaml
# docker-compose.yml
version: '3.8'
services:
  hypermachine:
    image: nervosys/hypermachine:latest
    privileged: true
    ports:
      - "8080:8080"
      - "50051:50051"  # gRPC
    volumes:
      - /dev/kvm:/dev/kvm  # Linux only
      - ./config:/etc/hypermachine
      - ./data:/var/lib/hypermachine
    environment:
      - HM_LOG_LEVEL=info
      - HM_API_KEY=${API_KEY}
```

---

## Configuration

### Configuration File

Create `/etc/hypermachine/config.toml` (Linux) or `C:\HyperMachine\config.toml` (Windows):

```toml
[server]
# API server configuration
bind_address = "0.0.0.0"
http_port = 8080
grpc_port = 50051
tls_enabled = true
tls_cert = "/etc/hypermachine/cert.pem"
tls_key = "/etc/hypermachine/key.pem"

[auth]
# Authentication settings
enabled = true
api_key_header = "X-API-Key"
jwt_secret = "your-256-bit-secret"
jwt_expiry_hours = 24

[vm]
# VM defaults
default_cpus = 2
default_memory_mb = 2048
max_vms = 100
storage_path = "/var/lib/hypermachine/vms"

[network]
# Network configuration
default_network = "nat"
bridge_interface = "hm-bridge0"
dhcp_range = "10.0.0.100-10.0.0.200"

[security]
# Security features
secure_boot_default = true
vtpm_default = true
memory_encryption = "sev"  # sev, tdx, or none

[telemetry]
# Metrics and logging
log_level = "info"
metrics_enabled = true
metrics_port = 9090
prometheus_path = "/metrics"

[agent]
# AI agent configuration
ontology_enabled = true
rate_limit_rpm = 600
allowed_origins = ["*"]
```

### Environment Variables

```bash
# Override any config via environment
export HM_SERVER_HTTP_PORT=8080
export HM_AUTH_ENABLED=true
export HM_AUTH_API_KEY="your-api-key"
export HM_VM_STORAGE_PATH="/data/vms"
export HM_LOG_LEVEL="debug"
```

---

## Security Hardening

### TLS Configuration

```bash
# Generate self-signed certificate (development)
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes

# Production: Use Let's Encrypt or enterprise CA
```

### API Authentication

```toml
[auth]
enabled = true
methods = ["api_key", "jwt", "mtls"]

# API Key configuration
[auth.api_key]
header = "X-API-Key"
keys = [
    { key = "hm_prod_xxx", name = "production", permissions = ["*"] },
    { key = "hm_ro_xxx", name = "readonly", permissions = ["read"] },
]

# JWT configuration
[auth.jwt]
issuer = "hypermachine"
audience = "hypermachine-api"
secret_file = "/etc/hypermachine/jwt.secret"

# mTLS configuration
[auth.mtls]
ca_cert = "/etc/hypermachine/ca.pem"
require_client_cert = true
```

### Firewall Rules

```powershell
# Windows Firewall
New-NetFirewallRule -DisplayName "HyperMachine API" -Direction Inbound -Protocol TCP -LocalPort 8080 -Action Allow
New-NetFirewallRule -DisplayName "HyperMachine gRPC" -Direction Inbound -Protocol TCP -LocalPort 50051 -Action Allow
New-NetFirewallRule -DisplayName "HyperMachine Metrics" -Direction Inbound -Protocol TCP -LocalPort 9090 -Action Allow -RemoteAddress "10.0.0.0/8"
```

### Security Checklist

- [ ] TLS enabled for all endpoints
- [ ] API authentication configured
- [ ] Rate limiting enabled
- [ ] Audit logging enabled
- [ ] Secure boot enabled for VMs
- [ ] vTPM enabled for sensitive workloads
- [ ] Memory encryption (SEV/TDX) for confidential computing
- [ ] Network isolation configured
- [ ] Regular security updates applied

---

## High Availability

### Active-Passive Setup

```yaml
# HAProxy configuration
frontend hypermachine
    bind *:8080 ssl crt /etc/ssl/hypermachine.pem
    default_backend hm_servers

backend hm_servers
    option httpchk GET /health
    server hm1 10.0.1.10:8080 check
    server hm2 10.0.1.11:8080 check backup
```

### Shared Storage

For VM migration support:

```toml
[storage]
type = "shared"
backend = "ceph"  # ceph, nfs, iscsi
ceph_pool = "hypermachine-vms"
ceph_conf = "/etc/ceph/ceph.conf"
```

### State Replication

```toml
[cluster]
enabled = true
node_id = "hm-node-1"
peers = ["10.0.1.11:7946", "10.0.1.12:7946"]
consensus = "raft"
```

---

## Monitoring

### Prometheus Metrics

Metrics available at `http://localhost:9090/metrics`:

```
# VM metrics
hypermachine_vm_count{state="running"} 10
hypermachine_vm_cpu_usage_percent{vm_id="vm-123"} 45.2
hypermachine_vm_memory_used_bytes{vm_id="vm-123"} 2147483648

# API metrics
hypermachine_api_requests_total{method="POST",path="/api/v1/vms"} 1234
hypermachine_api_latency_seconds{quantile="0.99"} 0.150

# System metrics
hypermachine_host_cpu_usage_percent 32.5
hypermachine_host_memory_available_bytes 34359738368
```

### Grafana Dashboard

Import dashboard ID: `12345` from Grafana.com or use [grafana-dashboard.json](./monitoring/grafana-dashboard.json).

### Alerting Rules

```yaml
# prometheus-alerts.yml
groups:
  - name: hypermachine
    rules:
      - alert: HighVMCount
        expr: hypermachine_vm_count > 90
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Approaching VM limit"

      - alert: APILatencyHigh
        expr: hypermachine_api_latency_seconds{quantile="0.99"} > 1
        for: 5m
        labels:
          severity: warning

      - alert: VMUnhealthy
        expr: hypermachine_vm_health{state="unhealthy"} > 0
        for: 1m
        labels:
          severity: critical
```

### Log Aggregation

```toml
[logging]
format = "json"
output = "stdout"
level = "info"

# Forward to external systems
[logging.forward]
enabled = true
type = "fluentd"  # fluentd, loki, elasticsearch
endpoint = "http://fluentd:24224"
```

---

## Troubleshooting

### Common Issues

#### "Hypervisor not available"

```powershell
# Enable Hyper-V Platform
Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All

# Reboot required
Restart-Computer
```

#### "VM creation failed: insufficient resources"

```powershell
# Check available memory
Get-Process | Sort-Object -Property WorkingSet64 -Descending | Select-Object -First 10

# Check disk space
Get-WmiObject Win32_LogicalDisk | Select-Object DeviceID, FreeSpace, Size
```

#### "API connection refused"

```powershell
# Check service status
Get-Service HyperMachine

# Check port binding
netstat -an | findstr 8080

# Check logs
Get-Content C:\HyperMachine\logs\hypermachine.log -Tail 100
```

### Debug Mode

```powershell
# Run in debug mode
$env:HM_LOG_LEVEL = "debug"
$env:RUST_BACKTRACE = "1"
C:\HyperMachine\hypermachine.exe serve
```

### Health Check Endpoints

```bash
# Basic health
curl http://localhost:8080/health

# Detailed status
curl http://localhost:8080/health/detailed

# Readiness (for k8s)
curl http://localhost:8080/health/ready

# Liveness (for k8s)
curl http://localhost:8080/health/live
```

---

## Support

- **Documentation**: https://hypermachine.dev/docs
- **GitHub Issues**: https://github.com/nervosys/HyperMachine/issues
- **Enterprise Support**: support@nervosys.com
- **Security Issues**: security@nervosys.com
