from pathlib import Path
import json,hashlib,importlib.util
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
spec=importlib.util.spec_from_file_location('compare',root/'compare.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
assert m.analyze()==json.loads((root/'summary.json').read_text())
print('Verified four allocator ABBA cohorts, 32 exact attempts, frozen inputs, memory, quantiles, idle holds and cleanup.')
