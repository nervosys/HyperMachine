#!/bin/bash
# $1 = output dir name, $2 = image, $3 = seconds; optional $4 = extra machine JSON
rm -rf "/var/tmp/$1"
export KEYS='[]'
export SETTLE=15
export EXPECT='[["(S)hell?","S\n"],["# ","uname -a; sysctl hw.ncpu hw.physmem; dmesg | grep -i \"^vio\\|^sd0\\|^pf\\|^vmm\"; pfctl -s info 2>&1 | head -4; ifconfig -a | grep -v \"^\\s\" | head; echo DONE-MARK-$((6*7))\n"],["DONE-MARK-42",""]]'
python3 /mnt/c/Users/adamm/tmp/blk/obsd_probe.py "$2" "/var/tmp/$1" "$3" ${4:+"$4"} 2>&1 | grep -v "^create\|^final\|^console bytes" | head -12
python3 - "$1" <<'EOF'
import sys, re
t = open(f"/var/tmp/{sys.argv[1]}/console.txt", errors="replace").read().replace("\x00", "")
t = re.sub(r"(\x08 \x08.)+", "", t)
i = t.rfind("OpenBSD 8.0")
print("---- tail of console ----")
print(t[-3500:])
EOF
