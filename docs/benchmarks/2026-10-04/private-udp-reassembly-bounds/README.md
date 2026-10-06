# Bounded private IPv4 UDP reassembly

**204 networking tests pass** against accepted isolated sources. This turn changes only the owned Ethernet test harness; production fragmentation/routing code is unchanged.

- Reverse-order delivery sends every fragment of a 65,507-byte datagram in reverse order, including the UDP-header fragment last. The guest receives the exact maximum-size reply; multiple valid-MTU fragments occur in both directions and the private transport receives exactly the original payload.
- Capacity starts two incomplete datagrams with distinct IPv4 identifiers, filling the configured two reassembly buffers. A complete third fragment set does not open private transport or deliver a datagram. Completing the first two delivers two exact maximum payloads. Retrying the third after capacity is released delivers its exact payload.
- Expiry starts two incomplete sets, waits beyond the five-second reassembly window, and supplies their late tails. They do not open private transport or deliver datagrams. After incomplete late tails also expire, a fresh identifier's complete maximum set delivers exactly.

IPv4 identifiers are changed on actual smoltcp-generated Ethernet fragments with recomputed IPv4 checksums. UDP payload/checksum are preserved. Captured incomplete sets are injected only into the owned in-process gateway. The bounded-capacity/expiry cases inspect exact private-hook admission; the reorder case also verifies exact guest reply emission. No malformed third-party traffic or external endpoints are used.

All 139 permitted root/isolate source pairs and accepted isolated core hashes are revalidated, with the authorized test file hash updated. Protected root core files were not read or built. Run `python3 verify-results.py` to check the suite count, named gates and source snapshot. `driver.py` retains the locked isolated Cargo invocation and original fresh log path. The manifest pins archived payloads.

This closes the owned Ethernet reorder/capacity/expiry proof gaps. It does not add actual KVM pressure/reorder/expiry evidence, overlap/corruption adversarial coverage, private IPv6, owner/pending/local-VM races, independent-host operation or crash recovery. The separate preceding KVM evidence remains scoped to its runtime inputs. No performance improvement or competitor superiority is claimed.
