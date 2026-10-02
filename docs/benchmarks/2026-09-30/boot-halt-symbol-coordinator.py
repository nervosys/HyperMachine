import importlib.util,json,hashlib
from pathlib import Path
from types import SimpleNamespace
root=Path("/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine")
spec=importlib.util.spec_from_file_location("fc",root/"tools/bench-firecracker-local.py")
fc=importlib.util.module_from_spec(spec);spec.loader.exec_module(fc)
args=SimpleNamespace(firecracker=Path("/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64"),kernel=Path("/var/tmp/hm-competitive/bzImage"),initrd=Path("/var/tmp/hm-competitive/guest-output-drain.cpio.gz"),timeout=30)
paths={name:getattr(args,name) for name in ("firecracker","kernel","initrd")}
paths.update(harness=Path(fc.__file__),coordinator=Path(__file__))
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
before={name:digest(path) for name,path in paths.items()}
original=fc.rpc
observed={}
def diagnostic(stream,request_id,body):
 result=original(stream,request_id,body)
 if request_id==2 and body.get("kind")=="exec":
  command="grep -E '^ffffffff81eda950 |^ffffffff816481' /proc/kallsyms"
  observed.update(command=command,response=original(stream,3,{"kind":"exec","program":"/bin/sh","args":["-c",command],"timeout_ms":10000}))
 return result
fc.rpc=diagnostic
row=fc.sample(args,0)
after={name:digest(path) for name,path in paths.items()}
report={"diagnostic_only":True,"sample":row,"symbol_query":observed,"artifact_sha256":before,"artifacts_unchanged":before==after}
Path("/var/tmp/hm-competitive/boot-halt-symbol.json").write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
