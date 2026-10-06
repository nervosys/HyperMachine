# Current API full-library regression and provenance correction

The full API library suite passes using offline locked dependencies in the accepted isolated checkout. The complete API and cluster Rust source/manifests were compared against the current root worktree and match byte-for-byte; the matching catalog is archived. Protected root core files remain outside the test build.

This corrects the overly broad earlier qualification in UDP archives that current root API TLS source additions differed from the tested API context. The inspected current API/cluster sources and manifests do not differ. The isolated core and lock remain a distinct test context, so this is not whole-root workspace validation. Frozen earlier archives are preserved; this authoritative full comparison supersedes their unsupported API-source mismatch statement.

Reproduce with `cargo test --offline --locked -p hv2-api --lib` in the accepted checkout. These are library tests, not production deployment, public CA validation or competitor benchmarks. UDP full-stack runtime scope remains as documented in its separate HTTPS/mTLS/KVM archives.
