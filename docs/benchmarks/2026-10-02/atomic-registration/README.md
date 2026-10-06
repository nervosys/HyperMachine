# Atomic named registration store evidence

The Linux library suite passed all 35 tests with `HV2_TEST_REDIS` pointing at an owned loopback Redis process. Both the shared contract and wrong-index-type test ran, and Redis exited with code 0 after cleanup. Windows library tests and strict library/test Clippy on both platforms also passed; Redis tests were enabled on Linux only.

The shared contract includes 16 competing registration races and 16 registration/deletion races, wrong-token/metadata/ID rejection, exact replay, stale-record protection and safe ownership reuse. The Redis-specific test injects a wrong type into each registration index and checks that neither the sandbox nor binding was written. The implementation is an unused storage primitive pending trusted node creation integration, not a completed name recovery feature.

`tests.log`, `redis.log` and `report.json` are raw outputs from the owned-server run; `store.patch` captures the tested source change against the base commit in `manifest.json`. File SHA-256 values are listed in that manifest. Reproduce the storage tests using an owned Redis instance, then run `HV2_TEST_REDIS=redis://127.0.0.1:PORT cargo test -p hv2-cluster --lib -- --nocapture`. The Redis tests use distinct namespaces and the captured run disabled persistence on its private server.

This is shared-store functional evidence. It makes no KVM, lost-response recovery or performance claim.
