# SentryWall's autoinstall answers, typed into the OpenBSD 8.0 installer (KVM)

**This was an interactive install using the answers, not an unattended autoinstall.** `obsd_autoinstall.py`
waits for each prompt on the serial console and types one answer ([`steps.log`](steps.log)); there was no
response file and no autoinstall mode. So the root password was asked twice, and `Public ssh key for root
account?` (asked only under autoinstall) never appeared. 1 run; WSL2, nested KVM.

## Differences from the requested file

OpenBSD 8.0 not 7.8; sets over HTTP from `mirror.leaseweb.com` with `-comp* -man* -game* -x*` (smaller than
`-game* -x*`); NIC `vio0` not `em0`; test key generated here (never used, deleted). The installer did not ask
`DNS domain name`, `Do you expect to run the X Window System` or `Start ntpd(8) by default`, so those answers
went unused; `/etc/myname` is `sentrywall.my.domain`. No `SHA256.sig` prompt (HTTP source).

## Observed

- The install completed (422 s to the final prompt). `Allow root ssh login` = `prohibit-password` was accepted
  and left **no `PermitRootLogin` line** in `/etc/ssh/sshd_config`.
- `/root/.ssh/authorized_keys` is empty. `/etc/boot.conf` is `stty com0 115200` / `set tty com0`.
  `/etc/ttys` line 6: `console "/usr/libexec/getty std.9600" vt220 off secure`
  ([`installer-shell-checks.txt`](installer-shell-checks.txt)).
- Normal boot, `root` with a wrong password at the console: `Login incorrect`
  ([`boot1-console-login-refused.txt`](boot1-console-login-refused.txt)).
- `boot -s` at the boot prompt gave a root shell with no password prompt (the console is `secure`)
  ([`boot2-single-user-attempt.txt`](boot2-single-user-attempt.txt)).

## Not verified

- The root password field in `master.passwd` (the installer shell has no `awk`; the single-user read was
  stopped by the host for low memory and not rerun).
- `sshd -T` on a booted system (no host keys existed in the installer shell); any ssh login (the guest has no
  inbound path).
