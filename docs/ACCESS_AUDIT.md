# Durable control-plane access records

Enable synced, tamper-evident records for the protected API in
`hv2-control-plane` by setting both variables before startup:

```sh
export HV2_ACCESS_AUDIT=/var/lib/hypermachine/control-1-access.jsonl
export HV2_ACCESS_AUDIT_KEY_FILE=/etc/hypermachine/access-audit.key
hv2-control-plane --store redis://127.0.0.1:6379
```

The key file contains a randomly generated 32-byte key encoded as 64 hexadecimal
characters, with optional surrounding whitespace; the file is limited to 128
bytes. Restrict file access to the operator and service account. Keep the key
and collected logs outside guests. Each control-plane instance needs its own
log path. A second cooperating writer is refused using an exclusive OS file
lock ([Rust file-lock documentation](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)).
Unset both variables to retain the existing tracing-only behavior. Setting
only one variable, an invalid key, an unverifiable log, or an unavailable lock
refuses startup.

An admission record is appended and synced before a protected request reaches
its handler. A completion record with status and elapsed microseconds is
appended and synced before returning the handler's response. Rejected API-key
authentication and scope checks also produce records. Record fields contain a
generated request UUID, HTTP method, matched route pattern, principal category,
configured-key fingerprint and authorization outcome. They exclude request
bodies, queries, caller-supplied path values, credentials and unknown-key hashes.
This privacy choice records an operation class rather than a specific sandbox.

Writes run on blocking workers and serialize within an instance. An audit I/O
failure permanently stops later protected requests in that process. Admission
failure returns 503 without dispatch. Completion failure returns 503 explaining
that the operation may already be committed; inspect its state rather than
retrying a mutation blindly. Already admitted requests may still complete.
Public health endpoints remain available. After repairing storage, restart the
control plane: it verifies every prior record before appending the next sequence.
Malformed, partial, oversized or edited records are refused without truncation
or automatic repair. Startup reads at most one bounded record at a time.

Verify a stopped instance's log independently:

```sh
python3 tools/verify-access-audit.py /var/lib/hypermachine/control-1-access.jsonl \
  --key-file /etc/hypermachine/access-audit.key
```

The verifier checks HMAC-SHA256, sequence, predecessor and correlated admission/
completion identities. It reports unfinished admissions separately. A crash or
cancelled request can leave an admission without completion; this is an unknown
outcome, not evidence that a mutation failed. The verifier targets this protected
API schema. Other HyperMachine audit sources may use different event schemas.

This feature covers protected control-plane API requests and HTTP upgrade
handshakes. Completion marks response headers, not streaming body delivery or
later TCP activity. Public sandbox URLs, direct node APIs, guest commands through
envd, tenant roles and per-resource attribution need separate auditing. It does
not provide automatic retention, rotation, remote collection, or a fleet-wide
ordered journal. Stop a writer before rotating its file; moving/replacing an
active path does not move its open file handle. Forward records to independently
controlled storage: an HMAC chain cannot detect tail truncation without an
external checkpoint, and a process with the key can forge records. Exclusive
locks coordinate participating writers and are not protection against an
operator rewriting files.

Synced admission and completion add storage work per request. No performance
win or throughput SLA is claimed. The [verification archive](benchmarks/2026-10-02/access-audit/README.md)
records restart, fault, privacy and KVM/TLS lifecycle evidence.
