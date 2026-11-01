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

## Single-user read of the installed disk (second attempt, after the host stopped the first)

`boot -s` on a fresh copy, `mount -uw /; mount -a`, then the checks ([`boot3-single-user-checks.txt`](boot3-single-user-checks.txt)):

- root's field in `/etc/master.passwd` is `*************` (13 asterisks).
- `/etc/ttys`: `console ... off secure` and `tty00 "/usr/libexec/getty std.115200" vt220 on secure`; `/etc/boot.conf`
  is `stty com0 115200` / `set tty com0`.
- `/etc/ssh/sshd_config` has only the commented defaults `#PermitRootLogin prohibit-password` and
  `#PasswordAuthentication yes`; `rcctl ls on` lists `sshd`.
- `sshd -T`: `sshd: no hostkeys available -- exiting.` (single-user, before first boot); `authorized_keys` is empty.

## Not verified

- The effective `sshd -T` values and any ssh login: no host keys existed in single-user mode, and the guest has
  no inbound network path.
