#!/usr/bin/env python3
"""Schedule Certbot renewal checks and reconcile verified HyperMachine deployment.

Linux operator worker for existing certificate lineages. --watch runs the built-in
scheduler; the default performs one due cycle. A private durable journal and lock
prevent overlapping workers and recover interrupted deployment on restart.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit


spec = importlib.util.spec_from_file_location("tls_deployment", Path(__file__).with_name("deploy-tls-certificate.py"))
deployment = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deployment)
require = deployment.require
STOP = threading.Event()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def fingerprint(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def absolute(value):
    require(isinstance(value, str) and Path(value).is_absolute() and len(value) <= 4096,
            "configuration paths must be absolute and bounded")
    return value


def integer(value, low, high, label):
    require(type(value) is int and low <= value <= high, "invalid " + label)
    return value


def configuration(raw, allow_empty=False):
    required = {"certbot", "config_dir", "work_dir", "logs_dir", "jobs"}
    optional = {"interval_seconds", "retry_seconds", "command_timeout_seconds", "server", "acme_ca_file"}
    require(isinstance(raw, dict) and required <= raw.keys() <= required | optional,
            "invalid renewal configuration fields")
    result = dict(raw)
    for key in ("certbot", "config_dir", "work_dir", "logs_dir"):
        absolute(result[key])
    for key, default, low, high in (("interval_seconds", 43200, 1, 604800),
                                    ("retry_seconds", 300, 1, 86400),
                                    ("command_timeout_seconds", 180, 5, 1800)):
        result[key] = integer(result.get(key, default), low, high, key)
    if "server" in result:
        require(isinstance(result["server"], str) and len(result["server"]) <= 4096, "invalid ACME server")
        server = urlsplit(result["server"])
        require(server.scheme == "https" and server.hostname and not server.username
                and not server.password and not server.query and not server.fragment, "ACME server requires HTTPS without credentials")
        _ = server.port
    if "acme_ca_file" in result:
        absolute(result["acme_ca_file"])
    require(isinstance(result["jobs"], list) and (0 if allow_empty else 1) <= len(result["jobs"]) <= 64, "invalid renewal job count")
    identities, groups = set(), set()
    jobs = []
    for source in result["jobs"]:
        fields = {"id", "cert_name", "lineage", "manifest", "generations", "control_plane", "domains"}
        extra = {"port", "connect_address", "ca_file", "pid_file", "activation_timeout_seconds", "initial_issuance", "domain_claim"}
        require(isinstance(source, dict) and fields <= source.keys() <= fields | extra, "invalid renewal job fields")
        job = dict(source)
        for key in ("id", "cert_name"):
            require(isinstance(job[key], str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", job[key]), "invalid renewal identity")
        require(job["id"] not in identities, "duplicate renewal job identity")
        identities.add(job["id"])
        for key in ("lineage", "manifest", "generations", "control_plane", "ca_file", "pid_file"):
            if key in job:
                absolute(job[key])
        require(Path(job["lineage"]) == Path(result["config_dir"]) / "live" / job["cert_name"],
                "lineage must match the configured Certbot directory and certificate name")
        require(isinstance(job["domains"], list) and 1 <= len(job["domains"]) <= 128, "invalid renewal domains")
        job["domains"] = sorted(deployment.name(value) for value in job["domains"])
        require(len(set(job["domains"])) == len(job["domains"]), "duplicate renewal domain")
        group = (job["manifest"], tuple(job["domains"]))
        require(group not in groups, "duplicate deployment group")
        groups.add(group)
        job["port"] = integer(job.get("port", 443), 1, 65535, "TLS port")
        job["activation_timeout_seconds"] = integer(job.get("activation_timeout_seconds", 10), 1, 60, "activation timeout")
        job["connect_address"] = job.get("connect_address", "127.0.0.1")
        if "domain_claim" in job:
            claim = job["domain_claim"]
            require(isinstance(claim, dict) and {"origin", "sandbox_id", "domain", "port"} <= claim.keys()
                    <= {"origin", "sandbox_id", "domain", "port", "ca_file"}, "invalid domain claim guard")
            require(isinstance(claim["origin"], str), "invalid claim origin")
            origin = urlsplit(claim["origin"])
            require(origin.scheme == "https" and origin.hostname and not origin.username and not origin.password
                    and origin.path in ("", "/") and not origin.query and not origin.fragment, "claim origin requires HTTPS")
            _ = origin.port
            owner = claim["sandbox_id"]
            require(isinstance(owner, str) and 1 <= len(owner) <= 128
                    and all(c.isascii() and (c.isalnum() or c in "-_") for c in owner), "invalid claim owner")
            require(job["domains"] == [deployment.name(claim["domain"])], "claim guard must match one job hostname")
            integer(claim["port"], 1, 65535, "claim guest port")
            if "ca_file" in claim:
                absolute(claim["ca_file"])
        if "initial_issuance" in job:
            initial = job["initial_issuance"]
            require(isinstance(initial, dict) and initial.keys() == {"webroot", "email", "agree_tos"}
                    and initial["agree_tos"] is True, "initial issuance requires explicit account and terms configuration")
            absolute(initial["webroot"])
            require(isinstance(initial["email"], str) and len(initial["email"]) <= 254
                    and re.fullmatch(r"[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9.-]+", initial["email"]),
                    "invalid initial issuance account email")
        require(isinstance(job["connect_address"], str), "invalid connection address")
        deployment.ipaddress.ip_address(job["connect_address"])
        jobs.append(job)
    result["jobs"] = jobs
    return result


def owned_file(path, limit):
    data, metadata = deployment.read_file(path, limit)
    require(metadata.st_uid == os.geteuid() and metadata.st_nlink == 1 and metadata.st_mode & 0o022 == 0,
            "configuration/state file must be owned and not writable by others")
    return data


def selected_pid(job):
    if "pid_file" in job:
        raw = owned_file(Path(job["pid_file"]), 32)
        require(re.fullmatch(rb"[0-9]+\n?", raw), "invalid control-plane PID file")
        return integer(int(raw), 2, 2 ** 31 - 1, "control-plane PID")
    target = Path(job["control_plane"]).stat()
    found = []
    for directory in Path("/proc").iterdir():
        if not directory.name.isdigit():
            continue
        try:
            if directory.stat().st_uid != os.geteuid():
                continue
            executable = (directory / "exe").stat()
            if (target.st_dev, target.st_ino) != (executable.st_dev, executable.st_ino):
                continue
            raw, _ = deployment.read_file(directory / "cmdline", 65536)
            argv = raw.rstrip(b"\0").decode().split("\0")
            if (argv.count("--tls-bundle-file") == 1
                    and argv[argv.index("--tls-bundle-file") + 1] == job["manifest"]):
                found.append(int(directory.name))
        except (OSError, UnicodeError, IndexError, ValueError):
            continue
    require(len(found) == 1, "expected exactly one matching owned control-plane process")
    return found[0]


def command(argv, env, timeout, capture=False, stop=STOP):
    require(not stop.is_set(), "worker stopping")
    process = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        start_new_session=True)
    deadline = time.monotonic() + timeout
    try:
        while True:
            require(not stop.is_set(), "worker stopping")
            remaining = deadline - time.monotonic()
            require(remaining > 0, "command timed out")
            try:
                output, _ = process.communicate(timeout=min(1, remaining))
                require(not capture or len(output) <= 65536, "deployment output exceeds limit")
                return process.returncode, output
            except subprocess.TimeoutExpired:
                pass
    finally:
        # Include descendants (Certbot plugins/hooks) in termination, so a timed
        # out attempt cannot continue concurrently with its durable retry.
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=2)


def verify_claim(job, collect=None):
    claim = job.get("domain_claim")
    if claim is None:
        return
    key = os.environ.get("HV2_DOMAIN_DISCOVERY_API_KEY")
    require(key, "domain discovery credential is unavailable")
    if collect is None:
        spec = importlib.util.spec_from_file_location("claim_discovery", Path(__file__).with_name("domain-certificate-jobs.py"))
        discovery = importlib.util.module_from_spec(spec); spec.loader.exec_module(discovery)
        collect = discovery.bounded_inventory
    records = collect(claim["origin"], key, claim.get("ca_file"))
    expected = {"domain": deployment.name(claim["domain"]), "sandbox_id": claim["sandbox_id"], "port": claim["port"]}
    require([row for row in records if row["domain"] == expected["domain"]] == [expected],
            "domain claim changed or disappeared; issuance and deployment refused")


def renew(config, job, force=False):
    verify_claim(job)
    argv = [config["certbot"], "renew", "--non-interactive", "--no-random-sleep-on-renew",
            "--no-directory-hooks", "--deploy-hook", "", "--cert-name", job["cert_name"],
            "--config-dir", config["config_dir"], "--work-dir", config["work_dir"],
            "--logs-dir", config["logs_dir"]]
    # Existing or interrupted Certbot state must go through renewal; never
    # interpret a broken lineage symlink or saved renewal config as first use.
    initial = job.get("initial_issuance")
    first = initial is not None and not os.path.lexists(job["lineage"]) and not os.path.lexists(
        Path(config["config_dir"]) / "renewal" / (job["cert_name"] + ".conf"))
    if first:
        webroot = Path(initial["webroot"]).lstat()
        require(stat.S_ISDIR(webroot.st_mode) and webroot.st_uid == os.geteuid()
                and webroot.st_mode & 0o022 == 0, "initial issuance webroot must be owned and not writable by others")
        argv = [config["certbot"], "certonly", "--non-interactive", "--webroot", "--webroot-path", initial["webroot"],
                "--email", initial["email"], "--agree-tos", "--no-directory-hooks", "--deploy-hook", "",
                "--cert-name", job["cert_name"], "--config-dir", config["config_dir"],
                "--work-dir", config["work_dir"], "--logs-dir", config["logs_dir"]]
        for domain in job["domains"]:
            argv += ["--domain", domain]
    if "server" in config:
        argv += ["--server", config["server"]]
    if force and not first:
        argv += ["--force-renewal", "--new-key"]
    env = dict(os.environ)
    if "acme_ca_file" in config:
        env["REQUESTS_CA_BUNDLE"] = config["acme_ca_file"]
    code, _ = command(argv, env, config["command_timeout_seconds"])
    return code


def activate(config, job):
    verify_claim(job)
    pid = selected_pid(job)
    argv = [sys.executable, str(Path(__file__).with_name("deploy-tls-certificate.py").resolve()),
            "--manifest", job["manifest"], "--lineage", job["lineage"],
            "--generations", job["generations"], "--control-plane", job["control_plane"],
            "--pid", str(pid), "--port", str(job["port"]), "--connect-address", job["connect_address"],
            "--timeout", str(job["activation_timeout_seconds"])]
    if "ca_file" in job:
        argv += ["--ca-file", job["ca_file"]]
    if "initial_issuance" in job:
        argv += ["--provision-new-group"]
    for domain in job["domains"]:
        argv += ["--domain", domain]
    code, output = command(argv, dict(os.environ), config["command_timeout_seconds"], capture=True)
    require(code == 0, "certificate activation failed; inspect the lineage and active manifest")
    receipt = json.loads(output)
    require(receipt.get("success") is True and receipt.get("activation_verified") is True
            and re.fullmatch(r"[0-9a-f]{64}", receipt.get("active_leaf_sha256", "")), "invalid activation receipt")
    return {"active_leaf_sha256": receipt["active_leaf_sha256"], "activation_verified": True,
            "unchanged": receipt.get("unchanged", False),
            "recovered_pending_deployment": receipt.get("recovered_pending_deployment", False)}


def new_state():
    return {"version": 1, "jobs": {}}


def validate_state(state):
    require(isinstance(state, dict) and state.keys() == {"version", "jobs"}
            and type(state["version"]) is int and state["version"] == 1
            and isinstance(state["jobs"], dict) and len(state["jobs"]) <= 64,
            "invalid renewal journal")
    for identity, value in state["jobs"].items():
        require(isinstance(identity, str) and isinstance(value, dict), "invalid journal job")
        fields = {"configuration_sha256", "next_check_at", "last_started_at", "pending", "last_result"}
        require(fields <= value.keys() <= fields | {"managed_job"}
                and re.fullmatch(r"[0-9a-f]{64}", value["configuration_sha256"]),
                "invalid journal job fields")
        if "managed_job" in value:
            job = value["managed_job"]
            require(isinstance(job, dict) and job.get("id") == identity and "domain_claim" in job,
                    "invalid managed deployment identity")
            require(isinstance(job.get("lineage"), str), "invalid managed lineage")
            # Reuse the strict job schema without executing any command. The
            # stored lineage determines its matching Certbot directory.
            configuration({"certbot": "/journal-validation", "config_dir": str(Path(job["lineage"]).parent.parent),
                           "work_dir": "/journal-validation", "logs_dir": "/journal-validation", "jobs": [job]})
        for key in ("next_check_at", "last_started_at"):
            integer(value[key], 0, 2 ** 63 - 1, "journal timestamp")
        pending = value["pending"]
        require(pending is None or (isinstance(pending, dict) and pending.keys() == {"phase", "renewal_exit"}
                and pending["phase"] in ("renewing", "deploying", "retry_deployment")
                and (pending["renewal_exit"] is None or type(pending["renewal_exit"]) is int)), "invalid pending renewal")
        if pending is not None:
            require(pending["phase"] != "renewing" or pending["renewal_exit"] is None,
                    "renewing journal cannot contain a completed command")
            require(pending["phase"] != "retry_deployment" or pending["renewal_exit"] == 0,
                    "deployment retry requires successful issuance")
        require(value["last_result"] is None or isinstance(value["last_result"], dict), "invalid renewal result")
    return state


def run_cycle(config, state, persist, now=None, force=False, renew_fn=renew, activate_fn=activate, stop=STOP):
    require(not force or now is None, "forced renewal cannot use a synthetic clock")
    current_time = (lambda: int(time.time())) if now is None else (lambda: now)
    global_config = {key: value for key, value in config.items() if key != "jobs"}
    summaries = []
    for job in config["jobs"]:
        if stop.is_set():
            break
        identity = job["id"]
        expected = fingerprint({"worker": global_config, "job": job})
        previous = state["jobs"].get(identity)
        if previous is None or previous["configuration_sha256"] != expected:
            require(previous is None or previous["pending"] is None,
                    "pending job configuration changed; reconcile before replacing the journal")
            previous = {"configuration_sha256": expected, "next_check_at": 0,
                        "last_started_at": 0, "pending": None, "last_result": None}
            state["jobs"][identity] = previous
        if "domain_claim" in job:
            managed = json.loads(canonical(job))
            if previous.get("managed_job") != managed:
                previous["managed_job"] = managed
                persist(state)
        start = current_time()
        waiting_retry = previous["pending"] is None or previous["pending"]["phase"] == "retry_deployment"
        if not force and waiting_retry and previous["last_started_at"] <= start < previous["next_check_at"]:
            summaries.append({"id": identity, "status": "not_due"})
            continue
        recovering_deployment = previous["pending"] is not None and previous["pending"]["phase"] in ("deploying", "retry_deployment")
        renewal_exit = previous["pending"]["renewal_exit"] if recovering_deployment else None
        previous["last_started_at"] = start
        previous["pending"] = {"phase": "deploying" if recovering_deployment else "renewing", "renewal_exit": renewal_exit}
        persist(state)
        renewal_error = None
        if not recovering_deployment:
            try:
                renewal_exit = renew_fn(config, job, force)
            except Exception:
                renewal_error = "renewal command failed, timed out or was interrupted"
            previous["pending"] = {"phase": "deploying", "renewal_exit": renewal_exit}
            persist(state)
        try:
            receipt = activate_fn(config, job)
            success = renewal_exit == 0 and renewal_error is None
            result = {"id": identity, "status": "verified" if success else "renewal_failed",
                      "success": success, "renewal_exit": renewal_exit, **receipt}
            if renewal_error:
                result["error"] = renewal_error
        except Exception:
            success = False
            result = {"id": identity, "status": "deployment_failed", "success": False,
                      "renewal_exit": renewal_exit, "activation_verified": False,
                      "error": "activation could not be verified; retained generation requires reconciliation"}
        finished = current_time()
        previous["last_result"] = result
        previous["next_check_at"] = finished + (config["interval_seconds"] if success else config["retry_seconds"])
        # Preserve successful issuance awaiting activation. A restart/retry skips
        # new issuance and reconciles the already-issued lineage immediately.
        previous["pending"] = None if success or renewal_exit != 0 else {"phase": "retry_deployment", "renewal_exit": 0}
        persist(state)
        summaries.append(result)
    return summaries


def discovered_configuration(config, state, settings, collect=None):
    require(isinstance(settings, dict) and {"origin", "allowed_suffixes", "template"} <= settings.keys()
            <= {"origin", "allowed_suffixes", "template", "ca_file"}, "invalid discovery configuration")
    key = os.environ.get("HV2_DOMAIN_DISCOVERY_API_KEY")
    require(key, "domain discovery credential is unavailable")
    spec = importlib.util.spec_from_file_location("worker_discovery", Path(__file__).with_name("domain-certificate-jobs.py"))
    discovery = importlib.util.module_from_spec(spec); spec.loader.exec_module(discovery)
    bindings = (collect or discovery.bounded_inventory)(settings["origin"], key, settings.get("ca_file"))
    planned = discovery.reconcile(config, settings["template"], settings["allowed_suffixes"], bindings, state,
                                  settings["origin"], settings.get("ca_file"))
    candidates = discovery.retirement_candidates(config, planned["configuration"], bindings, state, settings)
    require(not candidates, "completed managed job requires verified certificate retirement; journal retained")
    return planned


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--watch", action="store_true")
    parser.add_argument("--force-renewal", action="store_true", help="explicit testing/manual override; applies to one cycle only")
    parser.add_argument("--discovery-config", type=Path)
    args = parser.parse_args()
    require(not (args.watch and args.force_renewal), "forced renewal is incompatible with continuous scheduling")
    config = configuration(json.loads(owned_file(args.config, 1024 * 1024), object_pairs_hook=deployment.no_duplicate_keys),
                           allow_empty=bool(args.discovery_config))
    discovery_settings = (json.loads(owned_file(args.discovery_config, 1024 * 1024),
                                    object_pairs_hook=deployment.no_duplicate_keys) if args.discovery_config else None)
    require(args.state.is_absolute(), "journal path must be absolute")
    directory = args.state.parent.lstat()
    require(stat.S_ISDIR(directory.st_mode) and directory.st_uid == os.geteuid()
            and stat.S_IMODE(directory.st_mode) == 0o700, "journal directory must be owned and mode 0700")
    lock = os.open(args.state.with_name(args.state.name + ".lock"), os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        metadata = os.fstat(lock)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.geteuid()
                and metadata.st_nlink == 1 and stat.S_IMODE(metadata.st_mode) == 0o600, "unsafe journal lock")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            raw = owned_file(args.state, 1024 * 1024)
        except FileNotFoundError:
            state = new_state()
        else:
            state = validate_state(json.loads(raw, object_pairs_hook=deployment.no_duplicate_keys))
        def persist(value):
            raw = canonical(value) + b"\n"
            require(len(raw) <= 1024 * 1024, "journal exceeds size limit")
            deployment.atomic_write(args.state, raw, 0o600)
        def stopping(*_):
            STOP.set()
        signal.signal(signal.SIGTERM, stopping)
        signal.signal(signal.SIGINT, stopping)
        while not STOP.is_set():
            if discovery_settings is not None:
                config = discovered_configuration(config, state, discovery_settings)["configuration"]
            retained = {job["id"] for job in config["jobs"]}
            require(not any(value["pending"] is not None for key, value in state["jobs"].items() if key not in retained),
                    "removed job has pending deployment; reconcile before removing it")
            previous_jobs = state["jobs"]
            state["jobs"] = {key: value for key, value in previous_jobs.items() if key in retained}
            if state["jobs"] != previous_jobs or not args.state.exists():
                persist(state)
            summaries = run_cycle(config, state, persist, force=args.force_renewal)
            print(json.dumps({"jobs": summaries}), flush=True)
            if not args.watch:
                return int(any(value.get("success") is False for value in summaries))
            due = min((value["next_check_at"] for value in state["jobs"].values()), default=int(time.time()) + 1)
            STOP.wait(min(30, max(1, due - time.time())))
        return 0
    finally:
        os.close(lock)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        raise SystemExit("Renewal worker failed: " + str(error))
