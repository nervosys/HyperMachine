from pathlib import Path
import hashlib,json,subprocess,sys
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/"manifest.json").read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
context=json.loads((root/"context.json").read_text())
assert context["operation"]=="cold-create-with-exact-command" and context["template_mode"]=="disabled"
for d in root.glob("cohort-*"):
    text=(d/"daemon.txt").read_text()
    assert text.count("loaded linux boot image")==18 and "restored from" not in text
subprocess.run([sys.executable,str(root/"verify-metrics.py"),str(root)],check=True)
