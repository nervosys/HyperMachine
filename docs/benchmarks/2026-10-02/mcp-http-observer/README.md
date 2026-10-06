# MCP inherits the configured observer API role

A separate MCP HTTP process used an expiring, hashed upstream API policy with role observer and scope admin. Role enforcement capped the broad scope: the official Python MCP client 1.23.3 could list real guest inventory, but thirteen operations were refused. These were create, credential-bearing inspect, exec, pause, resume, fork, delete, checkpoint save/list/restore/delete, binary upload and binary download. Both inventory calls succeeded, the session remained usable, and credential-bearing descriptor fields were absent from results.

An independent authenticated control-plane TLS request confirmed that observer exec receives 403. An operator created the target KVM guest and wrote a unique marker. After all observer calls, the operator could still execute commands and read the unchanged marker; inventory was unchanged. The operator then deleted the fixture guest, and final inventory was empty.

The same run retained the 27-operation operator HTTPS/KVM regression, including real cancellation with continued accepted work, bounded binary transfer, checkpoint restore, pause/resume, fork isolation and cleanup. All five owned processes exited zero with no cleanup errors, and all artifacts remained unchanged. The fixture uses distinct MCP processes and Bearer credentials for the two configured roles. It does not implement or prove per-user role selection or tenant isolation within one MCP endpoint.

The previously used reserved-alias control plane predates explicit observer roles. This run used the accepted observer-capable private-web control plane, SHA 3acfd4e3a645888efeb2165ba031d94600f720f060f7fb36889c3afc37aa0280, whose prior build/test evidence is in ../private-web. The CLI remains the cancellation-verified release, SHA fd55361af11bf5dd10d6ffa84a81d6b9200a66079bb4ece2c72a73ff13b55ed1; no runtime rebuild or source change was required. Its isolated 144-test/Clippy/build provenance is retained in ../mcp-http-cancellation.

```sh
python -O tools/check-mcp-http-kvm.py --output /new/owned/run --daemon /path/node --control-plane /path/observer-capable-control --cli /path/hm --kernel /path/kernel --initrd /path/guest --files --check-cancellation --observer
```

Credentials, certificates, guest data and binaries are excluded. This is an owned verified-HTTPS, control-plane TLS, node mTLS and real KVM fixture. It establishes configured role enforcement through the MCP bridge, not OAuth/browser login, multi-user endpoint isolation, production proxy operation, or a performance ranking. Prior archives remain immutable.
