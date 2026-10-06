# Native gateway Redis outage and AOF recovery

The owned executable checker now stops and reaps its Redis process while the gateway and TLS node fixture remain live. A confirmed active TCP session closes, the native TCP listener becomes unavailable and the same gateway process remains alive. Redis restarts on the same private Unix socket and owned AOF directory; its complete public-port reservation matches the pre-outage record. Without restarting the gateway, new native TCP and UDP traffic succeeds on that exact port.

All thirteen checks passed, including the previous operator fixture's exact 1 MiB TCP, two peers' empty/binary/65,507-byte UDP datagrams, protocol updates, pause/resume, corrupt index refusal/correction, graceful signals, gateway process restart and deletion cleanup. The accepted binary is the identical immutable `/var/tmp/hm-native-gateway-operator-v2` input used by the [operator executable archive](../native-gateway-operator/README.md); its SHA-256 matches that archive and report.json. Only the checker changed. Checker/binary hashes are verified before and after. No Rust implementation change or rebuild was needed to establish this previously missing behavior.

The fixture uses only newly generated temporary certificates/credentials, an owned loopback mutual-TLS echo server and private Unix-socket Redis namespace. It seeds records directly as the fixture administrator; no owner-management API behavior is established. Redis uses AOF with appendfsync always and receives an orderly SIGTERM. Both Redis processes were reaped, both gateway runs exited gracefully, all node threads joined and the private fixture directory/credentials were removed. No keys/certificates are archived, and logs are checked for credential content. Source, exact logs and report are hashed in manifest.json.

Reproduction:

```sh
python3 tools/check-native-gateway.py --gateway /absolute/path/hv2-native-gateway --output /fresh/output/directory
```

These checks establish recovery from an owned local Redis process outage with orderly AOF restart and the gateway process retained. They do not establish physical power-loss durability, managed Redis failover/partitions, Redis TLS outage recovery, real guests, guest reboot, Internet operation, fleet behavior or an availability/performance SLA. TCP callers reconnect after closure; UDP callers establish new peer sessions when delivery resumes. No competitor feature/performance win is claimed.
