# Root lockfile alignment

The root and accepted isolated Cargo.lock files differ only by CRLF/LF. Parsing both as TOML yields identical documents, including every package version, dependency list, checksum and source. The workspace Cargo.toml files already match byte-for-byte. Before-copy hashes, raw lockfiles and the canonical parsed-document hash are archived.

Root lockfile bytes were copied into the accepted isolated checkout. Cargo check --offline --locked -p hm-cli -p hv2-sandboxd -p hv2-cluster --bins succeeds without modifying them. A fresh catalog verifies all Rust source/tests and manifests for the six IPv6 crates still match root bytes, alongside exact workspace manifest/lockfile equality. The root worktree files were unchanged.

This resolves the earlier lockfile uncertainty for the [source-aligned IPv6 verification](../udp-ipv6-source-alignment/README.md). Its KVM runs used identical parsed dependency data already; this audit requires no new performance or runtime claim. No runtime source, feature or dependency version changed, so the preceding verified immutable binaries and checks remain the evidence for their recorded source/build context.

The remaining protected-core limitation is unchanged: builds use the accepted isolated versions of kvm.rs, boot/linux.rs and boot/source.rs, whose exact hashes are cataloged. Protected root modifications were not read or executed. Other workspace crates were not exhaustively cataloged here. Thus six-crate source and dependency alignment does not establish whole-root-workspace correctness or competitor superiority.

Reproduce by comparing the archived TOML documents, checking raw byte equality after copying root lockfile bytes into an accepted source checkout, then running the recorded offline locked check. Earlier immutable archives retain accurate pre-alignment byte hashes; this follow-up clarifies their semantic equality without rewriting them.
