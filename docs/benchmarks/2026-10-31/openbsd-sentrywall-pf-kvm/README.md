# SentryWall's pf check on OpenBSD 8.0 (KVM)

Requested by SentryWall's session (`sentrywall-b2`). The script in
[`sentrywall-pf-check.ksh`](sentrywall-pf-check.ksh) is their text, unchanged. It was typed into the guest's
root shell over the serial console, one line at a time, then run with `ksh`, on a throwaway copy of the
[installed OpenBSD disk](../openbsd-install-pf-kvm/README.md). The original image was not written (its
modification time is from the install). The copy was deleted afterwards. WSL2, nested KVM, 1 run.

[`result.txt`](result.txt) is the script's output; [`console.txt`](console.txt) is the whole console.

## Result (1 run)

- Syntax (`pfctl -nf`): stateful, deny, lockdown_mgmt, a 31-character table name and `inject_shape`: rc=0.
  A 32-character table name: rc=1, `table name too long, max 31 chars`.
- **`embedded.conf`: rc=1**, `embedded.conf:6: port only applies to tcp/udp` (the rule
  `pass in quick to any port 22 label "ssh allow"` has no `proto tcp`).
- **`inject_shape.conf`: rc=0.** Its third line starts with `#`, so pf reads it as a comment and the file
  loads; it did not fail as the request's comment expected.
- Loading into the `sentrywall` anchor, adding, showing, reloading (the table kept `203.0.113.5`),
  deleting and flushing: rc=0 each. Deleting an address that is not there prints `0/1 addresses deleted.`
  and returns rc=0.
- `lockdown_mgmt` expands `to self` into rules for `127.0.0.1`, `10.0.2.15` and `::1`.
- The `block log all` ruleset loaded and the serial console kept working.
- `pflog0` is `UP,RUNNING`. `rc.subr` honours `daemon_execdir` (lines 117, 190, 383).

## Not verified

- One run. SentryWall's own binaries (`PfController`, `PfFilter`) did not run: only the text of the rules
  and the `pfctl` arguments were used.
- Whether traffic is blocked or passed: no packets were sent through the loaded rules.
- `sshd` is off in this guest, so the `port 22` rule has no listener.
