import gzip, hashlib, json
from pathlib import Path

def entries(path):
    raw = gzip.decompress(path.read_bytes())
    result = {}
    offset = 0
    while True:
        header = raw[offset:offset+110]
        assert header[:6] == b'070701'
        fields = [int(header[6+8*i:14+8*i],16) for i in range(13)]
        offset += 110
        name = raw[offset:offset+fields[11]-1].decode()
        offset = (offset+fields[11]+3)//4*4
        data = raw[offset:offset+fields[6]]
        offset = (offset+fields[6]+3)//4*4
        if name == 'TRAILER!!!': break
        assert name not in result
        result[name] = (fields[1],data)
    return result

source = Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')
output = Path('/var/tmp/hm-competitive/guest-no-agent.cpio.gz')
a,b = entries(source),entries(output)
assert set(a) == set(b)
changed = [name for name in a if a[name][1] != b[name][1]]
assert changed == ['init'], changed
assert a['bin/hv2-guest-agentd'] == b['bin/hv2-guest-agentd']
assert a['bin/busybox'] == b['bin/busybox']
report = dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
    output_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
    changed_contents=changed,changed_modes=[name for name in a if a[name][0] != b[name][0]],
    agent_and_busybox_exact=True,entry_sets_equal=True,
    verifier_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
Path('/var/tmp/hm-competitive/no-agent-image-verification.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report))
