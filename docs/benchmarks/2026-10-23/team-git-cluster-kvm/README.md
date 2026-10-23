# A team's Git credential through a real control plane, on KVM (2026-10-23)

`tools/check-team-git-cluster-kvm.py` on WSL2's KVM, running as root: an owned
Redis, one `hv2-sandboxd` node and the `hv2-control-plane` over verified TLS
and node mTLS, with an API key policy of a red key and a blue key (and the
administrator), the node holding `--team-git-credential red=...` and
`blue=...` for the same prefix, and an owned HTTPS Git server recording each
request's Authorization. The team is the control plane's, from the API key;
the script sends no team header. All 3 cases pass (`report.json`).

1. A sandbox the red key created cloned `/team/` with red's token on all 6
   requests, and blue's with blue's, never the other's.
2. A sandbox the administrator created (no team) sent no credential and was
   refused.
3. No token, nor its base64, is in any file or process environment of any
   guest.

This closes the gap the [single-node check](../2026-10-22/team-git-kvm/README.md)
left (the team header sent to a node directly, a `memory:` store).

NOT VERIFIED: a real GitHub or GitLab, `git push`, and a server-side
environment clone (which does not use a team's credential).
