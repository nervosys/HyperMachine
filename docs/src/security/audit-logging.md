# Audit Logging

The API server can record every request it serves. What it records is an HTTP
access record, not a resource-level event: read this page before relying on it
for anything that needs to say *who* did *what to which object*.

## What an entry contains

Emitted as one JSON object per request, through `tracing::info!` on the target
`audit_log`:

```json
{
  "timestamp": "1789500000",
  "method": "POST",
  "path": "/api/v1/vms",
  "status": 201,
  "duration_ms": 42,
  "request_id": "01J...",
  "client_ip": "192.168.1.100",
  "request_body": "{\"name\":\"my-vm\"}"
}
```

`timestamp` is Unix seconds as a string. `status`, `request_body` and the rest
are present according to the settings below; a field that is switched off is
omitted.

**There is no actor and no resource.** The entry does not name the API key that
authenticated, the identity behind it, or the object the request acted on. A
request's `path` and `request_body` are what identify it after the fact.

## Configuration

```toml
[middleware]
enable_audit_log = true
audit_log_request_body = true
audit_log_response_status = true
audit_log_max_body_bytes = 4096

[middleware.body_limit]
max_bytes = 1048576
```

`audit_log_excluded_paths` takes path prefixes to leave unrecorded — health
checks, mostly, which would otherwise dominate the log.

## Destination

The process log, and nowhere else. Entries go to `tracing` on the `audit_log`
target, so they are interleaved with everything else the server emits and are
separated by filtering:

```bash
RUST_LOG=audit_log=info hv2 serve 2> audit.log
```

Rotation, retention and shipping are whatever you point that at — `logrotate`,
a systemd unit, a sidecar. None of it is configured here.

## Not implemented

An earlier version of this page documented an `[audit]` section with
`log_path`, `rotation`, `retention_days`, `format` and an `events` allowlist,
and named File, Syslog, Elasticsearch and CloudWatch as destinations. None of
that is read or implemented: there is no audit configuration section, no file
sink, no rotation, no retention policy, and no per-event filtering. `hv2 config
check` reports an `[audit]` section as a key the build does not read.

The event shape on that page — `actor`, `resource`, `action`, `result` — is
also not what is emitted; see above for what is.
