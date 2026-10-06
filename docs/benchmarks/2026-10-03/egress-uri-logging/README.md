# Egress URI logging removal

The gateway relay now logs request method and response status without recording
request URIs, whose paths and query strings can contain guest credentials.
No request transformation or forwarding behavior was changed.

All 167 hv2-net library tests passed in an isolated accepted-source checkout,
including header injection, TLS, DNS rebinding refusal and destination-policy
checks. The tested mitm.rs bytes match the working file. The three protected
worktree boot files were not read, built or copied. The source base was
/var/tmp/hm-object-backup/source; only the unprotected mitm.rs was overlaid.

Command: CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-net --lib

This is library validation, not real guest evidence, a rebuilt accepted daemon,
secret-substitution parity or a performance measurement. Broader host-bound
placeholder substitution remains incomplete.
