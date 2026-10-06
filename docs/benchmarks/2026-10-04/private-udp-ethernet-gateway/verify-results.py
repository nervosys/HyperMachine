from pathlib import Path
p=Path(__file__).parent;s=(p/'network-tests.txt').read_text();assert '200 passed; 0 failed' in s
for name in ['private_udp_ethernet_preserves_empty_binary_and_payload_with_internet_disabled','private_udp_refusal_never_falls_back_under_permissive_internet_policy','private_udp_shares_tcp_admission_and_gateway_drop_releases_transport']:assert 'gateway::tests::'+name+' ... ok' in s
assert '64 passed; 0 failed; 2 ignored' in (p/'daemon-tests.txt').read_text()
assert '199 passed; 1 failed' in (p/'excluded-short-deadline-tests.txt').read_text()
print('200 network/64 daemon tests and Ethernet UDP exact/refusal/admission/teardown groups verified.')
