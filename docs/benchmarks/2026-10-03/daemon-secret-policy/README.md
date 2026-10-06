# Owned daemon secret policy startup and reload

Seven checks passed using a separate debug daemon built in an isolated checkout.
Networking is required; unsafe file permissions, symlink files and malformed
policy JSON refuse startup. A valid private policy starts an empty daemon.
Malformed SIGHUP reload is refused while the daemon remains responsive; a valid
reload completes. All owned daemon processes were reaped and no guests were
created. Input executable/kernel/initrd hashes remained unchanged.

The full isolated source hash catalog and selected tested sources are recorded.
Protected worktree boot files were excluded; their accepted isolated copies were
used. The accepted benchmark executable was neither rebuilt nor replaced.

This verifies binary startup and signal handling only. Empty inventory and log
markers do not prove active secret contents, guest substitution, fork/pause/resume
behavior or a performance win. Library tests supply separate substitution and
retained-store validation; KVM lifecycle verification remains outstanding.
