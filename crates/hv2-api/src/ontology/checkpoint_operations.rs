//! A sandbox's fork, checkpoints and snapshots, as the control plane
//! (`hv2-cluster`, `control.rs`: `forward`, `list_snapshots`) and the node
//! (`hv2-sandboxd`: `fork_sandbox`, `checkpoints.rs`, `snapshot_sandbox`,
//! `list_snapshots`) serve them. Modelled as data: `hv2-api` depends on
//! neither crate.
//!
//! The control plane forwards each `/sandboxes/{id}/...` route to the node that
//! holds the sandbox. It buffers the node's answer and keeps its status,
//! `content-type`, and the two headers a node sets on an archive
//! (`content-disposition`, `x-hv2-archive-skipped`), and no other.

use super::spec::{body_rejection, fail, ok, path_param, plain, query_param, required_query, Spec};
use super::{HyperMachineOntology, Operation, Parameter};
use serde_json::{json, Value};

fn checkpoint_param() -> Parameter {
    path_param(
        "name",
        "The checkpoint's name, as listed",
        json!({ "type": "string" }),
    )
}

fn checkpoint_schema() -> Value {
    json!({ "type": "object", "properties": {
        "name": { "type": "string" },
        "createdAt": { "type": "integer", "description": "unix milliseconds" }
    } })
}

impl HyperMachineOntology {
    /// The control plane's fork, checkpoint and snapshot routes.
    pub(super) fn build_checkpoint_control_plane_operations() -> Vec<Operation> {
        Self::checkpoint_operations(false)
    }

    /// The node's fork, checkpoint and snapshot routes.
    pub(super) fn build_checkpoint_node_operations() -> Vec<Operation> {
        Self::checkpoint_operations(true)
    }

    fn checkpoint_operations(node: bool) -> Vec<Operation> {
        let sandbox = || Self::sandbox_id_parameter(node);
        let access = |named: bool| Self::sandbox_access_responses(node, named);
        let no_checkpoint = || fail(404, "the sandbox has no checkpoint of that name");
        let path_query = || {
            required_query(
                "path",
                "An absolute path in the checkpoint's filesystem: no NUL, at most 4096 bytes",
                json!({ "type": "string" }),
            )
        };
        let reader_failures = || {
            vec![
                fail(
                    503,
                    "`checkpoint readers unavailable`: two throwaway guests are already \
                     restoring checkpoints on this node (or a node answered 503 and no other could)",
                ),
                fail(
                    500,
                    "the checkpoint could not be restored into a throwaway guest, or its \
                     filesystem could not be read",
                ),
                fail(
                    502,
                    "the guest that was restored from the checkpoint did not answer",
                ),
            ]
        };

        let specs = vec![
            Spec::new("fork_sandbox", "Fork A Sandbox", "POST", "/sandboxes/{SB}/fork")
                .about(
                    "Make `count` new sandboxes from a running one: it is checkpointed (its \
                     memory and its processes included) and each fork resumes from that, as \
                     a sandbox of its own with a new ID and access token and a reseeded \
                     random-number generator. A fork has the source's egress rules, volumes, \
                     environment variables, owner and team, and its metadata minus `hm.name`. \
                     The source keeps running. Because memory is copied, so is everything in \
                     it: keys, tokens, environment and open files; the fork can use them. \
                     A fork with another `cpuCount` or `memoryMB` is booted from a copy of the \
                     disk and does not carry the memory. A sandbox with a disk is forked at most \
                     8 at a time. The body may be left out. The answer is 201 even when every \
                     fork failed: each element is a `sandbox` or an `error`. A sandbox of the \
                     caller's own is needed; on the control plane each fork's descriptor names \
                     its node and the control plane's proxy port. Not idempotent.",
                )
                .with(sandbox())
                .takes(
                    json!({
                        "type": "object",
                        "properties": {
                            "timeout": { "type": "integer", "default": 15, "description": "seconds each fork lives, at most 24 hours" },
                            "count": { "type": "integer", "minimum": 1, "maximum": 100, "default": 1, "description": "at most 8 when the sandbox holds a disk" },
                            "cpuCount": { "type": "integer" },
                            "memoryMB": { "type": "integer" }
                        }
                    }),
                    false,
                )
                .answers(ok(
                    201,
                    "One element per fork asked for, in order.",
                    Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                        "sandbox": Self::sandbox_descriptor_schema(node),
                        "error": { "type": "object", "properties": {
                            "code": { "type": "integer" }, "message": { "type": "string" } } } } } })),
                ))
                .answers(fail(400, "`count` is not 1 to 100 (8 with a disk), or the size is not one a sandbox can have"))
                .answers(body_rejection())
                .answers(fail(404, "no such sandbox on the node"))
                .answers(fail(
                    409,
                    "the node has no templates, the sandbox is paused, or its registration \
                     with the cluster is still being reconciled",
                ))
                .answers(fail(500, "the source could not be checkpointed"))
                .answers(fail(503, "the owner of the sandbox could not be recorded"))
                .answers_all(access(true)),
            Spec::new(
                "create_sandbox_checkpoint",
                "Checkpoint A Sandbox",
                "POST",
                "/sandboxes/{SB}/checkpoints",
            )
            .about(
                "Keep a named copy of a running sandbox, memory and processes included, to \
                 restore it to later, read files from, or fork. The sandbox is paused for the \
                 moment it takes and keeps running. A sandbox with a disk is checkpointed with \
                 a copy of the disk. At most 10 are held (the one the node keeps for itself, \
                 `auto`, is not counted), so one must be deleted to make room. A checkpoint \
                 holds everything in the sandbox's memory, secrets and environment included, \
                 and is read by whoever may read the sandbox. The body may be left out, \
                 giving `cp-` and the time. Not idempotent: a name that is held is 409.",
            )
            .with(sandbox())
            .takes(
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "pattern": "^[A-Za-z0-9._-]+$", "description": "`auto` is reserved" } }
                }),
                false,
            )
            .answers(ok(201, "The checkpoint.", Some(checkpoint_schema())))
            .answers(fail(400, "a name with other characters than letters, digits and `- _ .`, or `auto`"))
            .answers(body_rejection())
            .answers(fail(404, "no such sandbox"))
            .answers(fail(
                409,
                "the sandbox is paused; a checkpoint of that name is held; 10 are held; or \
                 the node has no template to build one on",
            ))
            .answers(fail(500, "the checkpoint could not be made"))
            .answers_all(access(true)),
            Spec::new(
                "list_sandbox_checkpoints",
                "List A Sandbox's Checkpoints",
                "GET",
                "/sandboxes/{SB}/checkpoints",
            )
            .about(
                "The checkpoints held for a sandbox, with when each was made. Only a running \
                 or paused sandbox has any: one that has ended answers 404, and its checkpoints \
                 are gone with it.",
            )
            .with(sandbox())
            .answers(ok(
                200,
                "The checkpoints.",
                Some(json!({ "type": "array", "items": checkpoint_schema() })),
            ))
            .answers(fail(404, "no such sandbox, or it is not running or paused"))
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "delete_sandbox_checkpoint",
                "Delete A Sandbox's Checkpoint",
                "DELETE",
                "/sandboxes/{SB}/checkpoints/{name}",
            )
            .about(
                "Delete a checkpoint of a sandbox and free what it holds on the node: its \
                 memory image and, for a sandbox with a disk, the copy of the disk. The \
                 sandbox is not touched. The second call is 404. Checkpoints go when the \
                 sandbox ends.",
            )
            .with(sandbox())
            .with(checkpoint_param())
            .answers(ok(204, "Deleted.", None))
            .answers(no_checkpoint())
            .answers_all(access(true)),
            Spec::new(
                "restore_sandbox_checkpoint",
                "Restore A Sandbox To A Checkpoint",
                "POST",
                "/sandboxes/{SB}/checkpoints/{name}/restore",
            )
            .about(
                "Put a sandbox back to a checkpoint, in place: the same ID, access token and \
                 URL, but its memory, processes and (with a disk) files as they were then, at \
                 the checkpoint's size. Every connected client is dropped, and what the \
                 sandbox did since is lost. A restore that fails leaves the sandbox as it was. \
                 Not idempotent in its effect on a sandbox that has moved on.",
            )
            .with(sandbox())
            .with(checkpoint_param())
            .answers(ok(
                200,
                "Restored.",
                Some(json!({ "type": "object", "properties": {
                    "sandboxID": { "type": "string" }, "checkpoint": { "type": "string" } } })),
            ))
            .answers(no_checkpoint())
            .answers(fail(
                409,
                "the sandbox is paused, or its registration with the cluster is still being reconciled",
            ))
            .answers(fail(500, "the restore failed; the sandbox is untouched"))
            .answers_all(access(true)),
            Spec::new(
                "list_sandbox_checkpoint_files",
                "List A Directory In A Sandbox's Checkpoint",
                "GET",
                "/sandboxes/{SB}/checkpoints/{name}/files",
            )
            .about(
                "List a directory of a checkpoint's filesystem without touching the sandbox: \
                 the checkpoint is restored into a throwaway guest with its processes stopped, \
                 read through its agent and discarded. At most two such guests exist on a \
                 node at once, so a request can be refused (503). Up to 10,000 entries.",
            )
            .with(sandbox())
            .with(checkpoint_param())
            .with(path_query())
            .answers(ok(
                200,
                "The entries.",
                Some(json!({ "type": "object", "properties": {
                    "path": { "type": "string" }, "checkpoint": { "type": "string" },
                    "entries": { "type": "array", "items": { "type": "object" } } } })),
            ))
            .answers(fail(400, "no `path`, one that is not absolute, has a NUL or is too long, or is not a directory"))
            .answers(fail(404, "no such path in the checkpoint, or no such checkpoint"))
            .answers_all(reader_failures())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "read_sandbox_checkpoint_file",
                "Read A File From A Sandbox's Checkpoint",
                "GET",
                "/sandboxes/{SB}/checkpoints/{name}/file",
            )
            .about(
                "One file out of a checkpoint's filesystem, up to 64 MiB, read as in the \
                 listing route from a throwaway guest. It gives what the file held when the \
                 checkpoint was made, secrets included.",
            )
            .with(sandbox())
            .with(checkpoint_param())
            .with(path_query())
            .answers(ok(200, "The file's bytes.", Some(json!({ "type": "string", "format": "binary" }))))
            .answers(fail(400, "no `path`, one that is not absolute or is too long, or a directory"))
            .answers(fail(404, "no such path or checkpoint"))
            .answers(fail(413, "the file is over 64 MiB"))
            .answers_all(reader_failures())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "archive_sandbox_checkpoint_directory",
                "Download A Directory Of A Sandbox's Checkpoint As A Tar",
                "GET",
                "/sandboxes/{SB}/checkpoints/{name}/archive",
            )
            .about(
                "A directory of a checkpoint and what is below it as a gzipped tar, built in \
                 a throwaway guest. Limits: 64 MiB of archive, 256 MiB read and 200,000 \
                 entries, and a build over 180 seconds is stopped (504). The node names what \
                 it left out in an `x-hv2-archive-skipped` header, and a `content-disposition`; \
                 the control plane passes both on with the content type, so a caller there \
                 can tell a complete archive from one with entries skipped.",
            )
            .with(sandbox())
            .with(checkpoint_param())
            .with(path_query())
            .answers(ok(
                200,
                "The archive, `application/gzip`.",
                Some(json!({ "type": "string", "format": "binary" })),
            ))
            .answers(fail(400, "no `path`, one that is not absolute or is too long, or is not a directory"))
            .answers(fail(404, "no such path or checkpoint"))
            .answers(fail(413, "over 64 MiB of archive, 256 MiB read or 200,000 entries"))
            .answers(fail(504, "the build took over 180 seconds"))
            .answers_all(reader_failures())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "snapshot_sandbox",
                "Snapshot A Sandbox As A Template",
                "POST",
                "/sandboxes/{SB}/snapshots",
            )
            .about(
                "Turn a running sandbox into a template that new sandboxes start from: its \
                 memory and processes are kept, so each one starts exactly where this one is. \
                 The sandbox keeps running. The snapshot belongs to the sandbox's team and \
                 can be shared with others (the template share routes); one that is shared \
                 holds this sandbox's memory, and with it every secret in it. The same name \
                 replaces the earlier snapshot of that name. Refused for a sandbox that holds \
                 a disk, that is paused, or that was not itself started from a template. \
                 The body may be left out, giving `snap-` and 12 hex digits. Not idempotent.",
            )
            .with(sandbox())
            .takes(
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "pattern": "^[A-Za-z0-9._-]+(:[A-Za-z0-9._-]+)?$", "description": "a `:tag` is dropped" } }
                }),
                false,
            )
            .answers(ok(
                201,
                "The snapshot.",
                Some(json!({ "type": "object", "properties": {
                    "snapshotID": { "type": "string", "description": "`name:default`" },
                    "names": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers(fail(400, "a name with other characters than letters, digits and `- _ .`"))
            .answers(body_rejection())
            .answers(fail(404, "no such sandbox"))
            .answers(fail(
                409,
                "the name is a template built from an image; the sandbox holds a disk or is \
                 paused or was not restored from a template; or another team's snapshot has the name",
            ))
            .answers(fail(500, "the snapshot could not be made"))
            .answers_all(access(true)),
            Spec::new("list_snapshots", "List Snapshots", "GET", "/snapshots")
                .about(if node {
                    "List every snapshot on this node, with its names, the sandbox and template \
                     it was made from, when, the team that owns it and the teams it is shared \
                     with. A node answers for the whole node: the control plane narrows the \
                     answer to the caller's team."
                } else {
                    "List the snapshots of every live node that the caller's team owns or has \
                     been shared, merged by `snapshotID` with the `nodeIDs` that hold each. \
                     Filtered to those the caller may use. An observer or viewer is refused, as \
                     it is for templates, and the key needs the `templates` scope."
                })
                .with(query_param(
                    "sandboxID",
                    "Only snapshots made from this sandbox",
                    json!({ "type": "string" }),
                ))
                .with(query_param(
                    "name",
                    "Only the snapshot with this name",
                    json!({ "type": "string" }),
                ))
                .answers(ok(
                    200,
                    "The snapshots.",
                    Some(json!({ "type": "array", "items": { "type": "object", "properties": {
                        "snapshotID": { "type": "string" },
                        "names": { "type": "array", "items": { "type": "string" } },
                        "sandboxID": { "type": "string" },
                        "templateID": { "type": "string" },
                        "createdAt": { "type": "integer", "description": "unix milliseconds" },
                        "teamID": { "type": ["string", "null"] },
                        "sharedWith": { "type": "array", "items": { "type": "string" } },
                        "nodeIDs": { "type": "array", "items": { "type": "string" }, "description": "control plane only" } } } })),
                ))
                .answers(plain(400, "a query parameter that is not text: the framework's own rejection"))
                .answers_all(access(false))
                .repeatable(),
        ];
        specs.into_iter().map(|spec| spec.build(node)).collect()
    }
}
