import hashlib,json,sys
from pathlib import Path
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
for name,digest in json.loads((root/"manifest.json").read_text()).items():
    assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
final=json.loads((root/"final/report.json").read_text())
assert final["success"] and final["final_inventory_empty"] and final["inputs_unchanged"]
assert final["owned_directory_removed"] and not final["cleanup_errors"]
assert len(final["cases"])==10 and all(case["success"] for case in final["cases"])
assert len({case["name"] for case in final["cases"]})==10
assert len(final["processes"])==5 and all(row["stopped"] for row in final["processes"])
assert final["input_sha256"]==final["input_sha256_after"]
assert hashlib.sha256((root/"final/driver.py").read_bytes()).hexdigest()==final["input_sha256"]["driver"]
build=json.loads((root/"release-context.json").read_text())
for name in ["control","cli"]:assert build["artifacts"][name]["sha256"]==final["input_sha256"][name]
check=json.loads((root/"independent-check.json").read_text())
assert check["all_pass"] and check["inventory_empty"] and check["owned_directory_absent"]
assert check["release_context_sha256"]==hashlib.sha256((root/"release-context.json").read_bytes()).hexdigest()
initial=json.loads((root/"initial/report.json").read_text())
assert not initial["success"] and not initial["cases"]
assert "key usage extension" in initial["error"]
assert all(row["stopped"] for row in initial["processes"]) and initial["owned_directory_removed"]
source=(root/"final/driver.py").read_text()
assert "ssl.create_default_context(cafile=" in source and "keyUsage=critical,keyCertSign,cRLSign" in source
assert "CERT_NONE" not in source and "check_hostname=False" not in source
assert "owner-revocation-does-not-wake-paused-guest" in source
print(json.dumps({"manifest":"verified","real_kvm_cases":10,"stopped_final_processes":5,"inventory_empty":True,"strict_ca_failure_retained":True,"release_hashes_match":True},indent=2))
