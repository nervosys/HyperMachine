from pathlib import Path
p=Path(__file__).parent;s=(p/'network-tests.txt').read_text();assert '197 passed; 0 failed' in s
for name in ['exact_empty_binary_and_maximum_datagrams_preserve_boundaries','oversized_and_truncated_frames_close_the_session','backpressure_is_bounded_and_idle_or_lifetime_closes','consumer_close_and_cancellation_release_transport']:assert 'private_udp::tests::'+name+' ... ok' in s
assert '64 passed; 0 failed; 2 ignored' in (p/'daemon-tests.txt').read_text()
print('197 network/64 daemon tests and four private UDP session proof groups verified.')
