# Bounded UDP reply reuse candidate

Experimental source only; production remains unchanged. This follows the mixed release results of the unbounded-retention candidate. The inbound reader reuses its reply Vec, but replaces it with an empty Vec after a successful send whenever capacity exceeds 4096 bytes. This bounds retained buffer capacity between completed exchanges to 4 KiB per peer; maximum datagrams still require transient larger allocations. Outbound queues, framing, session budgets and timeouts are unchanged.

Five targeted native UDP tests pass in the accepted isolated checkout. They cover peer/datagram behavior, IPv4/IPv6, refusal, pressure, oversized ingress, failed sessions and cancellation. They do not establish release performance, retained process memory, or a competitor win. The prior candidate's rate differences were confounded by unchanged CLI reference variation; this candidate must receive matched release measurements before promotion.

The candidate source and patch are preserved here for a subsequent experiment. The isolated source was restored to the production baseline after testing. Baseline runtime artifacts from the earlier release experiment remain available for a matched comparison. No production Rust source change was made.
