//! Volumes, the content API that reads and writes them, and block disks, as
//! the control plane (`hv2-cluster`, `control.rs`: `list_volumes`,
//! `to_volume_node`) and the node (`hv2-sandboxd`: `volumes.rs`, `disks.rs`,
//! routed in `main.rs`) serve them. Modelled as data: `hv2-api` depends on
//! neither crate.
//!
//! The control plane answers `GET /volumes` itself (it asks every live node
//! and merges) and relays everything else about a volume, management and
//! content alike, to one node chosen by rendezvous hashing over the volume's
//! ID. Disks are the node's alone: the control plane has no `/disks` route.

use super::network_operations::control_plane_gate;
use super::spec::{fail, ok, path_param, plain, query_param, required_query, Spec};
use super::{
    Capability, CapabilityCategory, Guardrail, GuardrailSeverity, HyperMachineOntology, Operation,
    OperationResponse, Parameter,
};
use serde_json::{json, Value};

/// The path parameter naming a volume: `id` on the control plane,
/// `volumeID` on the node.
fn volume_param(node: bool, what: &str) -> Parameter {
    path_param(
        if node { "volumeID" } else { "id" },
        what,
        json!({ "type": "string" }),
    )
}

/// What a volume's management routes answer about the volume: its ID, name and
/// content token.
fn volume_with_token_schema() -> Value {
    json!({ "type": "object", "properties": {
        "volumeID": { "type": "string", "description": "`vol-` and 16 hex digits, derived from the name (and the team's label, for a team's volume)" },
        "name": { "type": "string" },
        "token": { "type": "string", "description": "the volume's content bearer token, for `/volumecontent`" } } })
}

/// What `get_volume` answers: the volume, its `token` always null. The key is
/// kept because E2B's `VolumeAndToken` requires it.
fn volume_info_schema() -> Value {
    json!({ "type": "object", "properties": {
        "volumeID": { "type": "string" },
        "name": { "type": "string" },
        "token": { "type": "null", "description": "always null: the token is shown by `create_volume` and `rotate_volume_token` only" } } })
}

/// A listed volume: the token is not in a listing.
fn volume_listed_schema() -> Value {
    json!({ "type": "object", "properties": {
        "volumeID": { "type": "string" },
        "name": { "type": "string" },
        "teamID": { "type": ["string", "null"], "description": "the owning team; null for a volume made outside a team" } } })
}

/// What a content route answers for a path: its stat.
fn stat_schema() -> Value {
    json!({ "type": "object", "properties": {
        "name": { "type": "string", "description": "the last path component; empty for the volume's root" },
        "type": { "type": "string", "enum": ["file", "directory", "symlink", "unknown"] },
        "path": { "type": "string", "description": "absolute within the volume, normalised" },
        "size": { "type": "integer" },
        "mode": { "type": "integer", "description": "permission bits with setuid, setgid and sticky (`st_mode & 0o7777`)" },
        "uid": { "type": "integer" },
        "gid": { "type": "integer" },
        "atime": { "type": "string", "description": "RFC 3339, to the second" },
        "mtime": { "type": "string" },
        "ctime": { "type": "string" },
        "target": { "type": "string", "description": "a symlink's target, as written; the link is never followed" } } })
}

fn disk_schema() -> Value {
    json!({ "type": "object", "properties": {
        "diskID": { "type": "string", "description": "`disk-` and 16 hex digits, derived from the name" },
        "name": { "type": "string" },
        "sizeMiB": { "type": "integer" },
        "attachedTo": { "type": ["string", "null"], "description": "the sandbox holding it; null when free, and also when the claim is a dead daemon's" } } })
}

/// The answers any node route can give for volumes and that the control
/// plane passes through unchanged.
fn node_volume_failures() -> Vec<OperationResponse> {
    vec![
        fail(
            401,
            "the node was started with a cluster token (or mTLS) and the request does not carry it in `x-hv2-cluster-token` (`this node answers its cluster's control plane only`); a node with no token answers everyone",
        ),
        fail(
            400,
            "`invalid team context` or `duplicate team context`: the `x-hv2-team` header the control plane adds is malformed or sent twice",
        ),
        fail(
            503,
            "`team attribution requires an authenticated cluster node`: a team header reached a node that is not clustered or has no cluster token",
        ),
        fail(
            501,
            "`volumes need a Linux host: they are served with openat2, which only Linux has`: a node on Windows or macOS",
        ),
    ]
}

/// What the control plane itself can answer on a relayed volume route.
fn relay_failures() -> Vec<OperationResponse> {
    vec![
        fail(
            400,
            "`invalid request path`: the target holds a `.` or `..` segment (in any percent-encoding), a backslash or a control character, and is not repeated to a node",
        ),
        fail(502, "the node chosen for the volume did not answer"),
        fail(
            503,
            "the cluster store is unavailable, or no node is alive (`no node is alive`)",
        ),
    ]
}

/// What a content route answers whatever it was asked: the checks in
/// `authorized`, and the framework's refusal of the query.
fn content_failures() -> Vec<OperationResponse> {
    vec![
        fail(
            404,
            "`no volume <id>`: no such volume, the ID is not 1 to 64 letters, digits, `_` and `-`, or the request carries no bearer token or not this volume's token. These are one answer with one body, so a caller cannot tell which volume IDs exist. The token is compared as a digest in constant time.",
        ),
        plain(
            400,
            "the query has no `path`, or a parameter is not the type it is read as (the framework's own rejection, plain text); also a `path` that is not the volume's own: a component over 255 bytes or holding a NUL (`EINVAL`), or a `..` that would climb out of the root (`EACCES`)",
        ),
        fail(
            500,
            "the volume's data directory could not be opened, or an I/O error the code does not name",
        ),
        fail(
            501,
            "`volumes need a Linux host: they are served with openat2, which only Linux has`: a node on Windows or macOS",
        ),
    ]
}

/// The path-resolution errors every path-taking content route can add.
fn path_failures() -> Vec<OperationResponse> {
    vec![
        fail(404, "the path, or a parent of it, does not exist"),
        fail(
            400,
            "a component of the path is a file where a directory is needed (`ENOTDIR`), the path is a directory where a file is needed (`EISDIR`), or resolving it would follow a symlink (`ELOOP`) or leave the volume (`EXDEV`)",
        ),
    ]
}

impl HyperMachineOntology {
    /// The operations on volumes and their contents, for the control plane
    /// (`node` false) or the node, which also serves disks.
    pub(super) fn volume_operations(node: bool) -> Vec<Operation> {
        let mut specs = Self::volume_management_specs(node);
        specs.extend(if node {
            Self::volume_content_node_specs()
        } else {
            Self::volume_content_relay_specs()
        });
        if node {
            specs.extend(Self::disk_specs());
        }
        specs.into_iter().map(|spec| spec.build(node)).collect()
    }

    fn volume_management_specs(node: bool) -> Vec<Spec> {
        let gate = || -> Vec<OperationResponse> {
            if node {
                node_volume_failures()
            } else {
                let mut responses = control_plane_gate("volumes", false);
                responses.extend(relay_failures());
                responses.extend(node_volume_failures().into_iter().skip(1));
                responses
            }
        };
        let id = |what: &str| volume_param(node, what);
        let where_made = if node {
            "on this node"
        } else {
            "on the node the control plane picks for it"
        };

        let list = Spec::new(
            "list_volumes",
            "List Volumes",
            "GET",
            "/volumes",
        )
        .about(if node {
            "List the volumes of this node's volume directory, sorted by name: each volume's \
             `volumeID`, `name` and `teamID` (null for a volume made outside a team). The \
             content token is not in the listing, nor in `get_volume`; only `create_volume` \
             and `rotate_volume_token` return it. When the control \
             plane relayed the call with a team, only that team's volumes are listed; a call \
             with no team context sees every volume, including those of every team. A \
             node on a shared volume directory lists what every node holds; a node whose \
             directory is its own lists its own."
        } else {
            "List the volumes of the cluster. The control plane does not relay this call to one \
             node: it asks every live node for `GET /volumes`, keeps each volume once (the first \
             answer for an ID wins), and returns them sorted by volume ID. A key or session \
             with a team sees that team's volumes only; one without a team (an administrator, \
             a legacy key, anonymous access) sees every volume of every team. The content \
             token is not in the listing. A node that does not answer, or answers anything \
             but 200 (a node on Windows or macOS answers 501), is left out without a word, so \
             the list can be short without saying so. When no node answers 200 (no live node, \
             or none that serves volumes) the answer is 503, not an empty list."
        })
        .answers(ok(
            200,
            "The volumes, as an array.",
            Some(json!({ "type": "array", "items": volume_listed_schema() })),
        ))
        .answers(fail(
            503,
            "Control plane only: no live node answered the list with 200.",
        ))
        .answers_all(gate())
        .repeatable();

        let create = Spec::new("create_volume", "Create A Volume", "POST", "/volumes")
            .about(if node {
                "Make a persistent volume: a directory of this node's volume directory (the \
                 snapshot store's `volumes/` when there is one and no `--volume-dir`, else \
                 node-local transient storage) that sandboxes mount by name through \
                 `volumeMounts` and the content API reads and writes. The body is `{\"name\"}`, \
                 1 to 64 letters, digits, `_` and `-`. The ID is derived from the name \
                 (and from the team's label when the control plane sent a team), so a name \
                 is one volume: a second create of it is 409 and never replaces the first's \
                 token or data, even when two creators race (the directory is reserved \
                 with an exclusive create and the metadata, mode 0600, is published by a \
                 rename after its sync). The 201 answer holds the volume's content bearer \
                 token. A failure to flush after publication can answer 500 while the volume \
                 exists and is readable; retrying the name is then 409. Names are unique \
                 within a team: two teams may each have `data`."
            } else {
                "Make a persistent volume on the node the control plane picks for it. The \
                 choice is rendezvous hashing over the volume's ID, which the body's `name` \
                 (and the caller's team) determines, so a create and every later call about \
                 the volume meet on the same node; with nodes sharing one volume directory \
                 (`--volume-dir`, or a shared snapshot store) every node holds every volume \
                 and the choice only spreads load. The control plane reads at most 64 KiB of \
                 body (more is 400), takes the name from it and relays it with the caller's \
                 team, taken from authentication and never from a header the caller sent. \
                 The body is `{\"name\"}`, 1 to 64 letters, digits, `_` and `-`; names are \
                 unique within a team. The 201 answer holds the volume's content bearer \
                 token, the only credential the content routes accept. A name that exists is \
                 409 and never replaces the first's token or data. A body that is not JSON \
                 hashes as an empty name and is refused by the node."
            })
            .takes(
                json!({ "type": "object", "required": ["name"], "properties": {
                    "name": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,64}$" } } }),
                true,
            )
            .answers(ok(
                201,
                &format!("Made {where_made}: its `volumeID`, `name` and content `token`."),
                Some(volume_with_token_schema()),
            ))
            .answers(fail(
                400,
                "the name is not 1 to 64 letters, digits, `_` and `-`",
            ))
            .answers(fail(
                409,
                "`volume <name> exists`: the name is taken in this team (or outside teams)",
            ))
            .answers(fail(
                500,
                "the volume directory could not be made or its metadata written or flushed (the message names the path)",
            ))
            .answers(super::spec::body_rejection())
            .answers_all(gate());

        let get = Spec::new(
            "get_volume",
            "Get A Volume And Its Token",
            "GET",
            if node {
                "/volumes/{volumeID}"
            } else {
                "/volumes/{id}"
            },
        )
        .about(
            "Read one volume: its `volumeID` and `name`. The `token` key is present and always \
             null: the content token is returned once, by `create_volume` and by \
             `rotate_volume_token`, and is stored only as a digest, so a caller who can read \
             a volume's metadata cannot read its credential. A volume of another team is not \
             forbidden but missing: 404, the same as one that does not exist. A call with no \
             team context reaches every volume. An ID that is not 1 to 64 letters, digits, \
             `_` and `-` is 404 too. The control plane relays the call to the node picked for \
             the ID.",
        )
        .with(id(
            "The volume's ID (`vol-` and 16 hex digits), from `create_volume` or `list_volumes`",
        ))
        .answers(ok(
            200,
            "The volume, with `token` null.",
            Some(volume_info_schema()),
        ))
        .answers(fail(
            404,
            "`no volume <id>`: it does not exist, or belongs to another team",
        ))
        .answers_all(gate())
        .repeatable();

        let delete = Spec::new(
            "delete_volume",
            "Delete A Volume",
            "DELETE",
            if node {
                "/volumes/{volumeID}"
            } else {
                "/volumes/{id}"
            },
        )
        .about(
            "Remove a volume and every file in it, at once (`remove_dir_all`), and forget its \
             quota counters. Sandboxes still mounting it are not stopped and are not refused: \
             they see an empty directory from then on. Nothing is kept, and the content token \
             stops working with it. A volume of another team is 404, as for `get_volume`. The \
             volume is first renamed to a hidden name, so it is gone from every listing and \
             its name is free at once; a failure part way through the removal (500) leaves \
             some files in that hidden directory, for an operator to clear. On the control plane the \
             call is relayed to the node picked for the ID, so a node that does not hold the \
             volume's directory (nodes without a shared volume directory) answers 404 and \
             removes nothing.",
        )
        .with(id("The volume's ID"))
        .answers(plain(204, "The volume and its files are gone."))
        .answers(fail(
            404,
            "`no volume <id>`: it does not exist, or belongs to another team",
        ))
        .answers(fail(
            500,
            "the directory could not be removed (the message is the operating system's)",
        ))
        .answers_all(gate());

        let rotate = Spec::new(
            "rotate_volume_token",
            "Rotate A Volume's Token",
            "POST",
            if node {
                "/volumes/{volumeID}/token"
            } else {
                "/volumes/{id}/token"
            },
        )
        .about(if node {
            "Replace a volume's content bearer token with a new one, returned once in the \
             answer, with the volume's `volumeID` and `name`. The old token stops working at \
             once: the new digest is published (temporary file, sync, rename) before the \
             answer is sent, so a caller holding the old token is refused from then on. The \
             volume, its files and its team are unchanged. Authorised as `get_volume` and \
             `delete_volume` are: the node's gate and the team context, never the content \
             token, which cannot rotate itself. A volume of another team, or none, is 404 \
             with the same body. Two rotations racing leave the one published last in force; \
             the other answer's token is then refused. A failure to publish answers 500 and \
             leaves the old token in force."
        } else {
            "Replace a volume's content bearer token with a new one, returned once in the \
             answer, from the node the control plane picks for the volume. The old token stops \
             working once the node publishes the new one, before its answer is sent. The volume \
             and its team are unchanged. Authorised as `get_volume` and `delete_volume` are: an \
             API key with the `volumes` scope and the caller's team. The content bearer token \
             is not accepted here and is not forwarded to the node. A volume of another team, \
             or none, is 404 with the same body. Racing rotations leave the last one published \
             in force."
        })
        .with(id(
            "The volume's ID, from `create_volume` or `list_volumes`",
        ))
        .answers(ok(
            200,
            "The volume and its new token, once.",
            Some(volume_with_token_schema()),
        ))
        .answers(fail(
            404,
            "`no volume <id>`: it does not exist, or belongs to another team",
        ))
        .answers(fail(
            500,
            "the new metadata could not be written or synced; the old token stays in force",
        ))
        .answers_all(gate())
        .repeatable();

        vec![list, create, get, delete, rotate]
    }

    /// The control plane's three content routes: every method is relayed.
    fn volume_content_relay_specs() -> Vec<Spec> {
        let relay = |id: &'static str, name: &'static str, path: &'static str, served: &str| {
            Spec::new(id, name, "ANY", path)
                .about(format!(
                    "Relay any request on `{path}` to the node picked for the volume (rendezvous \
                     hashing over the ID in the path, the same node as the management routes). \
                     The control plane asks for no API key, no session and no scope here: the \
                     route is registered outside the key check, and the only credential is the \
                     volume's own bearer token, which the node checks. Its `Authorization` \
                     header is repeated to the node for these paths, unless it is a session \
                     token (`hms1.`), which is never repeated. The caller's team is not \
                     carried: a team does not scope content. The cluster's own token \
                     replaces the node's gate, the query and the request and response bodies \
                     are streamed (an upload or a download may be large), and the node's \
                     status, `content-type` and a few of its headers come back. The node \
                     serves {served}; any other method is the node's own 405 with an empty \
                     body. The control plane answers itself only the path check, the missing \
                     node and the unreachable node. The node's per-method routes (`node_read_volume_file` \
                     and its neighbours) describe them in full."
                ))
                .with(volume_param(false, "The volume's ID, from `create_volume`"))
                .answers(plain(
                    200,
                    "Whatever the node answered, with its status and body (the node's per-method operations describe them).",
                ))
                .answers(fail(
                    404,
                    "`no volume <id>`, from the node: no such volume, a wrong or missing bearer token (the same answer as a missing volume), the node picked does not hold it (nodes without a shared volume directory), or the path inside it does not exist",
                ))
                .answers(plain(
                    405,
                    "a method the node does not serve on this path, with an empty body",
                ))
                .answers_all(relay_failures())
        };
        vec![
            relay(
                "volume_content_file",
                "Read Or Write A Volume File (Relayed)",
                "/volumecontent/{id}/file",
                "GET (read a file) and PUT (write one)",
            ),
            relay(
                "volume_content_dir",
                "List Or Make A Volume Directory (Relayed)",
                "/volumecontent/{id}/dir",
                "GET (list a directory) and POST (make one)",
            ),
            relay(
                "volume_content_path",
                "Stat, Change Or Remove A Volume Path (Relayed)",
                "/volumecontent/{id}/path",
                "GET (stat), PATCH (change owner or mode) and DELETE (remove)",
            ),
        ]
    }

    /// The node's content routes, one operation per method.
    fn volume_content_node_specs() -> Vec<Spec> {
        let volume = || volume_param(true, "The volume's ID");
        let path_query = |what: &str| required_query("path", what, json!({ "type": "string" }));
        let auth = "The only credential is the volume's bearer token (`Authorization: Bearer <token>`, from `create_volume` or `get_volume`); the node's cluster token is not asked for on these routes, and neither is an API key. ";

        let read = Spec::new(
            "read_volume_file",
            "Read A Volume File",
            "GET",
            "/volumecontent/{volumeID}/file",
        )
        .about(format!(
            "Stream the bytes of a file of a volume, as `application/octet-stream`, in 256 KiB \
             chunks. {auth}The path is resolved beneath the volume's root with no symlink \
             followed: `..` is resolved in the text and may not climb out. The answer has no length or checksum header; an error \
             part way through a large file closes the connection after a 200."
        ))
        .with(volume())
        .with(path_query("The file's path in the volume, such as `/workspace/input.bin`"))
        .answers(ok(
            200,
            "The file's bytes, streamed.",
            Some(json!({ "type": "string", "format": "binary" })),
        ))
        .answers_all(path_failures())
        .answers_all(content_failures())
        .repeatable();

        let write = Spec::new(
            "write_volume_file",
            "Write A Volume File",
            "PUT",
            "/volumecontent/{volumeID}/file",
        )
        .about(format!(
            "Write a file of a volume from the request body, streamed, up to 4 GiB. {auth}By \
             default the upload is made in place: the file is created or truncated at once, \
             so an interrupted or failed upload leaves a partial file and two writers need \
             the caller's coordination. With `atomic=true` the bytes go to a private \
             `.hm-upload-*` file beside the destination, which is synced and renamed over it, \
             so the previous file stays whole until the new one is complete (a hard kill can \
             leave the staging file; a directory-sync error after the rename can answer \
             failure with the new file visible). `overwrite=false` refuses an existing file \
             (409): `O_EXCL` in place, `RENAME_NOREPLACE` when atomic. `force=true` makes \
             missing parent directories. `mode` (default 0644), `uid` and `gid` are applied \
             after the write; a setuid or setgid bit in `mode` is dropped, sticky is kept; \
             sticky is kept. The file, the containing directories and the volume \
             root are synced before the 201. Every chunk is charged against the volume's \
             budget before it is written (`write` spends the same allowance as a guest's \
             writes): the node-wide host-disk reserve (on by default) and the optional \
             per-volume byte and entry quotas answer 507. See the guardrails."
        ))
        .with(volume())
        .with(path_query("The file's path; not the volume's root"))
        .with(query_param(
            "force",
            "Make missing parent directories (mode 0755). Default false",
            json!({ "type": "boolean" }),
        ))
        .with(query_param(
            "atomic",
            "Stage the upload and rename it into place, keeping the previous file until then. Default false",
            json!({ "type": "boolean" }),
        ))
        .with(query_param(
            "overwrite",
            "Replace an existing file. Default true; false answers 409 for one",
            json!({ "type": "boolean" }),
        ))
        .with(query_param(
            "mode",
            "Permission bits, as an integer (so `420` is 0644). Default 0644",
            json!({ "type": "integer" }),
        ))
        .with(query_param("uid", "Owner, as a number", json!({ "type": "integer" })))
        .with(query_param("gid", "Group, as a number", json!({ "type": "integer" })))
        .takes_raw(
            "application/octet-stream",
            json!({ "type": "string", "format": "binary" }),
            true,
        )
        .answers(ok(
            201,
            "Written: the file's stat.",
            Some(stat_schema()),
        ))
        .answers(fail(
            400,
            "no `path` or the root as the path (`a file path`); the body stream broke; or the destination is a directory (`EISDIR`) or, with `atomic`, not a regular file (`EINVAL`)",
        ))
        .answers(fail(
            404,
            "a parent directory does not exist and `force` is not set",
        ))
        .answers(fail(
            409,
            "`overwrite=false` and the file exists",
        ))
        .answers(fail(
            413,
            "the body is over 4 GiB (`larger than a volume file may be`); an in-place upload leaves what was written",
        ))
        .answers(fail(
            507,
            "the host-disk reserve or a per-volume quota refused the growth (`ENOSPC`, `EDQUOT`)",
        ))
        .answers_all(content_failures());

        let list = Spec::new(
            "list_volume_dir",
            "List A Volume Directory",
            "GET",
            "/volumecontent/{volumeID}/dir",
        )
        .about(format!(
            "List a directory of a volume, with each entry's stat. {auth}`depth` (default 1, \
             held to 1 through 32) goes into subdirectories that many levels down; the answer is \
             one flat array, entries of a directory in name order but directories visited \
             last-found first, so the order is not a tree walk. An entry that cannot be \
             statted is left out without a word. Symlinks are listed and never followed. \
             The path may be `/` or empty for the root."
        ))
        .with(volume())
        .with(path_query("The directory's path; `/` for the root"))
        .with(query_param(
            "depth",
            "How many levels to list, 1 to 32 (outside it is clamped). Default 1",
            json!({ "type": "integer", "minimum": 1, "maximum": 32 }),
        ))
        .answers(ok(
            200,
            "The entries, as an array of stats.",
            Some(json!({ "type": "array", "items": stat_schema() })),
        ))
        .answers_all(path_failures())
        .answers_all(content_failures())
        .repeatable();

        let mkdir = Spec::new(
            "make_volume_dir",
            "Make A Volume Directory",
            "POST",
            "/volumecontent/{volumeID}/dir",
        )
        .about(format!(
            "Make a directory in a volume. {auth}`force=true` makes missing parents (mode 0755) \
             and accepts a directory that exists already; without it an existing path is 409 \
             and a missing parent is 404. An existing regular file at the path is refused \
             even with `force`. `mode` (default 0755, setuid and setgid dropped), `uid` \
             and `gid` are applied after. The new entries and every directory above them are \
             synced before the 201. A made directory spends one entry of the volume's \
             quota. The body is empty."
        ))
        .with(volume())
        .with(path_query("The directory's path; not the volume's root"))
        .with(query_param(
            "force",
            "Make missing parents and accept an existing directory. Default false",
            json!({ "type": "boolean" }),
        ))
        .with(query_param(
            "mode",
            "Permission bits as an integer. Default 0755",
            json!({ "type": "integer" }),
        ))
        .with(query_param("uid", "Owner, as a number", json!({ "type": "integer" })))
        .with(query_param("gid", "Group, as a number", json!({ "type": "integer" })))
        .answers(ok(201, "Made: the directory's stat.", Some(stat_schema())))
        .answers(fail(
            400,
            "no `path` or the root as the path (`a directory path`), or the existing path under `force` is not a directory",
        ))
        .answers(fail(
            404,
            "a parent directory does not exist and `force` is not set",
        ))
        .answers(fail(409, "the path exists and `force` is not set"))
        .answers(fail(
            507,
            "the host-disk reserve or the entry quota refused the new directory",
        ))
        .answers_all(content_failures());

        let stat = Spec::new(
            "stat_volume_path",
            "Stat A Volume Path",
            "GET",
            "/volumecontent/{volumeID}/path",
        )
        .about(format!(
            "Describe one path of a volume: its type (`file`, `directory`, `symlink` or \
             `unknown`), size, permission bits, owner, and access, modify and change times as \
             RFC 3339. {auth}A symlink is described, with its `target`, and never followed. The \
             path may be `/` for the root, whose name is empty."
        ))
        .with(volume())
        .with(path_query("The path to describe"))
        .answers(ok(200, "The path's stat.", Some(stat_schema())))
        .answers_all(path_failures())
        .answers_all(content_failures())
        .repeatable();

        let update = Spec::new(
            "update_volume_path",
            "Change A Volume Path's Owner Or Mode",
            "PATCH",
            "/volumecontent/{volumeID}/path",
        )
        .about(format!(
            "Change the permission bits or owner of a path of a volume. {auth}The body is \
             `{{\"mode\", \"uid\", \"gid\"}}`, each optional; whatever is left out stays. \
             A setuid or setgid bit in `mode` is dropped. A symlink is left alone (the answer \
             still describes it). A request with no `Content-Type` is read as an empty \
             update and changes nothing; one that has it needs a valid JSON body. A `chown` \
             the node's user may not make fails as 500."
        ))
        .with(volume())
        .with(path_query("The path to change"))
        .takes(
            json!({ "type": "object", "properties": {
                "mode": { "type": "integer" },
                "uid": { "type": "integer" },
                "gid": { "type": "integer" } } }),
            false,
        )
        .answers(ok(
            200,
            "The path's stat after the change.",
            Some(stat_schema()),
        ))
        .answers(super::spec::body_rejection())
        .answers_all(path_failures())
        .answers_all(content_failures())
        .repeatable();

        let remove = Spec::new(
            "remove_volume_path",
            "Remove A Volume Path",
            "DELETE",
            "/volumecontent/{volumeID}/path",
        )
        .about(format!(
            "Remove a file, a symlink or a directory of a volume. {auth}A directory goes with \
             everything under it, entry by entry: it is not atomic, and a failure part way \
             leaves what was not yet removed and what was removed gone. The volume's root \
             cannot be removed (400 `not the volume's root`); delete the volume instead. \
             Removed entries give their space and their entries back to the volume's quota."
        ))
        .with(volume())
        .with(path_query("The path to remove"))
        .answers(plain(204, "Removed."))
        .answers(fail(
            400,
            "the path is the volume's root (`not the volume's root`)",
        ))
        .answers_all(path_failures())
        .answers_all(content_failures());

        vec![read, write, list, mkdir, stat, update, remove]
    }

    /// Block disks, which only a node serves.
    fn disk_specs() -> Vec<Spec> {
        let id = || {
            path_param(
                "diskID",
                "The disk's ID (`disk-` and 16 hex digits), from `create_disk` or `list_disks`",
                json!({ "type": "string" }),
            )
        };
        let gate = || {
            vec![fail(
                401,
                "the node was started with a cluster token (or mTLS) and the request does not carry it in `x-hv2-cluster-token`; a node with no token answers everyone",
            )]
        };
        let list = Spec::new("list_disks", "List Block Disks", "GET", "/disks")
            .about(
                "List the disks of this node's disk directory (`--disk-dir`, else \
                 `hv2-sandboxd-disks` under the node's temporary directory), sorted by name: \
                 `diskID`, `name`, `sizeMiB` and `attachedTo`, the sandbox holding the disk. \
                 A disk is held by one sandbox at a time. `attachedTo` is null when the disk is \
                 free, and also when its metadata names a sandbox this daemon process no longer \
                 holds (a restarted daemon treats a dead daemon's claim as free). Disks are \
                 not scoped to a team: every caller that passes the node's gate sees them \
                 all. The control plane has no `/disks` route; call the node.",
            )
            .answers(ok(
                200,
                "The disks, as an array.",
                Some(json!({ "type": "array", "items": disk_schema() })),
            ))
            .answers_all(gate())
            .repeatable();
        let create = Spec::new("create_disk", "Create A Block Disk", "POST", "/disks")
            .about(
                "Make a persistent ext4 block device a sandbox can hold, sparse on the host and \
                 formatted with the host's `mkfs.ext4` (e2fsprogs must be installed) before the \
                 answer. The body is `{\"name\", \"sizeMiB\"}`: a name of 1 to 64 letters, digits, \
                 `_` and `-`, and 16 to 1,048,576 MiB (1 TiB). The ID is derived from the name, \
                 so a name is one disk. The formatting runs while the node holds its disk lock, \
                 so other disk calls on the node wait for it. A failed create removes what it \
                 made. A sandbox then asks for the disk with `diskMount` on create, by name. \
                 The disk is a raw image: no copy-on-write, no snapshots of its own.",
            )
            .takes(
                json!({ "type": "object", "required": ["name", "sizeMiB"], "properties": {
                    "name": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,64}$" },
                    "sizeMiB": { "type": "integer", "minimum": 16, "maximum": 1048576 } } }),
                true,
            )
            .answers(ok(201, "Made and formatted.", Some(disk_schema())))
            .answers(fail(
                400,
                "the name is not 1 to 64 letters, digits, `_` and `-`, or `sizeMiB` is outside 16 to 1,048,576",
            ))
            .answers(fail(409, "`disk <name> exists`"))
            .answers(fail(
                500,
                "the image could not be made, or `mkfs.ext4` is missing or failed (the message says which), or its metadata could not be recorded",
            ))
            .answers(super::spec::body_rejection())
            .answers_all(gate());
        let get = Spec::new("get_disk", "Get A Block Disk", "GET", "/disks/{diskID}")
            .about(
                "Read one disk: `diskID`, `name`, `sizeMiB` and `attachedTo` (the holding sandbox, \
             or null when free or when the claim is a dead daemon's). An ID that is not 1 to 64 \
             letters, digits, `_` and `-` is 404.",
            )
            .with(id())
            .answers(ok(200, "The disk.", Some(disk_schema())))
            .answers(fail(404, "`no disk <id>`"))
            .answers_all(gate())
            .repeatable();
        let delete = Spec::new(
            "delete_disk",
            "Delete A Block Disk",
            "DELETE",
            "/disks/{diskID}",
        )
        .about(
            "Remove a disk and its image. Refused while a sandbox holds it: unlike a volume's, \
             whose sandboxes are left looking at an empty directory, a disk's sandbox would be \
             left writing to a file that no longer exists. A claim left by a daemon that is gone \
             does not count. The disk's ID is not reusable for another size: make the name \
             again for a new, empty disk.",
        )
        .with(id())
        .answers(plain(204, "The disk and its image are gone."))
        .answers(fail(404, "`no disk <id>`"))
        .answers(fail(
            409,
            "`disk <name> is attached to sandbox <id>`: a sandbox of this daemon holds it",
        ))
        .answers(fail(500, "the directory could not be removed"))
        .answers_all(gate());
        vec![list, create, get, delete]
    }

    /// The capabilities over those operations, for the control plane or the
    /// node.
    pub(super) fn build_volume_capabilities(node: bool) -> Vec<Capability> {
        let prefix = if node { "node_" } else { "" };
        let ids = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| format!("{prefix}{name}")).collect()
        };
        let (who, auth, perms, running): (&str, &str, Vec<String>, &str) = if node {
            (
                "the node daemon",
                "The management routes check only the cluster token when the node has one and \
                 otherwise answer every caller that can reach them",
                vec!["cluster token".to_string()],
                "a Linux node (`hv2-sandboxd`)",
            )
        } else {
            (
                "the cluster control plane",
                "The management routes need a key or session with the `volumes` scope, and a \
                 team's key sees and acts on its own team's volumes only",
                vec!["volumes".to_string()],
                "a control plane with at least one live node on Linux",
            )
        };
        let suffix = if node {
            "node daemon"
        } else {
            "cluster control plane"
        };
        let mut capabilities = vec![
            Capability {
                id: format!("{prefix}volume_management"),
                name: format!("Persistent Volumes ({suffix})"),
                description: format!(
                    "Make, list, read and delete persistent shared volumes, through {who}. A \
                     volume is a host directory that sandboxes mount over 9P by name, that \
                     outlives them and that is the content API's subject. {auth}. Creating a \
                     volume and rotating its token return the content bearer token, once; \
                     reading a volume never does. See docs/VOLUMES.md"
                ),
                category: CapabilityCategory::Storage,
                operations: ids(&[
                    "list_volumes",
                    "create_volume",
                    "get_volume",
                    "delete_volume",
                    "rotate_volume_token",
                ]),
                prerequisites: vec![running.to_string()],
                permissions_required: perms,
            },
            Capability {
                id: format!("{prefix}volume_content"),
                name: format!("Volume Files ({suffix})"),
                description: if node {
                    "Read, write, list, make, stat, change and remove the files of a volume over \
                     HTTP, with the volume's own bearer token as the only credential. Writes can \
                     be atomic and create-only; every write spends the same allowance as a \
                     guest's (host-disk reserve, optional quotas). See docs/VOLUMES.md and \
                     docs/SANDBOXES.md"
                        .to_string()
                } else {
                    "Relay the volume content API to the node picked for the volume. The control \
                     plane asks for no key or session on these routes: the volume's bearer \
                     token, which the node checks, is the only credential. See docs/VOLUMES.md"
                        .to_string()
                },
                category: CapabilityCategory::Storage,
                operations: if node {
                    ids(&[
                        "read_volume_file",
                        "write_volume_file",
                        "list_volume_dir",
                        "make_volume_dir",
                        "stat_volume_path",
                        "update_volume_path",
                        "remove_volume_path",
                    ])
                } else {
                    ids(&[
                        "volume_content_file",
                        "volume_content_dir",
                        "volume_content_path",
                    ])
                },
                prerequisites: vec![
                    running.to_string(),
                    "a volume and its content token".to_string(),
                ],
                permissions_required: vec!["the volume's bearer token".to_string()],
            },
        ];
        if node {
            capabilities.push(Capability {
                id: "node_block_disks".to_string(),
                name: "Block Disks (node daemon)".to_string(),
                description: "Make, list, read and delete persistent ext4 block disks that one \
                    sandbox holds at a time, at block-device speed. Disks are the node's own: \
                    the control plane has no route for them, they are not scoped to a team and \
                    a disk never moves between nodes. See docs/DISKS.md"
                    .to_string(),
                category: CapabilityCategory::Storage,
                operations: ids(&["list_disks", "create_disk", "get_disk", "delete_disk"]),
                prerequisites: vec![
                    "a node (`hv2-sandboxd`) with `mkfs.ext4` (e2fsprogs) installed".to_string(),
                ],
                permissions_required: vec!["cluster token".to_string()],
            });
        }
        capabilities
    }

    /// The guardrails of volumes, their content and disks.
    pub(super) fn volume_guardrails() -> Vec<Guardrail> {
        let strings =
            |items: &[&str]| -> Vec<String> { items.iter().map(|item| item.to_string()).collect() };
        vec![
            Guardrail {
                id: "volume-growth-limits-and-defaults".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "A volume is bounded by a host-disk reserve, not by default by a quota"
                    .to_string(),
                description: "Anything that grows a volume, a guest's write over 9P or a \
                    content-API upload, is checked before it is done. Two limits exist. The \
                    host-disk guard is on by default: growth is refused (ENOSPC, `507` over \
                    HTTP) when it would leave the volume's filesystem with less than a reserve \
                    free, 5% of the filesystem capped at 2 GiB, and 1% of its inodes. \
                    `--volume-reserve-percent N` changes it and 0 turns it off. The per-volume \
                    quotas `--volume-max-bytes` and `--volume-max-inodes` are off by default \
                    (0); when set they are refused with EDQUOT (also `507`). The flags are \
                    node-wide, apply to every volume alike, are counted over apparent sizes (a \
                    sparse file counts what it claims) and are shared by every mount of a \
                    volume and the content API. With the defaults one volume may fill the \
                    filesystem down to the reserve, starving the node's other volumes, \
                    templates and snapshots if they share it. Nothing limits a volume \
                    on a node that runs with the reserve at 0 and no quota."
                    .to_string(),
                applies_to: strings(&[
                    "volume_content",
                    "node_volume_content",
                    "node_write_volume_file",
                    "node_make_volume_dir",
                    "volume_content_file",
                    "volume_content_dir",
                    "create_volume",
                    "node_create_volume",
                ]),
                condition: "Guests or content-API callers that must not be able to fill the \
                    host's disk write to a volume."
                    .to_string(),
                mitigation: "Keep the reserve on, set `--volume-max-bytes` and \
                    `--volume-max-inodes` where volumes are shared between tenants, and put \
                    volumes on a filesystem of their own (`--volume-dir`) so a full volume \
                    cannot take the snapshot store with it."
                    .to_string(),
                reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
            },
            Guardrail {
                id: "volume-content-token-is-the-only-credential".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "Whoever holds a volume's token reads and writes all of it, with no \
                    API key or team check"
                    .to_string(),
                description: "The content routes are outside the control plane's key check and \
                    outside the node's cluster-token gate: the volume's bearer token is the \
                    only credential, compared as a SHA-256 digest in constant time. The token \
                    is random and is returned once, by `create_volume` and by \
                    `rotate_volume_token`; `meta.json` (mode 0600) keeps only its digest, and \
                    volumes written before that were migrated when first read. `get_volume` \
                    and `list_volumes` never return it (`get_volume` carries a null `token`). \
                    `rotate_volume_token` replaces it, and the old token stops working when the \
                    new digest is published. Rotation is management, authorised as \
                    `get_volume` is: a key with the `volumes` scope in the volume's team, never \
                    the content token itself. A key with that scope can therefore rotate a \
                    volume's token and obtain a working one, so `volumes` keys still reach \
                    every volume of their team. The content routes do not look at a team, so \
                    the token works for whoever has it. A volume that does not exist and a \
                    wrong or missing token are one 404 with one body, so an unauthenticated \
                    caller cannot tell which volume IDs exist (the IDs derive from names with a \
                    non-cryptographic hash, so a caller who knows a name can still derive its \
                    ID). The control plane repeats a caller's `Authorization` header to the node \
                    for `/volumecontent` paths only, and never a session token."
                    .to_string(),
                applies_to: strings(&[
                    "volume_content",
                    "node_volume_content",
                    "volume_content_file",
                    "volume_content_dir",
                    "volume_content_path",
                    "node_read_volume_file",
                    "node_write_volume_file",
                    "node_list_volume_dir",
                    "node_make_volume_dir",
                    "node_stat_volume_path",
                    "node_update_volume_path",
                    "node_remove_volume_path",
                    "get_volume",
                    "node_get_volume",
                    "create_volume",
                    "node_create_volume",
                    "rotate_volume_token",
                    "node_rotate_volume_token",
                ]),
                condition: "A volume's token is shared with a client, logged, or kept where \
                    others read it, or the content routes are reachable by parties who should \
                    not be able to probe for volumes."
                    .to_string(),
                mitigation: "Treat the token as a password: do not log it, hand it only to the \
                    one client that needs it, rotate it with `rotate_volume_token` when it may \
                    have leaked (the old one stops at once), use `volumes`-scoped keys sparingly \
                    (they can rotate and so obtain any of their team's tokens), and terminate \
                    TLS in front of the control plane so the bearer token is not sent in clear. \
                    Tokens already printed by earlier versions, or kept in backups of \
                    `meta.json` made before this change, are live until rotated."
                    .to_string(),
                reference: "docs/VOLUMES.md".to_string(),
            },
            Guardrail {
                id: "volume-writes-are-not-atomic-and-delete-is-immediate".to_string(),
                severity: GuardrailSeverity::Info,
                title: "Uploads stream in place unless asked, a volume's delete is immediate, \
                    and nodes must share its directory"
                    .to_string(),
                description: "`write_volume_file` writes in place by default: an interrupted \
                    upload can leave a partial file, concurrent writers are last-writer-wins, \
                    and a failed `force=true` upload can leave empty parent directories. \
                    `atomic=true` stages and renames, needs room for the whole replacement, \
                    and a hard kill can leave a private `.hm-upload-*` file to clean up by \
                    hand. A file is at most 4 GiB. `delete_volume` removes every file at once \
                    and does not stop or refuse sandboxes mounting the volume: they see an \
                    empty directory. The control plane picks the node for a volume by hashing \
                    its ID, so nodes that do not share the volume directory (`--volume-dir`, or \
                    a shared snapshot store) will not find each other's volumes: a call picked \
                    to another node answers 404, and `list_volumes` shows only what the \
                    answering nodes hold."
                    .to_string(),
                applies_to: strings(&[
                    "node_write_volume_file",
                    "volume_content_file",
                    "delete_volume",
                    "node_delete_volume",
                    "volume_management",
                    "node_volume_management",
                    "list_volumes",
                ]),
                condition: "A volume holds data that must not be lost or half written, or the \
                    cluster has more than one node."
                    .to_string(),
                mitigation: "Upload with `atomic=true` (and `overwrite=false` for create-only), \
                    keep volume data in a shared, durable directory, stop the sandboxes that \
                    mount a volume before deleting it, and back the directory up."
                    .to_string(),
                reference: "docs/VOLUMES.md".to_string(),
            },
            Guardrail {
                id: "block-disks-are-node-local-and-unscoped".to_string(),
                severity: GuardrailSeverity::Warning,
                title: "Block disks belong to one node and to no team".to_string(),
                description: "The disk routes exist on the node only: the control plane neither \
                    forwards `/disks` nor places a sandbox on the node holding its disk, and \
                    a disk never moves between nodes. They read no team context, so every \
                    caller that passes the node's gate (the cluster token, or anyone when the \
                    node has none) lists, creates and deletes every disk, and a sandbox asks \
                    for one by name. The default disk directory is under the node's temporary \
                    directory (`--disk-dir` overrides it), which an operating system may \
                    clear; a claim by a daemon that died is treated as free, so a disk can be \
                    held by a new sandbox while the old guest is gone. Power-loss durability \
                    beyond `fsync` on a guest flush is untested."
                    .to_string(),
                applies_to: strings(&[
                    "node_block_disks",
                    "node_list_disks",
                    "node_create_disk",
                    "node_get_disk",
                    "node_delete_disk",
                ]),
                condition: "Disks hold data that must outlive the node or stay private to a \
                    tenant."
                    .to_string(),
                mitigation: "Set `--disk-dir` to durable storage, keep the node's port behind \
                    the control plane's network and its cluster token, and do not give a \
                    tenant direct access to the node."
                    .to_string(),
                reference: "docs/DISKS.md".to_string(),
            },
        ]
    }
}
