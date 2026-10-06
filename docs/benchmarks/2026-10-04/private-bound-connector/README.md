# Generation-pinned private connector

`PrivateNodeConnector::open_bound_tcp` takes the complete address route claim, including source/destination membership generations. It validates the claim and fixed source before route lookup, then compares it to the fresh authorized claim before sending any HTTP request. Existing node/URL/context rechecks after upgrade remain in place. The original current-route `open_tcp` API retains its behavior; guest address dialing must use the bound API.

All nine connector tests pass. A source address allocated before destination rejoin is refused without contacting an owned listening destination socket. Additional stale source generation, forged source identity, zero port and unshared network claims are refused with exactly one lease drop per attempt. A real owned mTLS server verifies valid bound setup and exact early/binary bytes, then verifies refusals for source rejoin, destination removal/movement and lease revocation during upgrade.

Full isolated cluster suite: **111 passed, zero failed, one ignored**, with owned Redis enabled for both store contracts. Redis exited zero and was reaped. Redis ACL fault opt-in tests remain unset. Catalog verifies 137 permitted root/isolate source pairs and accepted isolated core hashes; protected root core sources were neither read nor built.

This prevents the source connector from refreshing a stale address binding into a new generation. Production membership-backed guest router wiring, durable address persistence, live daemon lease acquisition, guest DNS and same/cross-node KVM guest-to-guest verification remain unfinished. Host fixtures establish no competitor or performance ranking.
