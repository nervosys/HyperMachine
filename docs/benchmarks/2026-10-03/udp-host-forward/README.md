# Host UDP forwarding methods

Added GuestAgent::forward_udp and AgentVM::forward_udp_port using the existing GuestExec/vsock capability and blocking-task boundary. Port zero is rejected before contacting the guest. TCP and UDP share acknowledged-stream extraction, preserving buffered response-tail bytes and refusing non-vsock transports. Guest setup refusal and unexpected replies propagate as errors. UDP setup acknowledgement does not prove a peer is listening.

All 14 focused guest_agent protocol tests pass in the accepted isolated checkout; 516 other agent library tests were filtered out, not run. The new scripted-channel test checks exact ForwardUdp operation/port selection, guest refusal propagation and port-zero refusal before channel contact. Reproduce with `cargo test --offline --locked -p hv2-agent --lib guest_agent::tests`.

This verifies request/response handling, not a real vsock UDP session. Authenticated node/control-plane routing, lifecycle integration, CLI peer handling and rebuilt KVM guest verification remain required. The capability matrix continues to treat UDP access as incomplete.
