# Native TCP/UDP gateway through real KVM

Native public-port forwarding is now verified through the operator executable, owned Redis, mutual-TLS node API, actual KVM/vsock and guest TCP/UDP echo services. Two accepted runs use the same immutable host/gateway/kernel/guest inputs: IPv4 native ingress with two UDP peers and IPv6 ingress with eight peers. Each passed fourteen checks. Native TCP preserves exact 1 MiB binary data; each UDP peer preserves empty/binary/65,507-byte datagrams on the same allocated public port and guest destination. These are sequential peer functional checks, not concurrent throughput measurements.

The guest image adds the existing static TCP fixture and an opt-in UDP echo port to the exact accepted base and static agent. Both services listen on guest loopback port 18082 (TCP streaming fixture); default UDP port 5353 still serves existing framed tests. Native protocol changes retain the public port and close old TCP. Actual guest pause closes native sessions while retaining the exact reservation; resume restores both protocols on that port. Actual deletion through the API removes the Redis allocation, closes native sessions and leaves no VM inventory. The gateway exits gracefully. Current daemon/control binaries include the durable native allocation cleanup code; the previous pre-allocation node binary was not substituted.

The extended checker keeps native verification opt-in via `--native-gateway`, requiring node mutual TLS and current IPv4 guest destination negotiation. Native ingress can be IPv4 or IPv6 independently. Its original mode passed nine checks with the earlier accepted UDP-only image. Default UDP helper behavior remains unchanged; `--port` is only used by the owned native fixture. Static helpers compiled with `-Wall -Wextra -Werror`; the deterministic image builder refuses overlapping/nonfresh outputs, verifies the accepted base and static ELF inputs and hashes all payload binaries.

Both native runs and the default regression reaped daemon/control/Redis/CLI/gateway processes and left zero guests. Each native run checks executable, kernel, image and checker hashes before/after. Reports contain exact input hashes; image-report.json identifies base/agent/TCP/UDP payloads. The host build used isolated `/var/tmp/hm-egress-log-mA2CCL` with accepted core hashes. 131 permitted source/manifest files across six crates matched root and isolate exactly; source-context.json records them. Protected root core sources were not read, built or staged. This is not whole-root workspace verification. Sources, exact logs and reports are hashed in manifest.json.

Reproduction (fresh output path; the image must contain both owned fixture services):

```sh
python3 tools/check-udp-cluster-kvm.py --daemon /path/hv2-sandboxd \
  --control-plane /path/hv2-control-plane --cli /path/hm \
  --kernel /path/bzImage --initrd /path/native-fixture.cpio.gz \
  --native-gateway /path/hv2-native-gateway --tls --mtls --output /fresh/results
```

Add `--local-ipv6 --peer-count 8` for the second profile. The fixture seeds a fresh namespace's allocation as its administrator, so it does not establish owner-management API authorization. Node TLS is verified; owned Redis uses local plaintext TCP. Host executables are development builds, not release performance inputs. Actual guest reboot, public DNS/firewall/Internet delivery, managed Redis failover and performance/availability SLAs remain unverified. No throughput/latency score or competitor performance win follows from these functional runs.
