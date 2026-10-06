# Scheduled ACME renewal and durable deployment recovery

The final owned run passes **21 live issuer/worker checks and 14 real KVM checks**.
The scheduler's separate **21 unit/process tests** pass with `python3 -O`.
The expanded verifier also passes six legacy custom-domain checks on the same
compiled control-plane binary. All fixture processes are reaped, HTTP/DNS threads
stop, and the isolated KVM node inventory is empty.

| Behavior | Verified result |
|---|---|
| Scheduled issuance and activation | Worker forces a real HTTP-01 renewal and new certificate-validated TLS connections see its leaf |
| Not due | No issuance, challenge traffic or journal rewrite |
| Continuous scheduling | Two completed due checks are counted from the live worker's output; active generation is not duplicated; SIGTERM stops the worker cleanly |
| Exclusive worker | A second process refuses the first worker's held journal lock |
| Control-plane outage | Certbot issues successfully while activation is unavailable; a pending deployment survives in the durable journal |
| New PID | Restarted control plane is discovered and the pending lineage activates without another certificate issuance |
| Termination after publication | Owned wrapper exits immediately after atomic manifest publication, before signaling; worker reconciles disk/runtime and activates the issued lineage without reissuance |
| Termination after activation | Owned wrapper exits after TLS verification, before journal removal; retry verifies the active generation, clears the journal and creates no duplicate |
| Recovery input refusal | A corrupt journal checksum and an unrelated manifest edit are specifically refused without mutation |
| Real guest traffic | Worker-driven HTTP-01 renewal activates while the old 16 MiB KVM guest download completes exactly |
| Existing TLS protections | Untrusted-certificate rollback, key mismatch, wrong process/manifest, concurrent deployment refusal and expired-leaf recovery pass |
| Domain lifecycle | DNS ownership, port update, Redis persistence, auto-resume, deletion and reuse remain correct |

The two crash wrappers are generated only inside the owned fixture. Their source
and hashes are preserved in `report.json`; production sources contain no
environment-controlled crash switch. The published-state test verifies that the
old leaf remains active before reconciliation. The activated-state test verifies
that the new leaf is already active before reconciliation. Both require unchanged
issued certificate bytes and zero additional HTTP-01 responses during recovery.

Scheduler tests cover durable phase ordering, success only after activation,
not-due behavior, coalesced missed intervals, failure backoff, crash during
issuance/deployment, lost completion writes, configuration changes, backward
clock movement, independent due jobs, input refusals, owned process discovery,
ambiguity refusal and timeout cleanup of a parent/descendant process group.

Use [the operator worker setup](../../../TLS_RENEWAL_WORKER.md). The full owned
verification command adds the worker to the existing ACME/KVM fixture:

```sh
python3 -O tools/check-acme-issuance.py \
  --pebble /path/to/pebble --challenge-server /path/to/pebble-challtestsrv \
  --certbot /path/to/venv/bin/certbot --control-plane /path/to/hv2-control-plane \
  --deploy-hook /absolute/path/to/tools/deploy-tls-certificate.py \
  --renewal-worker /absolute/path/to/tools/renew-tls-certificates.py \
  --kvm-verifier /absolute/path/to/tools/verify-custom-domains.py \
  --daemon /path/to/hv2-sandboxd --kernel /path/to/bzImage \
  --initrd /path/to/guest-output-drain.cpio.gz --output /new/path/report.json

python3 -O tools/test-renew-tls-certificates.py
```

The archived `tools/` copies reproduce the exact drivers. The input catalog binds
their hashes, the unchanged control-plane/daemon binaries and kernel/initrd.
Certbot 5.8.0 and the official Pebble v2.10.1 release are used; dependency versions
are in `dependencies.txt`. Certbot's executable hash covers its launcher rather
than every installed Python module. The control-plane build/source context is
[preserved separately](../tls-bundle/README.md). No Rust source or protected
original core file changed during this worker implementation.

The first worker run established issuance/outage recovery but used an indirect
watch counter; the final verifier counts completed worker output directly.
Later runs added actual publication/activation termination, corrupt/external-update
refusal and overlapping-worker refusal. Only the final source-bound run supports
the table above. Earlier local exploratory logs are not performance evidence.

The deployment journal contains checksummed manifests and public leaf hashes;
private keys remain in `0600` generation files beneath an owned `0700` directory.
It reconciles only its recorded prior/proposed manifests and observed leaf pins.
Normal chain, hostname and time validation is required for new activation. Old
leaf pinning permits expired-certificate recovery but does not make an expired
rollback healthy. Changed generations, unrelated edits and failed reconciliation
still require operator intervention; all writers must honor the deployment lock.
Superseded and rejected generations remain available for operator maintenance.

All issuance uses an owned CA with real HTTP-01 responses, authorization reuse
disabled and ordinary ACME transport TLS verification. No public CA is contacted.
Issuer/DNS test services use loopback; existing HyperMachine listeners use their
normal bind behavior with random fixture credentials. Temporary keys, account
state and challenge files are removed. These results prove configured local
renewal/recovery behavior, not automatic initial issuance after a domain claim,
public DNS/CA operation, fleet orchestration or managed-product performance.

Primary references: [Certbot renewal/hooks](https://eff-certbot.readthedocs.io/en/stable/using.html),
[Pebble v2.10.1](https://github.com/letsencrypt/pebble/releases/tag/v2.10.1), and
[Python pidfd signaling](https://docs.python.org/3/library/signal.html#signal.pidfd_send_signal).
