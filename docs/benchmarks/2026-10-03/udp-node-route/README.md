# UDP node route wiring

Added /sandboxes/{sandboxID}/ports/{port}/udp to the same cluster-authenticated router as TCP. A shared forwarding handler selects exact transport negotiation and guest TCP or UDP setup. It rejects port zero and unknown local sandboxes, holds the lifecycle transition lock through opening/registration, registers the vsock session in the existing forwarding close list, and carries buffered bytes through the existing private loopback relay. UDP upgrades use hv2-udp/1.

The isolated daemon test suite passes. This proves compilation and existing daemon regressions, not live UDP route behavior, authentication refusal or lifecycle cancellation. Reproduce with `cargo test --offline --locked -p hv2-sandboxd` in the accepted isolated source context with the UDP API/agent overlays.

A rebuilt guest image, real KVM UDP payload round trips, pause/delete cancellation, authenticated control-plane routing and CLI handling remain required. Cluster credentials must be configured by the operator as for existing node routes. No user-facing UDP parity claim is made yet.
