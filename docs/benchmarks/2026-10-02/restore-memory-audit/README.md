# Accepted restore memory audit

This audit binds four source files to the accepted 550-file catalog used by
the measured daemon. It changes no runtime code. The full accepted copies are
under `accepted-source/`; `source-context.json` records their hashes and base
commit. Original provisional core files remain untouched and unstaged.

The image restore path in `crates/hv2-core/src/vm.rs` selects
`snapshot.header.memory_image`, calls `load_memory_image`, and does not enter
the inline or layered page-write loops. `load_memory_image` returns immediately
when `map_guest_memory_from` succeeds; its full-image copy is a fallback for
other backends. Accepted KVM maps the file with `MAP_PRIVATE | MAP_FIXED`,
preserving the registered host address. The C100 smaps evidence independently
observes private file mappings of the expected size.

Snapshot launch calls `provision_inner(false)`. That skips loading the configured
kernel/initrd and cold-boot memory writes. The daemon's `new_vm` configures boot
file paths and preserves the guest's device model, including its console.
The console uses empty `VecDeque` buffers and grows on output; its 1 MiB cap
does not mean every quiet restored guest allocates a 1 MiB output buffer.
Working-set prefaulting is conditional on nonempty ranges and is disabled by
default in the accepted daemon. These source facts rule out unconditional
image copying, repeated cold boot loading, and eager console-cap allocation
as explanations for the measured prepared-restoration memory gap.

They do not prove that every host write is necessary or identify the source
of the approximately 13 MiB private-dirty difference at C100. Snapshot states
are independently prepared under different device models; guest scheduling,
clock/RNG maintenance and command activity can also dirty pages. Removing
restore writes without establishing their ownership could corrupt resumed
state. The next diagnostic should measure the same owned child's mapping at
three boundaries: immediately after host restore before its first vCPU run,
after the restored-notice acknowledgment, and after the verification command.
That would distinguish pre-run host changes from later guest/device activity.
No ownership or causal performance claim follows from this static audit.
