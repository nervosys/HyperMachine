# Explicit upstream roots on the sandbox daemon

The Linux daemon now accepts `--egress-upstream-ca /absolute/path/roots.pem`
with `--network`. This explicitly adds operator roots to the public Web PKI
for intercepted upstream TLS. The guest interception CA is never implicitly
trusted upstream. The setting is node-local, applies to all gateways on that
node, and requires restart for changes; SIGHUP reloads secret policies only.

The immediate parent must be owned 0700; the file owned 0600, regular,
single-linked, and non-symlink. Ancestor-directory trust remains the operator's
responsibility. The bounded loader accepts at most 1 MiB and 128 certificate
roots and validates certificate DER before daemon startup. No option preserves
the existing public-root behavior. This option grants no egress permission.

All 188 hv2-net tests passed, including bounded certificate parsing and existing
owned trusted/untrusted upstream interception checks. The daemon compiled and
built in the isolated accepted-core checkout. The archived changed Rust files
match the checkout and workspace bytes; prior-build-context.json identifies the
preceding checkout catalog, with these changed files overlaid for this build.
The separate verification daemon hash is in result.json. The accepted benchmark
binary, kernel and initrd were not replaced.

The owned checker passed twelve startup/reload checks and reaped every daemon.
It creates a temporary local CA using OpenSSL and removes keys and policy files.
It refuses unsafe/symlink/invalid upstream roots and accepts a valid one. It
creates no guests and observes no upstream TLS request, so this does not establish
KVM guest lifecycle parity, competitor equivalence, or any performance win.

Reproduce with `cargo test -p hv2-net`, `cargo build -p hv2-sandboxd`, then
`python3 tools/check-daemon-secret-policy.py --daemon BIN --kernel KERNEL
--initrd INITRD --output NEW.json` in the isolated accepted-core checkout.
