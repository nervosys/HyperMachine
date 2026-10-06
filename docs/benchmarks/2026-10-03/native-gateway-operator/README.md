# Native gateway operator executable

`hv2-native-gateway` now runs the owned native supervisor from explicit bind/store/namespace settings and required operator mutual TLS. Environment-only `HV2_STORE_URL`/`HV2_CLUSTER_TOKEN` provide store and node credentials. Offline `--check` validates URL syntax, limits, node TLS identity and node credential format without connecting or binding; it does not validate store reachability or store TLS environment files. Configuration/errors do not echo credentials. Unix SIGINT/SIGTERM shut down owned relay tasks. Default and configurable session/listener limits are described in the [operator guide](../../../NATIVE_PORT_GATEWAY.md).

The new checker launches only owned Redis on a private Unix socket, an owned mutual-TLS loopback node echo server, temporary Ed25519 credentials and the immutable gateway binary. It seeds a fresh namespace's records directly as a fixture administrator: this is not owner-management API evidence. Eleven accepted checks cover offline validation without a store, exact 1 MiB TCP, two independent UDP peers with empty/binary/65,507-byte datagrams, same-port protocol update and old TCP closure, pause with retained reservation, same-port resume, corrupt global Redis index closure and correction/recovery, SIGTERM cleanup, executable process restart on the same port, owned sandbox/index removal closure and SIGINT cleanup. The node validates HTTP/1 ALPN, exact target/protocol and configured cluster token and refuses an ingress API-key header. Fixture logs are checked for credential content.

Accepted binary: `/var/tmp/hm-native-gateway-operator-v2`, whose SHA-256 is in report.json. The initial v1 input/run is preserved separately and excluded from this accepted final-source archive. Both fixture runs used fresh output directories; the accepted checker verifies binary/source hashes before and after. Accepted fixture `/var/tmp/hm-native-gateway-operator-fixture-v2`: all eleven checks passed; gateway/Redis were reaped, all node threads joined and the private credential directory removed. No generated key/certificate files are archived.

Validation: executable configuration test 1 passed; isolated cluster library 82 passed, one ignored; control-plane integration 28 passed. Optional Redis tests are not additional external Redis evidence in that default library run; this new process fixture provides explicit owned Redis evidence. Builds ran only in `/var/tmp/hm-egress-log-mA2CCL` with accepted core hashes. Exact operator/native modules, workspace manifest/lock and cluster manifest matched the root's permitted source files. Protected root core files were neither read nor built. Source snapshots, logs and report are hashed in manifest.json.

Reproduction after building the operator executable from the aligned isolated source:

```sh
python3 tools/check-native-gateway.py --gateway /absolute/path/hv2-native-gateway --output /fresh/output/directory
```

This establishes executable/Redis/mutual-TLS echo behavior and gateway process restart while Redis stays alive. It does not establish real guest or guest-reboot delivery, Redis outage restart behavior, public DNS/firewall operation, owner API authorization, sustained resource bounds or competitor performance superiority. No deployment or publication was performed.
