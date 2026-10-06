from pathlib import Path
import hashlib,json,importlib.util
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/"manifest.json").read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
spec=importlib.util.spec_from_file_location("analysis",root/"analysis.py");m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
summary=m.analyze(root/"report.json");assert summary==json.loads((root/"summary.json").read_text()) and summary["profile_success"]
for engine in summary["engines"].values():assert engine["passed"]==engine["attempted"]==engine["memory_batches"]==8 and not engine["failures"]
r=json.loads((root/"report.json").read_text());context=json.loads((root/"context.json").read_text())
assert json.loads((root/"terminal.json").read_text())["returncode"]==0
for name,digest in context["inputs_sha256"].items():assert r["artifact_sha256"][name]==digest
for name,field in [("bench-local-engines-concurrent.py","harness"),("bench-local-engines.py","shared_harness"),("bench-firecracker-local.py","firecracker_harness")]:assert hashlib.sha256((root/name).read_bytes()).hexdigest()==r["artifact_sha256"][field]
for batch in r["batches"]:assert batch["success"] and batch["all_guests_validated_while_held"] and batch["memory_idle_actual_seconds"]>=5 and batch["guest_idle_at_measurement_start_ms"]["min"]>=5000
print("Verified exact attempt coverage, frozen inputs/harnesses, quantiles, PSS, idle holds and cleanup.")

assert r["daemon_allocator_arena_max"]==context["allocator_experiment"]==2
assert "--daemon-allocator-arena-max" in context["argv"]
