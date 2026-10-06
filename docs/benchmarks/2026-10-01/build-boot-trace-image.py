import gzip, hashlib, json, subprocess, tempfile
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
source = Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')
output = Path('/var/tmp/hm-competitive/guest-boot-trace.cpio.gz')
archive = gzip.decompress(source.read_bytes())
names = subprocess.check_output(['cpio', '-t', '--quiet'], input=archive).decode().splitlines()
assert all(not Path(name).is_absolute() and '..' not in Path(name).parts for name in names)
with tempfile.TemporaryDirectory(prefix='hm-boot-image-', dir='/var/tmp') as directory:
    subprocess.run(['cpio', '-id', '--quiet', '--no-absolute-filenames'], input=archive,
        cwd=directory, check=True)
    folder = Path(directory)
    old_init = (folder/'init').read_bytes()
    init = (root/'tools/guest-image/init').read_bytes().replace(b'\r\n', b'\n')
    # Explicit opt-in in this diagnostic image, identical for both engines.
    init = init.replace(b'boot_trace core_mounts_ready',
        b'export HV2_BOOT_TRACE=1\nboot_trace core_mounts_ready')
    (folder/'init').write_bytes(init)
    packed = subprocess.check_output(['bash', '-c',
        'find . -print0 | LC_ALL=C sort -z | xargs -0 touch -h -d @0; '
        'find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc -R 0:0 --reproducible --quiet'], cwd=directory)
    output.write_bytes(gzip.compress(packed, compresslevel=9, mtime=0))
    report = {'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
        'output_sha256':hashlib.sha256(output.read_bytes()).hexdigest(),
        'old_init_sha256':hashlib.sha256(old_init).hexdigest(),
        'diagnostic_init_sha256':hashlib.sha256(init).hexdigest(),
        'builder_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'change':'Only init content changes; diagnostic opt-in before core_mounts_ready',
        'diagnostic_init':init.decode()}
    (output.with_suffix('.build.json')).write_text(json.dumps(report, indent=2))
    print(json.dumps({key:value for key,value in report.items() if key != 'diagnostic_init'}))
