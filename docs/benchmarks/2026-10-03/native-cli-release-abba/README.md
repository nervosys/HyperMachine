# Optimized native UDP and CLI tunnel comparison

These release-build measurements compare two HyperMachine product paths: native UDP gateway and CLI UDP tunnel. Both run throughout each CLI/native/native/CLI block sequence against the same real KVM guest IPv4 UDP echo service on port 18082. Each guest has one vCPU and 1,024 MiB, recorded from the API inventory. Ingress is IPv4 with two peers or IPv6 with eight peers. An idle native TCP connection remains open in every block. Each peer performs ten warmups and 1,000 measured 4,096-byte tagged round trips, with one outstanding datagram. All replies verify payload and full source address; timed transfers never retry.

| Ingress profile | Block/path | Round trips/s | Mean peer P50 ms | Mean peer P95 ms | Mean peer P99 ms |
|---|---|---:|---:|---:|---:|
| IPv4, 2 peers | 1: cli | 2348.64 | 0.8097 | 1.0092 | 1.1247 |
| IPv4, 2 peers | 2: native | 2992.94 | 0.6469 | 0.8632 | 0.9798 |
| IPv4, 2 peers | 3: native | 2849.13 | 0.6711 | 0.8928 | 1.0513 |
| IPv4, 2 peers | 4: cli | 2369.58 | 0.8225 | 1.0028 | 1.1300 |
| IPv6, 8 peers | 1: cli | 6626.61 | 1.1618 | 1.6502 | 1.9171 |
| IPv6, 8 peers | 2: native | 7322.89 | 1.0325 | 1.6109 | 1.9283 |
| IPv6, 8 peers | 3: native | 7474.12 | 1.0179 | 1.5724 | 1.8447 |
| IPv6, 8 peers | 4: cli | 6470.53 | 1.1833 | 1.6725 | 1.9750 |

Averaging block completion rates gives two-peer CLI 2,359.11 versus native 2,921.04 round trips/s (+23.8%), and eight-peer CLI 6,548.57 versus native 7,398.51 (+13.0%). Mean peer sample medians are 0.8161 versus 0.6590 ms (-19.3%), and 1.1725 versus 1.0252 ms (-12.6%). Tail rankings are mixed: first native eight-peer block mean peer P99 is 1.9283 ms versus 1.9171 ms for the preceding CLI block. These measurements do not establish an improvement in every metric. Percentiles use nearest rank per peer; table entries average peer percentiles rather than pool samples. Rate divides total replies by the longest peer measurement interval; summary rates average the two independent block rates.

Both profiles pass fifteen functional/lifecycle checks, gracefully stop all owned processes and leave zero guests. Payload/lifecycle checks include native 1 MiB TCP, empty/binary/65,507-byte UDP, protocol changes, real guest pause/resume and deletion with allocation cleanup. Node mutual TLS and API certificate refusal checks pass. Reservations are administratively seeded in a fresh owned namespace; owner-management APIs are not verified. Redis uses local plaintext TCP. The guest image/kernel match the preceding accepted native KVM archive exactly.

Host binaries were built in accepted isolated `/var/tmp/hm-egress-log-mA2CCL` with `CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo build --offline --locked --release -p hv2-sandboxd -p hv2-cluster -p hm-cli --bins`, then frozen to fresh immutable paths. Build exit status was zero; archived build log ends with optimized release completion. Release configuration is optimization level 3, fat LTO, one codegen unit, stripped symbols, overflow checks and panic=abort. Report build profile is an operator declaration backed here by that build log and frozen input hashes, not a binary-format inference. 131 permitted root/isolate sources/manifests were revalidated before and after building; accepted isolated core hashes were checked before build. Protected modified root core sources were not read, built or staged. The copied source context identifies this limited accepted context, not whole-root validation.

Reports preserve raw samples and verify binary, kernel, image and checker hashes before/after. Host metadata records WSL kernel 6.18.33.2, x86_64, 24 logical/allowed CPUs and Python 3.13.5. This is an unpinned short closed-loop benchmark, without CPU/memory attribution or sustained load. Native bypasses the CLI/control-plane TLS hop, so this is a product-path comparison, not an identical-topology code optimization. Earlier development measurements are retained separately and are not treated as a matched release-versus-development optimization experiment. Boxd/exe.dev remain unmeasured; no competitor or public Internet performance claim follows.

Reproduction requires the dual-service guest image and a fresh output directory:

```sh
python3 tools/check-udp-cluster-kvm.py --daemon /path/release/hv2-sandboxd \
  --control-plane /path/release/hv2-control-plane --cli /path/release/hm \
  --kernel /path/bzImage --initrd /path/native-fixture.cpio.gz \
  --native-gateway /path/release/hv2-native-gateway --tls --mtls \
  --host-build-profile release --peer-count 2 --native-comparison-samples 1000 \
  --output /fresh/results
```

Use `--local-ipv6 --peer-count 8` for the second profile. Source checker, reports, exact logs, build log, input hashes and source/manifest context are covered by manifest.json. No certificate private keys are archived.
