# A team's Git credential, held on the host, on KVM (2026-10-22)

`tools/check-team-git-kvm.py` on WSL2's KVM, running as root: one clustered
`hv2-sandboxd` (a `memory:` store and a cluster token, so a request may name
its team) with `--team-git-credential` for teams `alpha` and `beta` (two
different tokens for the same prefix) and a node-wide `--git-credential` with
`--git-credential-in-sandbox`; busybox guests with the host's git and curl; an
owned HTTPS server recording each request's Authorization. All 4 cases pass
(`report.json`).

1. alpha's sandbox cloned `/team/` with alpha's token on every request, beta's
   with beta's, never the other's.
2. A sandbox with no team cloning `/team/` was refused (401) and sent no
   credential.
3. `/other/`, covered only by the node's credential, was cloned by alpha, beta
   and the team-less sandbox, each with the node's token.
4. No token, nor its base64, is in any file or process environment of any
   guest.

NOT VERIFIED: the request through a running control plane (the team header was
sent to the node directly, as the control plane would), a Redis store, a real
GitHub or GitLab, `git push`, and a server-side environment clone.
