#!/bin/bash
# $1 = output dir name, $2 = image, $3 = seconds. Boots to the installer, takes the shell, runs checks.
rm -rf "/var/tmp/$1"
export KEYS='[]'
export SETTLE=10
export EXPECT='[["(S)hell?","S\n"],["# ","sysctl kern.version hw.ncpu hw.physmem hw.model\n"],["hw.model","dmesg | grep -i vio\n"],["vioblk0 at virtio0","pfctl -s info\n"],["Status:","pfctl -sr\n"],["# ","ls /sbin /usr/sbin | tr \"\\n\" \" \"\n"],["pfctl","echo DONE-MARK-$((6*7))\n"],["DONE-MARK-42",""]]'
python3 /mnt/c/Users/adamm/tmp/blk/obsd_probe.py "$2" "/var/tmp/$1" "$3" 2>&1 | grep -v "^create\|^final\|^console bytes" | head -12
python3 - "$1" <<'EOF'
import sys, re
t = open(f"/var/tmp/{sys.argv[1]}/console.txt", errors="replace").read().replace("\x00", "")
t = re.sub(r"(\x08 \x08.)+", "", t)
i = t.find("(I)nstall")
print("---- from the installer prompt ----")
print(t[i - 200:i + 5000])
EOF
