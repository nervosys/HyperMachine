import hashlib,json,re,sys
from pathlib import Path
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
for name,h in json.loads((root/"manifest.json").read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==h,name
report=json.loads((root/"report.json").read_text())
assert report["success"] and report["redis_outage_requested"]
assert len(report["cases"])==13 and len({row["name"] for row in report["cases"]})==13 and all(row["success"] for row in report["cases"])
assert report["final_inventory_empty"] and not report["cleanup_errors"] and report["owned_directory_removed"]
assert report["inputs_unchanged"] and report["input_sha256"]==report["input_sha256_after"]
assert len(report["processes"])==7 and all(row["stopped"] for row in report["processes"])
assert report["no_wake_checked_via_independent_node_mtls"]
assert len(report["outage_denial_elapsed_ms"])==2 and all(0<=ms<8000 for ms in report["outage_denial_elapsed_ms"])
assert hashlib.sha256((root/"candidate-driver.py").read_bytes()).hexdigest()==report["input_sha256"]["driver"]
ctx=json.loads((root/"release-context.json").read_text())
for name in ["control","cli"]:assert ctx["artifacts"][name]["sha256"]==report["input_sha256"][name]
logs="\n".join((root/"raw"/name).read_text() for name in ["redis.log.txt","redis-active-recovered.log.txt","redis-revoked-recovered.log.txt"])
pids=re.findall(r"(?m)^(\d+):C .*Redis is starting",logs)
assert len(pids)==3 and len(set(pids))==3
assert logs.count("DB loaded from append only file:")==2
check=json.loads((root/"independent-check.json").read_text());assert check["inventory_empty"] and check["owned_directory_absent"] and check["all_seven_processes_waited"]
source=(root/"candidate-driver.py").read_text()
assert "redis.kill();redis.wait(timeout=10)" in source and "node_context.load_cert_chain" in source
assert 'assert local_state()=="paused"' in source and "CERT_NONE" not in source
print(json.dumps({"manifest":"verified","kvm_cases":13,"redis_processes":3,"aof_reloads":2,"stopped_processes":7,"no_wake":"independent node mTLS state","scope":"functional local outage recovery"},indent=2))
