# Full CLI and cluster regression suites

`cargo test --offline --locked -p hm-cli -p hv2-cluster` completed successfully in the accepted isolated checkout: 286 tests passed, zero failed, and one real-daemon test was ignored by default. The suites cover CLI library/binary behavior, boot-source registry, job scheduling, MCP interfaces, VM/volume clients, cluster unit tests, control-plane routing and protocol volume routing. Documentation test targets contained zero tests.

The ignored real-daemon volume test is not counted as a pass here. It passed separately with explicit owned inputs in the volume-directory-sync archive. Neither the broader suite nor that fixture establishes competitor parity or fleet performance.

The archived source context identifies the isolated CLI/cluster Rust and manifest files and isolated lock. Dependencies were compiled from the accepted isolated source tree, which deliberately retains accepted core sources rather than protected root modifications. This does not validate the entire current root workspace, every dependency's latest root edits, or production deployment. Reproduce using the command above in the same accepted source context.
