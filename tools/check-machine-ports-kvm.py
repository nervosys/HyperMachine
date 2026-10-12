#!/usr/bin/env python3
"""Published ports on real KVM: a way in to a machine from the node's network.

Runs an owned hv2-sandboxd with a --machine-dir, three ports it may give out,
and one machine from the base template with no NIC, then checks:

1. A published port carries a connection to the guest's port: a file the
   guest serves over HTTP comes out whole, and the machine's record names
   the port.
2. Bytes go in as well as out: what is sent to a published port is what the
   guest's listener received.
3. Several connections at once each get the whole file.
4. A published port whose guest port has no listener accepts and closes.
5. What cannot be published is refused: outside the node's range, a port in
   use, every port taken, a port that is not one.
6. A port taken back refuses connections, and can be published again.
7. While the machine is stopped a connection is accepted and closed; when
   it is started again the port works without being published again.
8. After the node is restarted the port is still published and works.
9. Deleting the machine gives its ports back.

Writes report.json and the daemon's logs into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, threading, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "served-01"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def free_run(count):
    """The first of `count` consecutive ports nothing listens on."""
    for first in range(31000, 39000, 16):
        held = []
        try:
            for offset in range(count):
                sock = socket.socket()
                held.append(sock)
                sock.bind(("127.0.0.1", first + offset))
            return first
        except OSError:
            continue
        finally:
            for sock in held:
                sock.close()
    raise AssertionError("no run of free ports")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of machine published ports on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-ports-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    first = free_run(3)
    report["node_ports"] = [first, first + 2]
    node = {"daemon": None, "log": None, "starts": 0}

    def start_node():
        node["starts"] += 1
        node["log"] = (args.output / f"daemon-{node['starts']}.log").open("wb")
        node["daemon"] = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
             "--machine-dir", str(work / "machines"), "--machine-ports", f"{first}-{first + 2}",
             "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
            env=env, stdin=subprocess.DEVNULL, stdout=node["log"], stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 60
        while True:
            assert node["daemon"].poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)

    def stop_node():
        daemon = node["daemon"]
        if daemon is not None and daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()
        if node["log"] is not None:
            node["log"].close()

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=180):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, (cmd, status, value)
        return value["stdout"]

    def run(cmd):
        status, value = request("POST", f"/machines/{NAME}/runs", {"cmd": cmd})
        assert status == 201, (status, value)
        return value["runID"]

    def run_ended(run_id, seconds=30):
        deadline = time.monotonic() + seconds
        while True:
            status, value = request("GET", f"/machines/{NAME}/runs/{run_id}?stdout=0&stderr=0")
            if value["state"] != "running":
                return value
            assert time.monotonic() < deadline, value
            time.sleep(0.2)

    def serve():
        """The guest's web server, for as long as the machine runs."""
        run("exec httpd -f -p 8080 -h /root/www")
        deadline = time.monotonic() + 20
        while "8080" not in guest("netstat -ltn 2>/dev/null || true"):
            assert time.monotonic() < deadline, "the guest's server did not listen"
            time.sleep(0.2)

    def fetch(host_port, name, timeout=60):
        with urllib.request.urlopen(f"http://127.0.0.1:{host_port}/{name}", timeout=timeout) as response:
            return response.read()

    def closed_without_a_word(host_port):
        """Connect; what the node did with the connection."""
        with socket.create_connection(("127.0.0.1", host_port), timeout=10) as sock:
            sock.settimeout(15)
            try:
                sock.sendall(b"GET / HTTP/1.0\r\n\r\n")
                return sock.recv(64)
            except (ConnectionResetError, BrokenPipeError):
                return b""

    def publish(guest_port, host_port=None):
        body = {"guestPort": guest_port}
        if host_port is not None:
            body["hostPort"] = host_port
        return request("POST", f"/machines/{NAME}/ports", body)

    def listed():
        status, ports = request("GET", f"/machines/{NAME}/ports")
        assert status == 200, (status, ports)
        return ports

    try:
        start_node()
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1})
        assert status == 201 and created["state"] == "running" and created["ports"] == [], (status, created)
        assert created["network"] is None, "the check is of a machine with no NIC"
        state = {}
        # On its disk, so it is there after the machine is stopped and started.
        state["big"] = guest("mkdir -p /root/www && dd if=/dev/urandom of=/root/www/big.bin bs=1k count=5000 "
                             "2>/dev/null && echo small > /root/www/small.txt && sync && "
                             "sha256sum /root/www/big.bin").split()[0]
        serve()

        def carries_out():
            status, published = publish(8080)
            assert status == 201, (status, published)
            assert published == {"hostPort": first, "guestPort": 8080, "address": "127.0.0.1",
                                 "listening": True}, published
            assert fetch(first, "small.txt") == b"small\n"
            big = fetch(first, "big.bin")
            assert len(big) == 5000 * 1024 and hashlib.sha256(big).hexdigest() == state["big"]
            status, machine = request("GET", f"/machines/{NAME}")
            assert machine["ports"] == [{"hostPort": first, "guestPort": 8080}], machine
            return {"published": published, "bytes": len(big)}
        case("a published port carries a connection to the guest's port, and a file comes out whole",
             carries_out)

        def carries_in():
            status, published = publish(9001, first + 1)
            assert status == 201 and published["hostPort"] == first + 1, (status, published)
            receiving = run("nc -l -p 9001 > /root/received")
            deadline = time.monotonic() + 20
            while "9001" not in guest("netstat -ltn"):
                assert time.monotonic() < deadline
                time.sleep(0.2)
            sent = os.urandom(3 * 1024 * 1024 + 17)
            with socket.create_connection(("127.0.0.1", first + 1), timeout=30) as sock:
                sock.sendall(sent)
                sock.shutdown(socket.SHUT_WR)
                # The guest's end closing is the end of it.
                sock.settimeout(30)
                while sock.recv(4096):
                    pass
            ended = run_ended(receiving)
            assert ended["exitCode"] == 0, ended
            assert int(guest("wc -c < /root/received")) == len(sent)
            assert guest("sha256sum /root/received").split()[0] == hashlib.sha256(sent).hexdigest()
            return {"bytes": len(sent)}
        case("what is sent to a published port is what the guest's listener received", carries_in)

        def at_once():
            results, errors = [], []

            def one():
                try:
                    results.append(hashlib.sha256(fetch(first, "big.bin", 120)).hexdigest())
                except Exception as error:  # noqa: BLE001 - reported below
                    errors.append(repr(error))
            threads = [threading.Thread(target=one) for _ in range(8)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
            assert not errors, errors
            assert results == [state["big"]] * 8, results
            return {"connections": 8}
        case("several connections at once each get the whole file", at_once)

        def nothing_listening():
            status, published = publish(9999)
            assert status == 201 and published["hostPort"] == first + 2, (status, published)
            assert closed_without_a_word(first + 2) == b""
            return {"hostPort": first + 2}
        case("a published port whose guest port has no listener accepts and closes", nothing_listening)

        def refusals():
            results = {}
            for label, guest_port, host_port, want in [
                ("every port taken", 8081, None, 409),
                ("a port in use", 8081, first, 409),
                ("below the node's range", 8081, first - 1, 400),
                ("above the node's range", 8081, first + 3, 400),
                ("a low port", 8081, 22, 400),
                ("a guest port that is not one", 0, None, 400),
            ]:
                status, value = publish(guest_port, host_port)
                assert status == want, (label, status, value)
                results[label] = value["message"]
            status, value = request("POST", f"/machines/{NAME}/ports", {"guestPort": 80, "address": "0.0.0.0"})
            assert status == 422, (status, value)
            status, value = request("DELETE", f"/machines/{NAME}/ports/{first + 3}")
            assert status == 404, (status, value)
            status, value = request("POST", "/machines/absent-99/ports", {"guestPort": 80})
            assert status == 404, (status, value)
            assert [p["hostPort"] for p in listed()] == [first, first + 1, first + 2]
            return results
        case("what cannot be published is refused, and nothing is", refusals)

        def taken_back():
            status, value = request("DELETE", f"/machines/{NAME}/ports/{first + 2}")
            assert status == 204, (status, value)
            # The node stops listening as it answers, or a moment after.
            deadline = time.monotonic() + 3
            while True:
                try:
                    socket.create_connection(("127.0.0.1", first + 2), timeout=5).close()
                except ConnectionRefusedError:
                    break
                assert time.monotonic() < deadline, "a port taken back still accepts"
                time.sleep(0.05)
            assert [p["hostPort"] for p in listed()] == [first, first + 1]
            # The others are untouched, and the port can be given out again.
            assert fetch(first, "small.txt") == b"small\n"
            status, published = publish(8080)
            assert status == 201 and published["hostPort"] == first + 2, (status, published)
            assert fetch(first + 2, "small.txt") == b"small\n"
            status, value = request("DELETE", f"/machines/{NAME}/ports/{first + 2}")
            assert status == 204, (status, value)
            return None
        case("a port taken back refuses connections, and can be published again", taken_back)

        def across_a_stop():
            status, machine = request("POST", f"/machines/{NAME}/stop")
            assert status == 200 and machine["state"] == "stopped", (status, machine)
            assert [p["listening"] for p in listed()] == [True, True]
            assert closed_without_a_word(first) == b""
            status, machine = request("POST", f"/machines/{NAME}/start")
            assert status == 200 and machine["state"] == "running", (status, machine)
            serve()
            assert hashlib.sha256(fetch(first, "big.bin")).hexdigest() == state["big"]
            return None
        case("stopped, a connection is accepted and closed; started again, the port works as it was",
             across_a_stop)

        def across_a_node_restart():
            stop_node()
            # Nothing of the old node is listening.
            try:
                socket.create_connection(("127.0.0.1", first), timeout=5).close()
                raise AssertionError("a port outlived its node")
            except ConnectionRefusedError:
                pass
            start_node()
            assert [(p["hostPort"], p["guestPort"], p["listening"]) for p in listed()] == [
                (first, 8080, True), (first + 1, 9001, True)]
            # The machine comes back with the node; its server is the guest's to start.
            deadline = time.monotonic() + 120
            while request("GET", f"/machines/{NAME}")[1]["state"] != "running":
                assert time.monotonic() < deadline, "the machine did not come back with the node"
                time.sleep(0.5)
            serve()
            assert hashlib.sha256(fetch(first, "big.bin")).hexdigest() == state["big"]
            return {"node_starts": node["starts"]}
        case("after the node is restarted the port is still published and works", across_a_node_restart)

        def deleted():
            status, machine = request("POST", f"/machines/{NAME}/stop")
            assert status == 200, (status, machine)
            status, value = request("DELETE", f"/machines/{NAME}")
            assert status == 204, (status, value)
            for host_port in (first, first + 1):
                deadline = time.monotonic() + 5
                while True:
                    try:
                        # As a server takes a port: connections that have
                        # only just closed do not hold it, a listener does.
                        with socket.socket() as sock:
                            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                            sock.bind(("127.0.0.1", host_port))
                            sock.listen(1)
                        break
                    except OSError:
                        assert time.monotonic() < deadline, f"port {host_port} was not given back"
                        time.sleep(0.1)
            return None
        case("deleting the machine gives its ports back", deleted)

        report["success"] = True
    finally:
        stop_node()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
