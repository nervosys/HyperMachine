# Audit Logging

HyperMachine provides comprehensive audit logging for compliance and security.

## Log Format

```json
{
  "timestamp": "2025-01-15T10:30:00.123Z",
  "event": "vm.create",
  "actor": {
    "type": "api_key",
    "id": "key-xxxxx",
    "name": "Production"
  },
  "resource": {
    "type": "vm",
    "id": "vm-550e8400",
    "name": "my-vm"
  },
  "action": "create",
  "result": "success",
  "details": {
    "cpu_cores": 4,
    "memory_mb": 8192
  },
  "source_ip": "192.168.1.100"
}
```

## Configuration

```toml
[audit]
enabled = true
log_path = "/var/log/hypermachine/audit.log"
rotation = "daily"
retention_days = 90
format = "json"

# Events to log
events = [
  "vm.*",
  "snapshot.*",
  "auth.*",
  "admin.*"
]
```

## Log Destinations

- **File** - Local JSON/text files
- **Syslog** - System logging
- **Elasticsearch** - Centralized search
- **CloudWatch** - AWS logging

## Compliance

Supports requirements for:
- SOC 2 Type II
- HIPAA
- PCI DSS
- FedRAMP
