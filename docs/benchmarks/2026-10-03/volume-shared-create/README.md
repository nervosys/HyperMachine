# Shared-directory volume creation across independent daemons

Six owned API checks pass with two independent daemon processes sharing the same
local --volume-dir. Sixteen synchronized creates are distributed across both:
one returns 201 and fifteen return 409. Both APIs observe the same winning token,
and duplicates from each preserve metadata/data. The winning daemon is reaped
and restarted; its original token and marker remain. Deleting the owned volume
is visible to both daemons. Every owned process is reaped and private files are
removed. Tokens, metadata and raw API responses are not archived.

The updated checker's single-daemon mode also passes, including restart. Input
hashes in both reports exactly match volume-exclusive-create, whose archive
contains the tested runtime source, 43 passing daemon tests and build context.
No runtime or accepted benchmark binary changes were needed for this extension.

These are standalone direct daemon APIs sharing one local filesystem. There is
no Redis/control-plane cluster, network filesystem, guest mount or power-loss
test. This verifies cross-process exclusion locally and process restart
persistence, not NFS/managed storage durability, coordinated delete/create,
block-device equivalence, competitor parity or a performance win.

Reproduce: `python3 tools/check-volume-creation.py --nodes 2 --daemon BIN
--kernel KERNEL --initrd INITRD --output NEW.json`. Use --nodes 1 for the retained
single-daemon mode. Each report path must not exist.
