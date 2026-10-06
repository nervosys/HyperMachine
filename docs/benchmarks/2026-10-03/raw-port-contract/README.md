# Managed raw-port contract audit

Primary documentation was rechecked on 2026-10-03: [Boxd](https://docs.boxd.sh/llms-full.txt), Port forwarding; [exe.dev](https://exe.dev/docs/all), HTTPS proxy / Additional Ports. contract.json records the reviewed behavior and the resulting implementation/verification gates. No competitor endpoint was exercised and no performance was measured.

The comparison previously combined HTTP port URLs, local tunnels and managed public raw forwarding in one row. The corrected table separates them. Native listener work must address durable reservation and ownership as well as the packet relay; the current local framed tunnel remains verified in its recorded scope.

The reviewed exe.dev source does not establish raw UDP support. This is a documentation-evidence limit, not a claim that the product lacks it. Implementation and verification of HyperMachine's managed public raw forwards remain pending. The broader competitiveness goal remains incomplete.
