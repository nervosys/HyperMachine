# Volume directory creation CLI

Added `hm sandbox vm volume mkdir ID --path /nested/path [--force]`. It uses HV2_VOLUME_TOKEN without loading the platform API key, requires HTTPS or loopback HTTP, uses configured TLS trust/timeout/redirect refusal, and bounds returned JSON to 1 MiB. The existing daemon API supplies directory creation semantics; no directory durability or mode customization was added.

The owned real control-router/two-daemon fixture verifies missing-parent refusal without force, recursive creation with force, existing-directory refusal without force, and repeat success with force. The path /nested/a b&c is checked both in returned directory metadata and the actual shared storage directory. An invalid platform API key does not prevent success. Existing upload/download, browsing, token refusal, node termination, peer access, restart and cleanup checks pass. Seven volume CLI tests, twelve VM CLI regressions and two routing integration tests pass in the accepted isolated source checkout.

Reproduce CLI tests with `cargo test --offline --locked -p hm-cli --test volume_client --test sandbox_vm_client`. For real routing, set HM_VOLUME_TEST_DAEMON, HM_VOLUME_TEST_CLI, HM_VOLUME_TEST_KERNEL and HM_VOLUME_TEST_INITRD to their respective inputs and run `cargo test --offline --locked -p hv2-cluster --test volume_routing -- --include-ignored`.

This uses MemoryStore, an in-process real router, owned real daemon processes and loopback HTTP. A separate control-plane process, Redis, TLS/mTLS for the new command, guest mounts, network storage and power-loss durability remain outside the verification. No competitor win is claimed.
