import shutil
from pathlib import Path
out=Path('/var/tmp/hm-refusal-retry')
shutil.copyfile(out/'commands-matrix.json',out/'failed-setup-matrix.json')
shutil.copyfile(Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine/target/refusal-command-matrix.py'),out/'failed-setup-matrix-source.py')
for name in ['hv2-sandboxd-baseline','hv2-sandboxd-candidate']:
 path=out/name;path.chmod(0o755)