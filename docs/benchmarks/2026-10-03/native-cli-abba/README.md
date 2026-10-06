# Matched native UDP and CLI tunnel comparison

These internal HyperMachine measurements compare native gateway UDP against the shipped CLI UDP tunnel on the same actual KVM guest echo port 18082. Each fresh owned stack runs CLI/native/native/CLI blocks with both forwarding processes live throughout, an idle native TCP connection, 4,096-byte tagged payloads and one outstanding datagram per peer. Ten warmups precede 1,000 measured replies per peer per block. Every reply verifies bytes and full source address; timed transfers have no retries. Each profile passes fifteen functional/lifecycle checks, reaps all owned processes and leaves zero guests.

| Ingress profile | Block/path | Round trips/s | Mean peer P50 ms | Mean peer P95 ms | Mean peer P99 ms |
|---|---|---:|---:|---:|---:|
| IPv4, 2 peers | 1: cli | 1422.85 | 1.3745 | 1.6828 | 1.8781 |
| IPv4, 2 peers | 2: native | 1790.69 | 1.0960 | 1.3894 | 1.5530 |
| IPv4, 2 peers | 3: native | 1795.78 | 1.0829 | 1.3884 | 1.5706 |
| IPv4, 2 peers | 4: cli | 1436.14 | 1.3668 | 1.6468 | 1.8121 |
| IPv6, 8 peers | 1: cli | 3385.56 | 2.2715 | 3.2930 | 3.8489 |
| IPv6, 8 peers | 2: native | 3790.51 | 2.0228 | 3.0609 | 3.5581 |
| IPv6, 8 peers | 3: native | 3843.64 | 2.0026 | 2.9961 | 3.5095 |
| IPv6, 8 peers | 4: cli | 3394.06 | 2.2837 | 3.2628 | 3.7874 |

Averaging the two block completion rates per path gives IPv4/two-peer CLI 1,429.49 versus native 1,793.24 round trips/s (+25.4%); IPv6/eight-peer CLI 3,389.81 versus native 3,817.08 (+12.6%). Mean peer sample medians are 1.3707 versus 1.0895 ms (-20.5%) and 2.2776 versus 2.0127 ms (-11.6%), respectively. Percentiles use nearest rank per peer; table entries average those peer percentiles, not a pooled distribution. Completion rate is measured replies divided by the longest peer measurement interval; the two block rates are averaged without pooling.

The accepted immutable binaries, kernel and dual-service image are identical between profiles and to the preceding native KVM functional archive. Reports verify their hashes and the current checker before/after. The copied source context comes from that preceding accepted isolated build (131 permitted files); it does not validate protected modified root core sources. Host executables are unoptimized development builds on an unpinned WSL host. Native forwarding bypasses the CLI/control-plane TLS hop, so this compares two product paths, not a code optimization with identical topology. It does not establish sustained capacity, CPU/memory efficiency, public Internet delivery, statistical confidence or performance against Boxd/exe.dev, whose endpoints remain unavailable.

Reproduce with fresh output directories and accepted dual-service image:

```sh
python3 tools/check-udp-cluster-kvm.py --daemon /path/hv2-sandboxd \
  --control-plane /path/hv2-control-plane --cli /path/hm \
  --kernel /path/bzImage --initrd /path/native-fixture.cpio.gz \
  --native-gateway /path/hv2-native-gateway --tls --mtls \
  --peer-count 2 --native-comparison-samples 1000 --output /fresh/results
```

Use `--local-ipv6 --peer-count 8` for the second profile. Guest destination remains IPv4. Reservations are fixture-admin seeded, not owner-management API evidence. An initial run completed its blocks but failed the CLI SIGTERM exit-status check and produced no accepted report; it is excluded. The accepted reruns use the CLI's established SIGINT shutdown. All archived reports, exact logs, checker and provenance are covered by manifest.json; no private keys or credentials are archived.
