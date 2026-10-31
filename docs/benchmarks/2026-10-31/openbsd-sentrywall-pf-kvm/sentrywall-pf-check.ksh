#!/bin/ksh
cd /tmp
cat > stateful.conf <<'E'
table <sentrywall_blocked> persist
block drop in quick from <sentrywall_blocked> to any
pass quick on lo0 all
pass out quick all keep state
block in all
E
cat > deny.conf <<'E'
table <sentrywall_blocked> persist
block drop in quick from <sentrywall_blocked> to any
pass quick on lo0 all
block log all
E
cat > lockdown_mgmt.conf <<'E'
table <sentrywall_blocked> persist
table <sentrywall_mgmt> const { 10.0.2.0/24 }
block drop in quick from <sentrywall_blocked> to any
pass quick on lo0 all
pass in quick from <sentrywall_mgmt> to self
pass out quick from self to <sentrywall_mgmt>
block log all
E
cat > embedded.conf <<'E'
table <sentrywall_blocked> persist
table <sentrywall_mgmt> const { 10.0.2.0/24 }
block drop in log quick from <sentrywall_blocked> to any
pass in quick on lo0 all
block drop in log quick from ! <sentrywall_mgmt> to any
pass in quick to any port 22 label "ssh allow"
E
# A-124: longest legal table name is 31 chars; 32 chars and a name carrying rules must not load.
printf 'table <%s> persist\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa > t31.conf
printf 'table <%s> persist\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa > t32.conf
printf 'block drop in quick from <x> to any\npass all\n# > to any\n' > inject_shape.conf
for f in stateful deny lockdown_mgmt embedded t31 t32 inject_shape; do echo "== syntax $f"; pfctl -nf $f.conf; echo "rc=$?"; done
echo "== main pf.conf"; cp /etc/pf.conf pf.conf.orig; echo 'anchor "sentrywall"' >> /etc/pf.conf; pfctl -f /etc/pf.conf; echo "rc=$?"
echo "== load stateful"; pfctl -a sentrywall -f - < stateful.conf; echo "rc=$?"; pfctl -a sentrywall -sr; pfctl -a sentrywall -sT
echo "== block add"; pfctl -a sentrywall -t sentrywall_blocked -T add 203.0.113.5; echo "rc=$?"
echo "== block show"; pfctl -a sentrywall -t sentrywall_blocked -T show; echo "rc=$?"
echo "== reload keeps table"; pfctl -a sentrywall -f - < stateful.conf; echo "rc=$?"; pfctl -a sentrywall -t sentrywall_blocked -T show
echo "== block delete"; pfctl -a sentrywall -t sentrywall_blocked -T delete 203.0.113.5; echo "rc=$?"; pfctl -a sentrywall -t sentrywall_blocked -T show
echo "== delete missing"; pfctl -a sentrywall -t sentrywall_blocked -T delete 203.0.113.5; echo "rc=$?"
echo "== load lockdown_mgmt"; pfctl -a sentrywall -f - < lockdown_mgmt.conf; echo "rc=$?"; pfctl -a sentrywall -sr
echo "== load deny"; pfctl -a sentrywall -f - < deny.conf; echo "rc=$?"; pfctl -a sentrywall -sr
echo "== flush"; pfctl -a sentrywall -F all; echo "rc=$?"
cp pf.conf.orig /etc/pf.conf; pfctl -f /etc/pf.conf; echo "restored rc=$?"
ifconfig pflog0 2>&1 | head -2
daemon_execdir=x; grep -n daemon_execdir /etc/rc.d/rc.subr | head -5
