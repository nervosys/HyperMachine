import gzip, hashlib, json, subprocess, tempfile
from pathlib import Path

source = Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')
output = Path('/var/tmp/hm-competitive/guest-no-agent.cpio.gz')
archive = gzip.decompress(source.read_bytes())
assert len(archive) <= 64*1024*1024
names = subprocess.check_output(['cpio','-t','--quiet'],input=archive).decode().splitlines()
assert all(not Path(name).is_absolute() and '..' not in Path(name).parts for name in names)
with tempfile.TemporaryDirectory(prefix='hm-no-agent-image-',dir='/var/tmp') as directory:
    subprocess.run(['cpio','-id','--quiet','--no-absolute-filenames'],input=archive,cwd=directory,check=True)
    init_path = Path(directory)/'init'
    init = init_path.read_bytes()
    assert init.count(b'/bin/hv2-guest-agentd &') == 1
    changed = init.replace(b'/bin/hv2-guest-agentd &',b'# Negative control: omit guest agent launch')
    init_path.write_bytes(changed)
    packed = subprocess.check_output(['bash','-c',
        'find . -print0 | LC_ALL=C sort -z | xargs -0 touch -h -d @0; '
        'find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc -R 0:0 --reproducible --quiet'],cwd=directory)
    output.write_bytes(gzip.compress(packed,compresslevel=9,mtime=0))
report = dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
    output_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
    builder_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    diagnostic_only=True,change='Single init agent-launch line replaced by comment',diagnostic_init=changed.decode())
output.with_suffix('.build.json').write_text(json.dumps(report,indent=2))
print(json.dumps({key:value for key,value in report.items() if key != 'diagnostic_init'}))
