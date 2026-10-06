# Volume CLI browsing

Added `hm sandbox vm volume ls ID --path / --depth 1` and `volume stat ID --path /file`. These use HV2_VOLUME_TOKEN, ignore the platform API key, require HTTPS or loopback HTTP, retain the configured TLS trust/request timeout/redirect refusal, and bound JSON responses to 1 MiB. Directory depth is validated from 1 through 32. Existing daemon and control-plane content routes are reused.

Seven shipped CLI volume tests and twelve existing VM CLI tests pass in the accepted isolated checkout with offline locked dependencies. The new owned HTTP protocol test verifies bearer authentication, absence of the API-key header even with an invalid platform key, exact path/query encoding, successful JSON output, missing-path failure, oversized-response refusal, and invalid-depth refusal before server contact.

Reproduce with `cargo test --offline --locked -p hm-cli --test volume_client --test sandbox_vm_client`. This archive verifies the CLI against a protocol fixture, not these new commands through a real daemon/control plane or TLS endpoint. Real endpoint integration remains to be checked. Listing is bounded rather than paginated; directories whose response exceeds 1 MiB fail explicitly.
