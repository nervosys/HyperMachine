#!/bin/bash
# $1 = output dir name, $2 = image (an installed disk), $3 = seconds
rm -rf "/var/tmp/$1"
export KEYS='[]'
export SETTLE=15
export HMT=/var/tmp/hm-trace-target/release
export EXPECT='[["login:","root\n"],["Password:","swtest-pw1\n"],["# ","uname -a; sysctl kern.version | head -1; pfctl -s info; pfctl -sr; ifconfig vio0; echo DONE-MARK-$((6*7))\n"],["DONE-MARK-42",""]]'
python3 ./obsd_probe.py "$2" "/var/tmp/$1" "$3" '{"network":{"allowInternetAccess":true}}' 2>&1 | grep -v "^create\|^final\|^console bytes" | head -8
python3 - "$1" <<'EOF'
import sys, re
t = open(f"/var/tmp/{sys.argv[1]}/console.txt", errors="replace").read().replace("\x00", "")
t = re.sub(r"(\x08 \x08.)+", "", t)
t = t.replace("\r\n", "\n").replace("\r", "\n")
t = re.sub(r"\n{2,}", "\n", t)
i = t.find("login:")
print("---- from the login prompt ----")
print(t[max(0, i - 900):i + 3500])
EOF
