#!/bin/ksh
cd /tmp
echo "== main pf.conf anchor lines before"; grep -n anchor /etc/pf.conf; echo "grep rc=$?"
cat > embedded_stateful.conf <<'E'
table <sentrywall_blocked> persist
block drop in log quick from <sentrywall_blocked> to any
pass in quick proto tcp from any to any port 22 keep state label "ssh allow"
block return in quick from 198.51.100.7 to any label "reject one"
E
cat > embedded_lockdown.conf <<'E'
table <sentrywall_blocked> persist
table <sentrywall_mgmt> const { 10.0.2.0/24 }
block drop in log quick from <sentrywall_blocked> to any
pass in quick on lo0 all
block drop in log quick from ! <sentrywall_mgmt> to any
block drop in quick proto tcp from 203.0.113.9 to any port 23 label "drop telnet"
E
for f in embedded_stateful embedded_lockdown; do
echo "== syntax $f"; pfctl -nf $f.conf; echo "rc=$?"
echo "== load $f"; pfctl -a sentrywall -f $f.conf; echo "rc=$?"
pfctl -a sentrywall -sr
pfctl -a sentrywall -T show -t sentrywall_blocked; echo "show rc=$?"
done
echo "== flush"; pfctl -a sentrywall -F all; echo "rc=$?"
