# Connection headers and revoked scopes

The isolated accepted-source checkout passed all 187 hv2-net tests on Linux.
The tested secret_substitution.rs bytes match the workspace file. The checkout
retains accepted core sources; protected workspace core files were not built.

Request rewriting now excludes headers nominated by every Connection header,
with case-insensitive header names. Invalid or empty Connection tokens refuse
rewriting before commit. Tests cover multiple Connection fields, unchanged
nominated placeholders, substitution in an end-to-end header, and unchanged
headers after malformed-input refusal.

The sandbox-scope test also removes and re-adds the same ID: the new store can
substitute, while an old retained handle stays revoked. This verifies library
handle behavior, not daemon lifecycle concurrency, KVM guest traffic, managed
competitor equivalence, or performance. No benchmark binary was replaced.

Reproduce in the accepted-source checkout with the archived module overlaid:
`CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo test -p hv2-net`.
