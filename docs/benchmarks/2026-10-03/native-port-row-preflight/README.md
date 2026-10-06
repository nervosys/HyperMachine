# Redis reservation validation before mutation

Existing managed-port records are checked for unknown fields, valid protocol, bounded opaque owner identity, correct destination/public-port identity and agreement between indexes before update or deletion. Previously an unknown field could survive the Lua update and only be rejected by Rust decoding after both writes.

The explicitly owned Redis fixture injects four mutually agreeing malformed records: unknown field, invalid protocol, invalid owner and an array instead of an object. Both owner-authorized and privileged internal update/delete operations reject every case, and both index values remain byte-identical. Valid records are restored before the existing ACL-denial and AOF-restart checks continue.

Validation: 86 cluster library tests passed (one owned Redis test ignored by default); the explicit owned Redis test passed separately. Tests ran in the isolated checkout with accepted core sources. The source context records matching permitted root/isolated files and accepted isolated core hashes; protected root core files were excluded. No KVM binaries were rebuilt and earlier frozen capacity evidence remains tied to its earlier source context.

Reproduce from the isolated checkout:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-cluster --lib
CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-cluster --lib managed_ports_redis_owned_restart -- --ignored --nocapture
```

This is a data-integrity regression check, not a performance result or proof of managed Redis durability.
