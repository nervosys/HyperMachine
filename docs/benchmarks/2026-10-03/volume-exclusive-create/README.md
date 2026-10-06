# Exclusive persistent-volume creation

Creation previously checked for an existing name, then create_dir_all reused the
deterministic volume directory and rewrote its metadata. Concurrent creates
could both pass the check and publish different access tokens for the same volume.
The daemon now reserves that directory with create_dir before writing metadata.
An existing complete or incomplete directory conflicts without mutation. Metadata
is published by rename only within the newly reserved directory. Failed ordinary
publication removes only the unpublished temporary file and empty reserved
directories; cleanup is not recursive. A crash may leave an incomplete reservation
that remains fail-closed for operator review; automatic crash repair is absent.

All 43 daemon tests pass. Sixteen simultaneous helper callers produce one winner;
losers return AlreadyExists. The winner's token and marker bytes survive another
duplicate. A pre-existing incomplete directory and marker are also preserved.
The owned HTTP checker sends sixteen synchronized POST /volumes requests and
observes one 201 and fifteen 409 statuses. Published token matches the winning
response, a subsequent duplicate preserves metadata/data, listing has one entry,
and the owned volume and daemon are cleaned up. No guests are created.

The archived module bytes match the isolated checkout and workspace. The full
isolated source catalog identifies the separate verification daemon. Protected
workspace core sources were neither read nor built. Private tokens, metadata and
raw API responses are not archived. The accepted benchmark daemon is unchanged.

This validates one local filesystem and daemon, not multi-node NFS semantics,
power-loss durability, block-device attach equivalence, managed competitor parity
or a performance win. Cross-process/fleet creation and crash recovery remain to
be exercised. No external publication occurred.

Reproduce in the isolated accepted-core checkout: cargo test -p hv2-sandboxd;
cargo build -p hv2-sandboxd; then `python3 tools/check-volume-creation.py --daemon
BIN --kernel KERNEL --initrd INITRD --output NEW.json`. The report records exact
input hashes. The output must not exist.
