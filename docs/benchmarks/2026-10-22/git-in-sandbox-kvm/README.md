# A sandbox's own git, with a credential only the host holds (2026-10-22)

`tools/check-git-in-sandbox-kvm.py` on WSL2's KVM (24 host CPUs): an owned
`hv2-sandboxd --network --git-credential https://git.example.test:PORT/team/=VAR
--git-credential-in-sandbox`, an owned HTTPS server (a bare repository over
git's dumb protocol; `/team/` and `/other/` need `Basic bot:<token>`, `/open/`
does not, and every request's `Authorization` is recorded), and the busybox
guest plus the host's `git` 2.47.3, `git-remote-https` and `curl` with their
libraries. All 6 cases pass (`report.json` has the digests).

1. `git clone https://.../team/r.git` in the guest works; all 6 requests to
   `/team/` carried exactly the node's credential.
2. The token (and its base64) is in no file under `/etc /root /tmp /home`,
   no process environment, and not the clone's `.git/config`; a control file
   holding it is found by the same search.
3. A `curl -H 'Authorization: Bearer guest'` under the prefix reaches the
   server with the node's credential and not the guest's.
4. `/open/` clones with no `Authorization` on any request; `/other/` is 401
   with none sent.
5. `/team/../other/...` and `/team/%2e%2e/other/...` (sent as written) are
   401, with no credential sent.
6. A node without the flag: the clone fails (`could not read Username ...
   terminal prompts disabled`) and the server saw no credential.

NOT VERIFIED: a real GitHub or GitLab, `git push`, git's smart protocol,
a Debian guest (this one is busybox plus copied binaries), more than one
credential, and the credential per team.
