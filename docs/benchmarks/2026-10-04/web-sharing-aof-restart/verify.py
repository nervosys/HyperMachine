import hashlib, json, re, sys
from pathlib import Path
root = Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
for name, digest in json.loads((root/"manifest.json").read_text()).items():
    assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest, name
text=(root/"final-tests.txt").read_text()
assert "1 passed; 0 failed; 0 ignored" in text
for phase in range(1,6): assert f"sharing AOF restart={phase}" in text
pids=re.findall(r"(?m)^(\d+):C .*Redis is starting",text)
assert len(pids)==6 and len(set(pids))==6, pids
assert text.count("DB loaded from append only file:")==5
assert "corrupt JSON and wrong Redis type deny admission and updates" in text
assert "broken pipe" in (root/"initial-tests.txt").read_text()
check=json.loads((root/"cleanup-check.json").read_text())
assert check["test_exit_code"]==0 and all(check["five_restart_markers"])
assert check["owned_directory_removed"] and check["owned_process_absent"]
source=(root/"candidate-store.rs").read_text()
start=source.index('    #[tokio::test]\n    #[ignore = "launches an owned AOF Redis server; invoke explicitly on Linux"]')
end=source.index('    async fn owned_port_contract',start)
assert source[:start]+source[end:]==(root/"before-store.rs").read_text(), "production or existing test/helper changed"
assert '"--appendfsync", "always"' in source and '"--appendonly", "yes"' in source
assert 'self.child.kill().unwrap(); self.child.wait().unwrap();' in source
assert hashlib.sha256((root/"candidate-store.rs").read_bytes()).hexdigest()==check["candidate_store_sha256"]
print(json.dumps({"manifest":"verified","test":"one explicit ignored gate passed","distinct_redis_processes":6,"aof_reloads":5,"production_unchanged":True,"cleanup":"independently checked after test"},indent=2))
