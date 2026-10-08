#!/usr/bin/env python3
"""Firmware boot on real KVM: a stock cloud image, from firmware to login.

Runs hv2-core's `pvh_firmware_probe` example, which enters a firmware image by
the PVH boot protocol with a raw disk image attached over PCI, and checks the
guest's serial console:

1. With no disk, the firmware is entered and says it was PVH-booted, then
   reports it has nothing to boot from.
2. With the image, the firmware finds the disk on the PCI bus, finds the EFI
   system partition and loads the bootloader on it.
3. The bootloader starts the image's own kernel and the guest reaches a login
   prompt on the serial console. The image is used as downloaded: no kernel,
   command line or file in it is changed. The run is on a copy, so the guest
   can write to its disk.

Writes report.json and each run's console into --output.
"""
import argparse, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path


def digest(path):
    hasher = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            hasher.update(block)
    return hasher.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["probe", "firmware", "image", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--login", default="login:", help="text that shows the guest reached its prompt")
    parser.add_argument("--seconds", type=int, default=180)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of PVH firmware boot on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["probe", "firmware", "image"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-firmware-check-", dir="/var/tmp"))

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def run(label, disk, until, seconds):
        env = dict(os.environ, RUST_LOG="warn", HV2_SETTLE_SECS=str(seconds), HV2_UNTIL=until)
        if disk:
            env["HV2_DISK"] = str(disk)
        done = subprocess.run([str(args.probe), str(args.firmware)], env=env, stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=seconds + 60)
        raw = done.stdout.decode(errors="replace").replace("\x00", "")
        text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", raw).replace("\r", "\n")
        lines = [line.rstrip() for line in text.split("\n") if line.strip()]
        (args.output / f"{label}.console.txt").write_text("\n".join(lines) + "\n")
        return "\n".join(lines)

    try:
        def entered():
            console = run("no-disk", None, "Unable to boot", 20)
            assert "Booting with PVH Boot Protocol" in console, console[-600:]
            assert "Unable to boot from any virtio-blk device" in console, console[-600:]
            return {"said": "Booting with PVH Boot Protocol"}
        case("the firmware is entered by PVH and has nothing to boot without a disk", entered)

        disk = work / "disk.raw"
        shutil.copyfile(args.image, disk)
        state = {}

        def finds_the_disk():
            state["console"] = run("image", disk, args.login, args.seconds)
            console = state["console"]
            found = re.search(r"Found PCI device vendor=1af4 device=1042 in slot=(\d+)", console)
            assert found, console[-800:]
            capacity = re.search(r"Virtio block device configured. Capacity: (\d+) sectors", console)
            assert capacity and int(capacity[1]) * 512 == disk.stat().st_size, (capacity, disk.stat().st_size)
            assert "Found EFI partition" in console, console[-800:]
            loader = re.search(r"Found bootloader: (\S+)", console)
            assert loader and "Executable loaded" in console, console[-800:]
            return {"pci_slot": int(found[1]), "sectors": int(capacity[1]), "bootloader": loader[1]}
        case("the firmware finds the disk on the PCI bus and loads the bootloader from its EFI partition",
             finds_the_disk)

        def reaches_login():
            console = state["console"]
            after = console.split("Executable loaded", 1)[1]
            assert args.login in after, after[-800:]
            assert "PANIC" not in console and "Kernel panic" not in console, console[-800:]
            return {"last_lines": after.strip().split("\n")[-2:]}
        case("the image's own bootloader and kernel bring the guest to a login prompt on serial", reaches_login)
        report["success"] = True
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
