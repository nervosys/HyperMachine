from pathlib import Path
import hashlib, json, subprocess, sys
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/"manifest.json").read_text()).items():
    assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
context=json.loads((root/"source-context.json").read_text())
assert context["release_binary_sha256"]==json.loads((root/"context.json").read_text())["binary_sha256"]
subprocess.run([sys.executable,str(root/"verify-metrics.py"),str(root)],check=True)
