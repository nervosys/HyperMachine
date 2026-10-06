# Forced mkdir refuses existing files

Previously the daemon accepted every EEXIST result for directory creation when force=true, including an existing regular file, and returned successful metadata. It now opens the existing entry with the beneath-root directory resolver, requiring an actual directory before accepting the conflict. This also uses the resolver's existing no-symlink rules.

All 46 daemon tests and both owned control-router integration tests pass. The new shipped CLI regression calls mkdir --force on an existing 512 KiB file through the router to a real daemon, requires failure, and verifies the exact file bytes are unchanged. Existing missing-parent/recursive-creation/repeat-directory checks still pass. Build and tests used the accepted isolated core checkout; protected root core sources were not used.

Reproduce daemon tests with `cargo test --offline --locked -p hv2-sandboxd`. Set HM_VOLUME_TEST_DAEMON, HM_VOLUME_TEST_CLI, HM_VOLUME_TEST_KERNEL and HM_VOLUME_TEST_INITRD and run `cargo test --offline --locked -p hv2-cluster --test volume_routing -- --include-ignored` for real routing.

This fixes a file-type contract error, not a transactional namespace guarantee. Concurrent renames between directory validation and later metadata operations are not covered. The fixture uses local HTTP and MemoryStore, with no guest mount, Redis, TLS, network filesystem or power-loss test. No competitor performance claim is made.
