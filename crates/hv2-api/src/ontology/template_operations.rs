//! Templates and their builds, as the control plane (`hv2-cluster`,
//! `control.rs`: `templates`, `build_templates`, `template_alias`,
//! `delete_template`, `to_builder`) and the node (`hv2-sandboxd`: `main.rs`
//! `list_templates` and `build_template_route`, `builds.rs`, `snapshots.rs`)
//! serve them. Modelled as data: `hv2-api` depends on neither crate.
//!
//! Each operation is written once and built twice, as in
//! `network_operations`: `template_operations(false)` for the control plane,
//! `template_operations(true)` for the node (ids start `node_`). The node also
//! serves the share routes whose control-plane twins are in `ontology.rs`
//! (`build_template_share_operations`).
//!
//! The control plane relays every build call (`to_builder`) to one node, the
//! template's build node, chosen from the template's name by rendezvous hashing
//! over the live nodes; the node's answer comes back unchanged.

use super::network_operations::control_plane_gate;
use super::spec::{
    body_rejection, capability, fail, ok, path_param, plain, query_param, required_query, Spec,
};
use super::{
    Capability, Guardrail, GuardrailSeverity, HyperMachineOntology, Operation, OperationResponse,
    Parameter,
};
use serde_json::{json, Value};

/// A path string for the control plane or the node.
fn route(node: bool, control_plane: &'static str, node_path: &'static str) -> &'static str {
    if node {
        node_path
    } else {
        control_plane
    }
}

/// The `{id}` / `{templateID}` path parameter.
fn template_param(node: bool) -> Parameter {
    path_param(
        if node { "templateID" } else { "id" },
        "The template's name. An optional `:tag` is ignored by the routes that read it. A key \
         with a principal and no team names templates in its own scope: the control plane \
         puts `scope.<16 hex>--` before the name it sends on",
        json!({ "type": "string" }),
    )
}

/// The `{build}` / `{buildID}` path parameter.
fn build_param(node: bool) -> Parameter {
    path_param(
        if node { "buildID" } else { "build" },
        "The build's ID: the `buildID` that `request_template_build` answered. A node keeps \
         builds in memory only, so a node restart forgets every build",
        json!({ "type": "string" }),
    )
}

/// The `{hash}` path parameter.
fn hash_param() -> Parameter {
    path_param(
        "hash",
        "The files' hash, as the SDK names a `COPY`'s upload: 1 to 128 characters of letters, \
         digits, `-` and `_` (anything else is 400). Not necessarily a digest of the bytes; \
         see the upload-hash guardrail",
        json!({ "type": "string", "maxLength": 128 }),
    )
}

/// The refusals every route here can give.
fn template_gate(node: bool) -> Vec<OperationResponse> {
    if node {
        vec![fail(
            401,
            "the node was started with a cluster token and the request did not send it \
             (`this node answers its cluster's control plane only`); never returned when the \
             node has no token, which is the default",
        )]
    } else {
        control_plane_gate("templates", false)
    }
}

/// What the control plane adds for a caller limited to a scope, on the routes
/// that relay a call to the build node and name a template in the path.
fn scoped_relay_responses() -> Vec<OperationResponse> {
    vec![
        fail(
            400,
            "`invalid template ID` (the name holds a `%`, for a caller limited to a scope)",
        ),
        fail(
            404,
            "`no template ...`: the name is under another principal's `scope.` prefix (for a \
             caller limited to a scope)",
        ),
        fail(
            503,
            "no node is alive (`no node is alive`) or the cluster store is unavailable",
        ),
        fail(502, "the build node could not be reached"),
    ]
}

fn template_schema() -> Value {
    json!({ "type": "object", "properties": {
        "templateID": { "type": "string" },
        "buildID": { "type": "string", "description": "the template's name again; builds made from steps have their own IDs" },
        "aliases": { "type": "array", "items": { "type": "string" } },
        "public": { "type": "boolean", "description": "always false" },
        "buildStatus": { "type": "string", "description": "`ready`; on a node also `building` or `error`" },
        "snapshot": { "type": ["boolean", "null"], "description": "whether sandboxes are restored from a snapshot; null when unknown" },
        "cpuCount": { "type": ["integer", "null"] },
        "memoryMB": { "type": ["integer", "null"] },
        "nodeIDs": { "type": "array", "items": { "type": "string" }, "description": "control plane only" },
        "nodes": { "type": "array", "items": { "type": "object" }, "description": "control plane only: each node's `nodeID`, `snapshot`, `cpuCount` and `memoryMB`" },
        "envdVersion": { "type": "string", "description": "node only" },
        "detail": { "type": "string", "description": "node only, for a build in progress or failed: the image, and the error" }
    } })
}

fn build_accepted_schema() -> Value {
    json!({ "type": "object", "properties": {
        "templateID": { "type": "string" },
        "buildID": { "type": "string" },
        "public": { "type": "boolean" },
        "aliases": { "type": "array", "items": { "type": "string" } },
        "names": { "type": "array", "items": { "type": "string" }, "description": "`name:tag` for each tag" },
        "tags": { "type": "array", "items": { "type": "string" } }
    } })
}

impl HyperMachineOntology {
    /// The template and build operations of the control plane (`node` false)
    /// or the node.
    pub(super) fn template_operations(node: bool) -> Vec<Operation> {
        let gate = || template_gate(node);

        let mut specs = vec![
            Spec::new(
                "list_templates",
                "List Templates",
                "GET",
                "/templates",
            )
            .about(if node {
                "List the templates this node offers, in E2B's `Template` shape as far as it \
                 applies: those built from an OCI image (`POST /templates`), with their size \
                 (`cpuCount`, `memoryMB`), `snapshot` (whether sandboxes are restored from a \
                 snapshot rather than booted) and `envdVersion`, then those still building \
                 (`buildStatus: building`, `detail` the image) or failed (`buildStatus: \
                 error`, `detail` the image and the error). Every entry has `public: false`. \
                 Snapshots a team took (`GET /snapshots`) are not listed here. There is no \
                 filter and no pagination, and the answer is not limited to a team: the node \
                 serves its control plane, which does the limiting."
            } else {
                "List every template a live node offers, once each, with the nodes offering it. \
                 A node that reports no templates is taken to offer `base`. E2B's `Template` \
                 shape as far as the cluster knows it: `buildStatus` is always `ready`, \
                 `public` always false, `snapshot` is true when every node offering it says \
                 so and false when any says it is not, and `cpuCount` and `memoryMB` appear \
                 only when every node offering it agrees. A caller limited to a team or a \
                 principal sees the operator's templates, its own, and snapshots shared with \
                 it; a template under another principal's `scope.` prefix is not listed, and \
                 `aliases` shows a principal's own template without its prefix. A build \
                 still in progress or failed is not listed (use the build's status route, or \
                 the node's `GET /templates`). Also open to an `inventory` key and to an \
                 observer."
            })
            .answers(ok(
                200,
                "The templates.",
                Some(json!({ "type": "array", "items": template_schema() })),
            ))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "build_template_from_image",
                "Build A Template From An OCI Image",
                "POST",
                "/templates",
            )
            .about(if node {
                "Build a template from an OCI image, which this node pulls from its registry \
                 itself (no container runtime). It answers 202 at once; the build runs in the \
                 background and its progress is in `GET /templates`, where the template is \
                 `ready` when a create for it will work. The body is `{templateID, image, \
                 username?, password?, cpuCount?, memoryMB?}`: `templateID` is 1 to 64 \
                 letters, digits, `-`, `_` and `.`; `username` and `password` are used for \
                 this pull only and not kept, and are used only when both are sent; the size \
                 defaults to the node's `--cpu-cores` and `--memory-mb` and must be a size a \
                 guest here can be (`cpuCount` from 1 to the host's CPUs, `memoryMB` an even \
                 number from 256 to 65536), and it is fixed for the template. A name that is \
                 already a ready template is 409, and so is one already building; a name \
                 whose last build failed is rebuilt. The node needs `--guest-kit`, or the \
                 route is 501. This route builds from an image only; the E2B SDK's \
                 `Template.build()` uses `request_template_build` and `start_template_build` \
                 instead."
            } else {
                "Have every live node build a template from an OCI image: the request is sent \
                 to each node, not one, since without a shared snapshot store a template \
                 exists only where it was built, and with one the repeated builds converge \
                 on the same content-addressed files. The body is the node's \
                 (`build_template_from_image` on a node: `{templateID, image, username?, \
                 password?, cpuCount?, memoryMB?}`). The answer is an array with each \
                 node's `{nodeID, status, answer}`, and the status is 202 if any node took \
                 the build, else 502, whatever the nodes said (so a build every node \
                 refuses as a conflict is 502, and so is one made with no node alive, with \
                 `[]`). A key with a principal and no team has the name put under its own \
                 scope before it is sent, and is refused (403) a name under another's; a \
                 body that is not JSON, or has no string `templateID`, is sent as it is for \
                 the nodes to refuse. The registry `password`, if sent, goes to every node. \
                 The scope is not applied to a key with a team: its templates share the \
                 nodes' namespace (see the build-scope guardrail)."
            })
            .takes(
                json!({ "type": "object", "required": ["templateID", "image"], "properties": {
                    "templateID": { "type": "string", "maxLength": 64, "description": "letters, digits, `-`, `_`, `.`" },
                    "image": { "type": "string", "description": "an OCI image reference: `python:3.12-slim`, `ghcr.io/org/tool@sha256:...`" },
                    "username": { "type": "string", "description": "registry login for this pull; used only with `password`" },
                    "password": { "type": "string" },
                    "cpuCount": { "type": "integer", "minimum": 1 },
                    "memoryMB": { "type": "integer", "minimum": 256, "maximum": 65536, "description": "even" }
                } }),
                true,
            )
            .answers(ok(
                202,
                if node {
                    "The build was started: `{templateID, buildStatus: building}`."
                } else {
                    "At least one node took the build: an array of `{nodeID, status, answer}`, \
                     one per live node."
                },
                Some(if node {
                    json!({ "type": "object", "properties": {
                        "templateID": { "type": "string" },
                        "buildStatus": { "type": "string" } } })
                } else {
                    json!({ "type": "array", "items": { "type": "object", "properties": {
                        "nodeID": { "type": "string" },
                        "status": { "type": "integer" },
                        "answer": {} } } })
                }),
            ))
            .answers_all(if node {
                vec![
                    fail(
                        400,
                        "the template name is not 1 to 64 letters, digits, `-`, `_` and `.`, or \
                         the size is not one a guest here can be",
                    ),
                    fail(
                        409,
                        "the template already exists (`template NAME exists`) or is building",
                    ),
                    fail(
                        501,
                        "the node builds no templates: it was started without `--guest-kit`",
                    ),
                    body_rejection(),
                ]
            } else {
                vec![
                    fail(
                        403,
                        "the name is under another principal's `scope.` prefix (`that template \
                         name is under another principal's scope`)",
                    ),
                    fail(
                        502,
                        "no node accepted the build (none answered 202), including when no node \
                         is alive; the body lists each node's answer",
                    ),
                ]
            })
            .answers_all(gate()),
            Spec::new(
                "get_template_alias",
                "Check A Template Name",
                "GET",
                "/templates/aliases/{alias}",
            )
            .about(if node {
                "Say whether a template or snapshot of this name exists on this node: \
                 `{templateID, public: false}` if so, 404 if not. This is what the E2B SDK \
                 asks before a build to learn whether a name is taken. An optional `:tag` is \
                 dropped. It looks at templates built from an image and at snapshots; a \
                 build made from steps that has not finished is not found."
            } else {
                "Say whether a live node offers a template of this name to the caller: \
                 `{templateID, public: false}` if so, 404 `no template NAME` if not, and the \
                 same 404 for a name another principal or team owns (the two look the \
                 same). A key with a principal and no team gets its own template of that \
                 name, and `templateID` is then the scoped name (`scope.<16 hex>--name`), \
                 which is what a create must name; a name another scope owns is 404. A \
                 snapshot another team shared with the caller is found; one that was not is \
                 not."
            })
            .with(path_param(
                "alias",
                "The template's name, with an optional `:tag` that is dropped",
                json!({ "type": "string" }),
            ))
            .answers(ok(
                200,
                "The template exists.",
                Some(json!({ "type": "object", "properties": {
                    "templateID": { "type": "string" },
                    "public": { "type": "boolean", "description": "always false" } } })),
            ))
            .answers(fail(404, "no template of that name (for the caller)"))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "delete_template_snapshot",
                "Delete A Snapshot Template",
                "DELETE",
                route(node, "/templates/{id}", "/templates/{templateID}"),
            )
            .about(if node {
                "Delete a snapshot by name (a template made by `POST /sandboxes/{id}/snapshots` \
                 or by a build from steps). The snapshot's record, and its file when the node \
                 has a snapshot store, are removed and the cluster is told at once. \
                 Sandboxes already created from it run on. Only snapshots are deleted: a \
                 template built from an image is 409 and stays, and there is no way to \
                 delete one through the API. The control plane sends the caller's team with \
                 the delete, and this route deletes only what that team may: its own \
                 snapshots, or a principal's own scope's templates by their scoped names. \
                 Anything else of a team is 404, as a missing name is. With no team sent -- \
                 an administrator, or a caller holding only the cluster token -- any snapshot \
                 is deleted."
            } else {
                "Delete a snapshot wherever it is offered: the control plane sends the \
                 delete to every live node and answers 204 if any removed it. A team \
                 deletes its own snapshots and nothing else: not another team's, and not an \
                 operator's template every team uses (404 for both, which look the same). \
                 A key with a principal and no team also deletes the templates it built \
                 itself, which are its own by their scope prefix. An administrator may \
                 delete any. If no node removed it and one answered 409 (the name is a \
                 template built from an image), that is the answer; otherwise 404. Sandboxes \
                 already created from the snapshot run on."
            })
            .with(template_param(node))
            .answers(plain(
                204,
                if node {
                    "The snapshot was deleted."
                } else {
                    "At least one node deleted the snapshot."
                },
            ))
            .answers(fail(
                404,
                if node {
                    "no snapshot of that name on this node"
                } else {
                    "no snapshot of that name that the caller may delete, on any node (for a \
                     name that is not the caller's, or that no node has)"
                },
            ))
            .answers(fail(
                409,
                "the name is a template built from an image, which only snapshots are \
                 deleted (`NAME is a template built from an image; only snapshots are deleted`)",
            ))
            .answers_all(if node {
                vec![]
            } else {
                vec![fail(
                    400,
                    "`invalid snapshot ID`: the name cannot be one URL path segment",
                )]
            })
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "get_template_build_status",
                "Read A Template Build's Status",
                "GET",
                route(
                    node,
                    "/templates/{id}/builds/{build}/status",
                    "/templates/{templateID}/builds/{buildID}/status",
                ),
            )
            .about(format!(
                "Read the status of a build made with `request_template_build`: `status` is \
                 `waiting` (named, not started), `building`, `ready` or `error`, with the \
                 build's log entries and, for an error, a `reason` with the failing step \
                 (`base`, a step's number or `finalize`), the message and the last 20 log \
                 entries. At most 100 entries come back per call, from `logsOffset` (default \
                 0); poll with the offset advanced by the entries received. {} The node \
                 finds the build by its ID alone and does not compare it with the template in \
                 the path.{}",
                if node {
                    "A node keeps builds in memory: after a restart the build is 404, though \
                     a template it finished remains."
                } else {
                    "The control plane relays to the build node chosen from the template name."
                },
                if node {
                    ""
                } else {
                    " A caller limited to a principal's scope is told 404 `no build` unless \
                     the build's template is its own."
                }
            ))
            .with(template_param(node))
            .with(build_param(node))
            .with(query_param(
                "logsOffset",
                "The index of the first log entry to return",
                json!({ "type": "integer", "minimum": 0, "default": 0 }),
            ))
            .answers(ok(
                200,
                "The build's status and a page of its log.",
                Some(json!({ "type": "object", "properties": {
                    "templateID": { "type": "string" },
                    "buildID": { "type": "string" },
                    "status": { "type": "string", "enum": ["waiting", "building", "ready", "error"] },
                    "logs": { "type": "array", "description": "always empty; use `logEntries`" },
                    "logEntries": { "type": "array", "items": { "type": "object", "properties": {
                        "timestamp": { "type": "string" },
                        "level": { "type": "string" },
                        "message": { "type": "string" },
                        "step": { "type": "string" } } } },
                    "reason": { "type": "object", "description": "only for `error`: `message`, `step`, `logEntries`" }
                } })),
            ))
            .answers(fail(
                404,
                if node {
                    "no build with that ID on this node"
                } else {
                    "no build with that ID on the build node, or (for a caller limited to a \
                     scope) the build is not for the caller's template"
                },
            ))
            .answers_all(if node { vec![] } else { scoped_relay_responses() })
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "get_template_file_link",
                "Ask Where To Upload A Build's Files",
                "GET",
                route(
                    node,
                    "/templates/{id}/files/{hash}",
                    "/templates/{templateID}/files/{hash}",
                ),
            )
            .about(format!(
                "Say whether a `COPY` step's files are already on the node, and if not give a \
                 URL to `PUT` them to. Files are kept per scope: the caller's team's, or the \
                 node's own for an administrator or a node without teams. 201 `{{present: \
                 true}}` when a file of that hash is stored for the caller's scope, and \
                 another scope's file of that hash is neither seen nor reused; otherwise \
                 201 `{{present: false, url}}`, where the URL is \
                 `{{scheme}}://{{host}}/templates/{{template}}/files/{{hash}}?token=...`, \
                 the host from `X-Forwarded-Host` or `Host` as the caller reached it \
                 (through a control plane, the control plane's, which relays the upload). \
                 The token is one random value, held in memory as its digest, good for two \
                 hours and for that scope, template and hash alone. Asking again for the \
                 same scope, template and hash gives a second URL and leaves the first \
                 standing (the SDK asks once per `COPY`, and two `COPY`s of the same files \
                 share a hash); each is used up only by its own successful upload. Before \
                 it hands one out the node refuses \
                 (507) when it already holds 256 uploads or 64 GiB. {} Not an inventory \
                 route: an `inventory` key or an observer is refused, since the answer is a \
                 capability to write.",
                if node {
                    ""
                } else {
                    "The control plane sends this to the template's build node, the one \
                     `request_template_build` and `start_template_build` reach."
                }
            ))
            .with(template_param(node))
            .with(hash_param())
            .answers(ok(
                201,
                "`{present: true}`, or `{present: false, url}` with the upload URL.",
                Some(json!({ "type": "object", "properties": {
                    "present": { "type": "boolean" },
                    "url": { "type": "string", "description": "only when `present` is false" } } })),
            ))
            .answers(fail(
                400,
                "the hash is not 1 to 128 letters, digits, `-` or `_`",
            ))
            .answers(fail(
                507,
                "the node holds as many uploads, or as many bytes, as it keeps",
            ))
            .answers_all(if node { vec![] } else { scoped_relay_responses() })
            .answers_all(gate()),
            Spec::new(
                "upload_template_files",
                "Upload A Build's Files",
                "PUT",
                route(
                    node,
                    "/templates/{id}/files/{hash}",
                    "/templates/{templateID}/files/{hash}",
                ),
            )
            .about(format!(
                "Store a `COPY` step's files, as the SDK packs them (a tar, gzipped or not), \
                 under their hash, from the URL `get_template_file_link` gave. This route \
                 takes no API key and no cluster token: the SDK sends none with an upload, \
                 so the `token` query parameter stands in, compared in constant time with \
                 the ones `get_template_file_link` issued. The token is for one scope, \
                 template and hash: the bytes are stored under the scope the link was issued \
                 to, and a template or hash in the path other than the link's is refused \
                 403. It expires after two hours and is removed only after a successful \
                 upload. The body is streamed to a scratch file, then published with a hard \
                 link, so within a scope the first writer of a hash wins and a second upload \
                 of the same name is told 200 and discarded; nobody can replace another's \
                 file. One upload is at most 4 GiB. By default the bytes are not checked \
                 against the hash; with the node's `--verify-upload-hash` they must hash \
                 (SHA-256, hex) to it. {} See the upload guardrails.",
                if node {
                    "The route is served after the node's cluster-token check, which is why \
                     it needs no token of the cluster's."
                } else {
                    "The control plane relays it, unauthenticated, to the node chosen from \
                     the template name in the path: a name that is not the one the link was \
                     made for reaches another node and is refused 403."
                }
            ))
            .with(template_param(node))
            .with(hash_param())
            .with(required_query(
                "token",
                "The upload token from the URL `get_template_file_link` gave",
                json!({ "type": "string" }),
            ))
            .takes_raw(
                "application/octet-stream",
                json!({ "type": "string", "format": "binary", "description": "a tar, gzipped or not" }),
                true,
            )
            .answers(plain(
                200,
                "Stored, or a file of that hash was already stored (the new bytes are discarded).",
            ))
            .answers(fail(
                400,
                "the hash is not valid, or the body could not be read or written",
            ))
            .answers(fail(
                403,
                "`not an upload this node offered`: the token is missing, wrong, expired, \
                 used, voided by a later link for the same scope, template and hash, or \
                 offered for another template or hash (or, through a control plane, the \
                 request reached a node that issued none)",
            ))
            .answers(fail(413, "the upload is larger than 4 GiB"))
            .answers(fail(
                422,
                "with `--verify-upload-hash`, the bytes' SHA-256 is not the hash they were \
                 sent as; nothing is stored",
            ))
            .answers(fail(
                507,
                "the node holds 256 uploads or 64 GiB already",
            ))
            .answers_all(if node {
                vec![]
            } else {
                vec![
                    fail(400, "`invalid request path`"),
                    fail(503, "no node is alive or the cluster store is unavailable"),
                    fail(502, "the build node could not be reached"),
                ]
            }),
            Spec::new(
                "request_template_build",
                "Name A Template Build",
                "POST",
                "/v3/templates",
            )
            .about(format!(
                "Name a build for E2B's `Template.build()`: the first of its calls, before \
                 the files are uploaded and the build is started. Answers 202 with a new \
                 `buildID` (a UUID the node keeps in memory) and the names, and the build is \
                 `waiting` until `start_template_build`. The body is `{{name or alias, tags?, \
                 cpuCount?, memoryMB?}}`: `name` (else `alias`) is the template, 1 to 64 \
                 letters, digits, `-`, `_` and `.`, and an optional `:tag` on it is put \
                 first among the tags (the default tag is `default`). A name that already \
                 exists is rebuilt in place and sandboxes created from it afterwards get \
                 the new build, so naming an existing template is how to replace it; a name \
                 that is a template built from an image is 409 instead. A build made for a \
                 team is that team's snapshot: it replaces only its own team's template of \
                 the name, and a name another team or an operator holds is 409 (the node \
                 checks it at the request and again when the build is kept). The node needs \
                 `--guest-kit`, or 501. {}",
                if node {
                    "The node does not check who is asking."
                } else {
                    "The control plane reads the body (up to 1 MiB, else 400) for the name; a \
                     key with a principal and no team has its scope prefix put before the \
                     name (before any `:tag`), and a name under another scope is 403. The \
                     request is then relayed to the build node chosen from the name. A key \
                     with a team is refused (403, shared by every team) until templates are \
                     partitioned by team; an administrator names templates in the node's own \
                     namespace, which is shared by every such caller."
                }
            ))
            .takes(
                json!({ "type": "object", "properties": {
                    "name": { "type": "string", "description": "the template, with an optional `:tag`; `alias` is read when `name` is absent" },
                    "alias": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "cpuCount": { "type": "integer", "minimum": 1 },
                    "memoryMB": { "type": "integer", "minimum": 256, "maximum": 65536 }
                } }),
                true,
            )
            .answers(ok(
                202,
                "The build is named and waiting to be started.",
                Some(build_accepted_schema()),
            ))
            .answers(fail(
                400,
                "no name, a name that is not 1 to 64 letters, digits, `-`, `_` and `.`, or a \
                 size a guest here cannot be",
            ))
            .answers(fail(
                409,
                "the name is a template built from an image (`build under another name`)",
            ))
            .answers(fail(
                501,
                "the node builds no templates: it was started without `--guest-kit`",
            ))
            .answers(body_rejection())
            .answers_all(if node {
                vec![]
            } else {
                vec![
                    fail(
                        403,
                        "the name is under another principal's `scope.` prefix",
                    ),
                    fail(502, "the build node could not be reached"),
                ]
            })
            .answers_all(gate()),
            Spec::new(
                "start_template_build",
                "Start A Template Build",
                "POST",
                route(
                    node,
                    "/v2/templates/{id}/builds/{build}",
                    "/v2/templates/{templateID}/builds/{buildID}",
                ),
            )
            .about(format!(
                "Start a named build, from an image (`fromImage`, pulled by the node, with \
                 `fromImageRegistry` for a private registry) or from another template \
                 (`fromTemplate`): exactly one of the two. It answers 202 at once with no \
                 body and the build runs in the background; read it with \
                 `get_template_build_status`. The body's `steps` are E2B's, each \
                 `{{type, args, filesHash?}}` with type `RUN`, `COPY`, `ENV`, `WORKDIR` or \
                 `USER`; a `COPY` names its uploaded files by `filesHash`. `startCmd` is a \
                 process started in the build's sandbox and `readyCmd` the check that it is \
                 up. `force` is accepted. The steps run in a microVM of their own, never on \
                 the node, each `RUN` for at most an hour and a readiness check for ten \
                 minutes; then the sandbox, with the start command running, is snapshotted \
                 into the template, and every sandbox created from it is restored from \
                 that snapshot. A build that is not `waiting` is 409, so each build starts \
                 once. {}",
                if node {
                    "The registry login in `fromImageRegistry` can be a username and password, \
                     AWS keys or a GCP service account; it is used for this build's pull."
                } else {
                    "For a key with a principal and no team the control plane reads the body \
                     (up to 8 MiB, else 400) and resolves `fromTemplate` to that key's own \
                     template of the name when it has one, and refuses (400 `no template`) one \
                     under another scope; it also requires the build to be the caller's own \
                     (404 `no build`). The registry password in `fromImageRegistry` passes \
                     through the control plane."
                }
            ))
            .with(template_param(node))
            .with(build_param(node))
            .takes(
                json!({ "type": "object", "properties": {
                    "fromImage": { "type": "string" },
                    "fromTemplate": { "type": "string" },
                    "fromImageRegistry": { "type": "object", "description": "`type` and, by type, `username`/`password`, `awsAccessKeyId`/`awsSecretAccessKey`/`awsRegion` or `serviceAccountJson`" },
                    "force": { "type": "boolean" },
                    "steps": { "type": "array", "items": { "type": "object", "properties": {
                        "type": { "type": "string", "enum": ["RUN", "COPY", "ENV", "WORKDIR", "USER"] },
                        "args": { "type": "array", "items": { "type": "string" } },
                        "filesHash": { "type": "string" } } } },
                    "startCmd": { "type": "string" },
                    "readyCmd": { "type": "string" }
                } }),
                true,
            )
            .answers(plain(
                202,
                "The build was started; it is `building`.",
            ))
            .answers(fail(
                400,
                "not exactly one of `fromImage` and `fromTemplate` (the build stays `waiting` \
                 and can be started again)",
            ))
            .answers(fail(
                404,
                if node {
                    "no build with that ID on this node"
                } else {
                    "no build with that ID on the build node, or it is not the caller's \
                     (for a caller limited to a scope)"
                },
            ))
            .answers(fail(
                409,
                "the build is not `waiting`: it is already `building`, `ready` or `error`",
            ))
            .answers(body_rejection())
            .answers_all(if node { vec![] } else { scoped_relay_responses() })
            .answers_all(gate()),
        ];

        if node {
            specs.push(
                Spec::new(
                    "share_template_snapshot",
                    "Share A Snapshot With A Team",
                    "PUT",
                    "/templates/{templateID}/shares/{team}",
                )
                .about(
                    "Let `team` start sandboxes from a snapshot, on this node. What it starts \
                     from is the source sandbox as it was: its memory, its files and its \
                     environment variables, so the body must be \
                     `{\"acknowledgeContents\": true}` (400 without it). The node does not \
                     check who is asking: the control plane's `share_template_snapshot` \
                     limits the call to the snapshot's own team or an administrator and \
                     then tells every node that has it. The share list is written to the \
                     snapshot store when there is one, and the cluster is told. Sharing \
                     twice is the same as once. An operator's template (one with no team) \
                     is 409, as is sharing with the snapshot's own team.",
                )
                .with(template_param(true))
                .with(path_param(
                    "team",
                    "The team to share with: a bounded team label",
                    json!({ "type": "string" }),
                ))
                .takes(
                    json!({ "type": "object", "required": ["acknowledgeContents"], "properties": {
                        "acknowledgeContents": { "type": "boolean", "description": "must be true" } } }),
                    true,
                )
                .answers(ok(
                    200,
                    "Shared (or already shared).",
                    Some(json!({ "type": "object", "properties": {
                        "snapshotID": { "type": "string" },
                        "teamID": { "type": "string" },
                        "sharedWith": { "type": "array", "items": { "type": "string" } } } })),
                ))
                .answers(fail(
                    400,
                    "the team is not a valid label, or the body does not say \
                     `acknowledgeContents: true`",
                ))
                .answers(fail(404, "no snapshot of that name on this node"))
                .answers(fail(
                    409,
                    "the snapshot is an operator's template, or the team is its own",
                ))
                .answers(fail(
                    500,
                    "the share list could not be written to the snapshot store",
                ))
                .answers_all(template_gate(true))
                .repeatable(),
            );
            specs.push(
                Spec::new(
                    "unshare_template_snapshot",
                    "Stop Sharing A Snapshot With A Team",
                    "DELETE",
                    "/templates/{templateID}/shares/{team}",
                )
                .about(
                    "Stop sharing a snapshot with `team`, on this node. Sandboxes the team \
                     already started from it run on. The node does not check who is asking; \
                     the control plane limits the call to the snapshot's own team or an \
                     administrator. It is 204 whether or not the team was on the list. The \
                     same refusals as `node_share_template_snapshot` apply to an unknown \
                     snapshot, an operator's template and the snapshot's own team.",
                )
                .with(template_param(true))
                .with(path_param(
                    "team",
                    "The team to stop sharing with",
                    json!({ "type": "string" }),
                ))
                .answers(plain(204, "Not shared with the team any more."))
                .answers(fail(400, "the team is not a valid label"))
                .answers(fail(404, "no snapshot of that name on this node"))
                .answers(fail(
                    409,
                    "the snapshot is an operator's template, or the team is its own",
                ))
                .answers(fail(
                    500,
                    "the share list could not be written to the snapshot store",
                ))
                .answers_all(template_gate(true))
                .repeatable(),
            );
        }
        specs.into_iter().map(|spec| spec.build(node)).collect()
    }

    /// The capabilities over [`Self::template_operations`], for the control
    /// plane (`node` false) or the node.
    pub(super) fn build_template_capabilities(node: bool) -> Vec<Capability> {
        let (who, auth, perms, running): (&str, &str, &[&str], &str) = if node {
            (
                "the node daemon",
                "It checks only the cluster token when it has one and otherwise answers every \
                 caller that can reach it; the upload route checks neither",
                &["cluster token"],
                "a node (`hv2-sandboxd`) started with `--guest-kit`",
            )
        } else {
            (
                "the cluster control plane",
                "A key or session needs the `templates` scope, and a key with a principal and \
                 no team names templates in its own scope; the upload route takes no key",
                &["templates"],
                "a control plane with at least one live node with `--guest-kit`",
            )
        };
        let suffix = if node {
            "node daemon"
        } else {
            "cluster control plane"
        };
        let mut capabilities = vec![];
        capabilities.push(capability(
            "template_catalogue",
            &format!("Templates And Image Builds ({suffix})"),
            &format!(
                "List the templates offered, check whether a name is taken, build a template \
                 from an OCI image, and delete a snapshot, through {who}. {auth}. See \
                 docs/SANDBOXES.md"
            ),
            &[
                "list_templates",
                "get_template_alias",
                "build_template_from_image",
                "delete_template_snapshot",
            ],
            node,
            &[running],
            perms,
        ));
        capabilities.push(capability(
            "template_builds",
            &format!("E2B Template Builds ({suffix})"),
            &format!(
                "Build a template from steps as E2B's `Template.build()` does: name the build, \
                 ask where to upload a `COPY`'s files, upload them, start the build and read \
                 its status and log, through {who}. {auth}. See docs/SANDBOXES.md and \
                 docs/TOOLS_IMAGE.md"
            ),
            &[
                "request_template_build",
                "get_template_file_link",
                "upload_template_files",
                "start_template_build",
                "get_template_build_status",
            ],
            node,
            &[running],
            perms,
        ));
        if node {
            capabilities.push(capability(
                "team_template_sharing",
                "Snapshot Sharing (node daemon)",
                "Let another team start sandboxes from a team's snapshot, or stop, on this node. \
                 The node does not check the caller's team; the control plane does, and tells \
                 every node that has the snapshot. It checks only the cluster token when it \
                 has one. See docs/TEAMS.md",
                &["share_template_snapshot", "unshare_template_snapshot"],
                true,
                &["a node (`hv2-sandboxd`)", "a snapshot owned by a team"],
                &["cluster token"],
            ));
        }
        capabilities
    }
}

/// The guardrails for templates and their builds.
pub(super) fn template_guardrails() -> Vec<Guardrail> {
    let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    vec![
        Guardrail {
            id: "template-upload-hash-not-verified-by-default".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "A build's uploaded files are not checked against their hash unless the \
                    node is told to"
                .to_string(),
            description: "An upload is stored under the hash in its URL, and the node keeps \
                one file per hash for every template of its scope (a team's, or the node's \
                own for an administrator), so no other scope shares it. By default the \
                bytes are not hashed against that name (E2B's SDK names an upload by a hash \
                of its file list, not of the bytes, so a check that is on by default would \
                refuse every E2B build). A caller who holds an upload URL for a hash, or who \
                gets there first, therefore decides what every later `COPY` of that hash \
                runs, within its own scope on that node, and `get_template_file_link` then \
                says `present` to that scope. The first writer wins and nobody can replace a \
                stored file within its scope, but \
                the first writer is not checked. Starting the node with \
                `--verify-upload-hash` makes the name have to be the content's SHA-256 \
                (422 otherwise), at the cost of refusing the SDK's own names. A node holds \
                at most 256 uploads and 64 GiB, 4 GiB each (413, 507)."
                .to_string(),
            applies_to: ids(&[
                "template_builds",
                "node_template_builds",
                "get_template_file_link",
                "node_get_template_file_link",
                "upload_template_files",
                "node_upload_template_files",
                "start_template_build",
                "node_start_template_build",
            ]),
            condition: "Callers of the template routes are not all trusted equally, and the \
                node was not started with `--verify-upload-hash`."
                .to_string(),
            mitigation: "Build with content-addressed names and start nodes with \
                `--verify-upload-hash` where the clients allow it; restrict the `templates` \
                scope to callers who may run code in builds; remember that a build's steps \
                run as the build's author in a microVM."
                .to_string(),
            reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
        },
        Guardrail {
            id: "template-upload-route-needs-no-api-key".to_string(),
            severity: GuardrailSeverity::Info,
            title: "The upload route takes a token in the URL instead of an API key".to_string(),
            description: "`PUT /templates/{id}/files/{hash}` is mounted outside the control \
                plane's key check and the node's cluster-token check, because the SDK sends \
                no key with an upload. What authorises it is the `token` query parameter \
                that `get_template_file_link` issued, 244 random bits compared in constant \
                time against every live token. The node keeps each token's digest in memory, \
                for two hours, with the scope, template and hash it was issued for: a token \
                opens that one upload and no other. Asking again for the same scope, template \
                and hash gives another token and leaves the first standing; each is used up \
                only by its own successful upload. It travels in a URL, so a proxy that logs \
                URLs logs it, and an unused one stays good until it expires. \
                A control plane relays the upload to the build node chosen from the template \
                name in the path."
                .to_string(),
            applies_to: ids(&["upload_template_files", "node_upload_template_files"]),
            condition: "The control plane or node is behind a proxy that logs request URLs, \
                or two builds use the same files hash at once."
                .to_string(),
            mitigation: "Do not log query strings on these routes; use a fresh link for each \
                upload and upload at once."
                .to_string(),
            reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
        },
        Guardrail {
            id: "template-build-scope-residuals".to_string(),
            severity: GuardrailSeverity::Warning,
            title: "Template scoping covers names, not what builds share".to_string(),
            description: "Only a key with a principal and no team gets a scope: its builds \
                are stored as `scope.<16 hex>--name`, which no other caller can name, list, \
                use, build over or delete, and the control plane checks a build ID is the \
                caller's before a start or status call. Uploads and cached build steps are \
                kept per scope (a team's, or the node's own for an administrator), so one \
                scope's files and steps are never another's. What is not scoped: pulled \
                images are shared node-wide by identical input; templates built before the \
                scoping existed keep their plain names and \
                are visible to everyone; a key with a team, and an administrator, name \
                templates in the node's own namespace, so `request_template_build` over a \
                name another team's build used replaces it in place; and a scoped name \
                leaves 40 of the node's 64 characters."
                .to_string(),
            applies_to: ids(&[
                "template_catalogue",
                "node_template_catalogue",
                "template_builds",
                "node_template_builds",
                "list_templates",
                "request_template_build",
                "node_request_template_build",
                "build_template_from_image",
                "node_build_template_from_image",
                "delete_template_snapshot",
            ]),
            condition: "Several principals or teams build templates on the same cluster and \
                must not see or influence one another's."
                .to_string(),
            mitigation: "Give each tenant a key with its own principal (and no team) where \
                builds must be private, or its own cluster; treat identical inputs and old \
                template names as shared."
                .to_string(),
            reference: "docs/security/SECURITY_AUDIT_2026-11-16.md".to_string(),
        },
    ]
}
