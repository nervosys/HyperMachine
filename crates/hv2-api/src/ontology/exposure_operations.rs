//! What a control plane lets a sandbox be reached by: public ports, browser
//! sharing, custom domains, names, and the private network tags a sandbox
//! carries (`hv2-cluster`, `control.rs`: `reserve_public_port`,
//! `update_web_sharing`, `bind_domain`, `assign_sandbox_name`,
//! `update_private_membership` and their neighbours). The node serves none of
//! these: they are held in the cluster store. Modelled as data: `hv2-api`
//! depends on neither crate.

use super::network_operations::{control_plane_gate, owner_bound_responses};
use super::spec::{body_rejection, fail, ok, path_param, plain, Spec};
use super::{HyperMachineOntology, Operation};
use serde_json::{json, Value};

fn public_port_schema() -> Value {
    json!({ "type": "object", "properties": {
        "machinePort": { "type": "integer", "description": "the guest's port" },
        "publicPort": { "type": "integer", "description": "the port the native gateway listens on" },
        "protocol": { "type": "string", "enum": ["tcp", "udp", "both"] } } })
}

fn grant_schema() -> Value {
    json!({ "type": "object", "properties": {
        "subject": { "type": "string", "description": "1-128 characters of letters, digits and `._@+-`" },
        "expires_at": { "type": "integer", "description": "unix seconds, positive" } } })
}

fn binding_schema() -> Value {
    json!({ "type": "object", "properties": {
        "domain": { "type": "string", "description": "lower case, no final dot" },
        "sandbox_id": { "type": "string" },
        "port": { "type": "integer", "description": "the guest's HTTP port" } } })
}

fn domain_param() -> super::Parameter {
    path_param(
        "domain",
        "An ASCII DNS hostname of at most 253 bytes with at least two labels (punycode for an IDN): not an IP address, not the reserved sandbox hostname format. Lower-cased, one final dot removed",
        json!({ "type": "string" }),
    )
}

fn sandbox_param() -> super::Parameter {
    path_param("id", "The sandbox's ID", json!({ "type": "string" }))
}

impl HyperMachineOntology {
    /// The control plane's exposure routes; a node serves none.
    pub(super) fn exposure_operations(node: bool) -> Vec<Operation> {
        if node {
            return Vec::new();
        }
        let gate = || control_plane_gate("sandboxes", true);
        let store_timeout = || {
            fail(
                503,
                "the cluster store is unavailable or did not answer in 5 seconds; for a write \
                 the outcome may have been committed, so retry the same request",
            )
        };
        let mut specs = vec![
            Spec::new(
                "list_sandbox_public_ports",
                "List A Sandbox's Public Ports",
                "GET",
                "/sandboxes/{id}/public-ports",
            )
            .about(
                "The sandbox's reserved public ports: each guest port with the port the \
                 native TCP/UDP gateway listens on for it, and the protocol. Only the \
                 sandbox's owner sees them (the key needs a configured `principal_id`; a \
                 teammate acts as the creator), and the control plane must have been \
                 started with `--native-port-range` or every public-port route is 503. \
                 A reservation is a record: whether a listener exists depends on the \
                 separate `hv2-native-gateway` process, which polls the records.",
            )
            .with(sandbox_param())
            .answers(ok(
                200,
                "The reservations.",
                Some(json!({ "type": "array", "items": public_port_schema() })),
            ))
            .answers(fail(
                503,
                "`public port management is not enabled`: no --native-port-range",
            ))
            .answers(store_timeout())
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "reserve_sandbox_public_port",
                "Publish A Sandbox's Port On The Internet-Facing Gateway",
                "PUT",
                "/sandboxes/{id}/public-ports/{port}",
            )
            .about(
                "Reserve a stable public port for a guest port and protocol, from the \
                 operator's `--native-port-range` window. The native gateway, which polls \
                 the reservations, then listens on that port on the interface the operator \
                 gave it and relays raw TCP, UDP or both to the guest through the node's \
                 mutual TLS; the gateway adds no login or TLS of its own, so what protects \
                 the guest service is the service. The answer is 202 and means the \
                 reservation is stored, not that the listener is bound yet; a repeat or a \
                 change of protocol keeps the port. A sandbox has at most 16. Pause or node \
                 loss releases the listeners and keeps the reservation, and resume rebinds \
                 the same port; a fork needs its own. Public DNS, firewall and NAT are the \
                 operator's. Needs a key with a configured `principal_id` that is the \
                 sandbox's owner (a teammate may); a legacy administrator key, an \
                 observer, and a control plane started with --allow-unauthenticated are \
                 refused. A store call that times out may have committed, so retry the same \
                 request.",
            )
            .with(sandbox_param())
            .with(path_param(
                "port",
                "The guest's port, 1 to 65535",
                json!({ "type": "integer", "minimum": 1, "maximum": 65535 }),
            ))
            .takes(
                json!({ "type": "object", "properties": {
                    "protocol": { "type": "string", "enum": ["tcp", "udp", "both"], "default": "tcp" } } }),
                false,
            )
            .answers(ok(
                202,
                "Reserved: `machinePort`, `publicPort` and `protocol`. No owner is in the answer.",
                Some(public_port_schema()),
            ))
            .answers(fail(
                400,
                "`invalid sandbox or destination port` (port 0, or an ID that is not a plain identifier)",
            ))
            .answers(body_rejection())
            .answers(fail(409, "`sandbox public port limit reached`: 16 are reserved"))
            .answers(fail(
                503,
                "`public port pool exhausted`, or `public port management is not enabled` \
                 (no --native-port-range)",
            ))
            .answers(store_timeout())
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "remove_sandbox_public_port",
                "Remove A Sandbox's Public Port",
                "DELETE",
                "/sandboxes/{id}/public-ports/{port}",
            )
            .about(
                "Delete the reservation for a guest port, which frees the public port for \
                 reuse. The native gateway notices on its next poll: the 204 is not a \
                 barrier, so a connection may still be carried after it. The second call is \
                 404. Same key requirements as `reserve_sandbox_public_port`.",
            )
            .with(sandbox_param())
            .with(path_param(
                "port",
                "The guest's port the reservation was made for",
                json!({ "type": "integer", "minimum": 1, "maximum": 65535 }),
            ))
            .answers(ok(204, "Removed.", None))
            .answers(fail(
                404,
                "`public port reservation does not exist`, or no such sandbox",
            ))
            .answers(fail(400, "`invalid sandbox or destination port`"))
            .answers(fail(
                503,
                "`public port management is not enabled`, or the store did not answer",
            ))
            .answers_all(owner_bound_responses())
            .answers_all(gate()),
            Spec::new(
                "get_sandbox_web_sharing",
                "Read Who A Sandbox's Web Is Shared With",
                "GET",
                "/sandboxes/{id}/web-sharing",
            )
            .about(
                "The owner's grants for the sandbox's guest URLs (`{port}-{sandboxID}` and \
                 bound domains): each a subject and an expiry in unix seconds, in subject \
                 order, with the revision to send back when replacing them. A sandbox that \
                 was never shared answers `{\"revision\": null, \"grants\": []}`; one \
                 whose grants were all revoked answers no grants and keeps its revision. \
                 A grant lets a browser identity open the sandbox's web; it is not an API \
                 permission. Needs a key with a configured `principal_id` that is the \
                 sandbox's owner (a teammate may).",
            )
            .with(sandbox_param())
            .answers(ok(
                200,
                "The sharing state.",
                Some(json!({ "type": "object", "properties": {
                    "revision": { "type": ["string", "null"] },
                    "grants": { "type": "array", "items": grant_schema() } } })),
            ))
            .answers(store_timeout())
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "set_sandbox_web_sharing",
                "Replace Who A Sandbox's Web Is Shared With",
                "PUT",
                "/sandboxes/{id}/web-sharing",
            )
            .about(
                "Replace the whole grant list for the sandbox's guest URLs. Each grant names \
                 a subject, which is either a browser credential the operator provisioned \
                 with `allow_owner_grants: true` in the web-access file, or the email of a \
                 member signed in through single sign-on with `--sso-guest-urls`; once \
                 granted until `expires_at`, that identity opens every port and bound \
                 domain of this sandbox, and only this instance of it (a fork does not \
                 inherit). It is not public access, and it grants no API, exec or envd \
                 access. Operator scopes in the web-access file authorize independently \
                 and are not revoked by removing a grant here. It is a compare-and-set: \
                 send the `expectedRevision` read by GET (`null` for a sandbox never \
                 shared) and a new canonical UUID v4 `revision`; an exact retry of an \
                 uncertain request succeeds, a stale revision is 409. At most 256 unique \
                 subjects within 64 KiB; an empty list revokes and keeps the revision. \
                 Revocation applies to later requests; a connection already admitted keeps \
                 its admission. Needs a key with a configured `principal_id` that is the \
                 sandbox's owner (a teammate may).",
            )
            .with(sandbox_param())
            .takes(
                json!({ "type": "object", "required": ["revision", "grants"], "properties": {
                    "expectedRevision": { "type": ["string", "null"], "description": "canonical lower-case UUID v4, or null" },
                    "revision": { "type": "string", "description": "a new canonical lower-case UUID v4" },
                    "grants": { "type": "array", "maxItems": 256, "items": grant_schema() } } }),
                true,
            )
            .answers(ok(
                200,
                "Applied: the new state.",
                Some(json!({ "type": "object", "properties": {
                    "revision": { "type": "string" },
                    "grants": { "type": "array", "items": grant_schema() } } })),
            ))
            .answers(fail(
                400,
                "`invalid expected sharing revision`, a `revision` that is not a canonical \
                 UUID v4, a duplicate subject, or more than 256 grants",
            ))
            .answers(plain(
                422,
                "a grant that is malformed (an empty or wildcard subject, a non-positive \
                 expiry, an unknown field) or an unknown field: the framework's own rejection; \
                 413 for a body over 64 KiB",
            ))
            .answers(fail(
                409,
                "`sharing revision or sandbox incarnation changed; read current state`",
            ))
            .answers(store_timeout())
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "list_sandbox_domains",
                "List A Sandbox's Custom Domains",
                "GET",
                "/sandboxes/{id}/domains",
            )
            .about(
                "The hostnames bound to this sandbox, each with the guest HTTP port it \
                 reaches, sorted by hostname. Needs the `sandboxes` scope (an inventory key \
                 does not reach it) and access to the sandbox.",
            )
            .with(sandbox_param())
            .answers(ok(
                200,
                "The bindings.",
                Some(json!({ "type": "array", "items": binding_schema() })),
            ))
            .answers(fail(404, "`sandbox does not exist`"))
            .answers(fail(500, "`internal error`: the store failed; the detail is only logged"))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "get_sandbox_domain_challenge",
                "Get The DNS Challenge For A Custom Domain",
                "GET",
                "/sandboxes/{id}/domains/{domain}/challenge",
            )
            .about(
                "The TXT record to publish to prove the caller controls a hostname, when \
                 the control plane runs with `--domain-verification-file`: a record name \
                 (`_hypermachine-domain.{hostname}`), the type `TXT`, a value that is an \
                 HMAC bound to the hostname, this sandbox and the cluster namespace, and \
                 `expires_at` (unix seconds, ten minutes ahead). After publishing, bind with \
                 that `expires_at` as `challenge_expires_at`. A proof for one sandbox does \
                 not claim a replacement or a fork. Nothing is stored. Without the policy \
                 the route is 501 and binding needs no proof.",
            )
            .with(sandbox_param())
            .with(domain_param())
            .answers(ok(
                200,
                "The challenge.",
                Some(json!({ "type": "object", "properties": {
                    "record_name": { "type": "string" },
                    "record_type": { "type": "string", "enum": ["TXT"] },
                    "record_value": { "type": "string" },
                    "expires_at": { "type": "integer" } } })),
            ))
            .answers(fail(400, "an invalid hostname, or one too long for its TXT name"))
            .answers(fail(404, "`sandbox does not exist`"))
            .answers(fail(501, "`DNS ownership verification is not configured`"))
            .answers(fail(500, "`internal error`: the store failed"))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "bind_sandbox_domain",
                "Bind A Custom Domain To A Sandbox",
                "PUT",
                "/sandboxes/{id}/domains/{domain}",
            )
            .about(
                "Route an operator-managed hostname to one HTTP port of a sandbox: the \
                 control plane's workload proxy then serves requests for that hostname, \
                 with their path, query and body unchanged, from the guest port, over \
                 HTTP/1.1 and HTTP/2, and wakes the sandbox for a request if it is paused \
                 and was made with `autoResume`. DNS and a certificate whose names cover the \
                 hostname are the operator's. Ownership of a hostname is exclusive in the \
                 cluster namespace and goes to the first claim; the same sandbox may \
                 call again to change the port. The proxy adds no login of its own unless \
                 the operator turned on browser login (`--web-access-file` or \
                 `--sso-guest-urls`). When the control plane runs with \
                 `--domain-verification-file`, a new binding or a port change needs a DNS \
                 proof: send the `expires_at` of a challenge as `challenge_expires_at` \
                 (428 without it, 403 if the TXT record is not there). Otherwise any caller \
                 with the `sandboxes` scope on the sandbox may claim any free hostname, \
                 whether or not they control its DNS. A proof is checked once: it expiring \
                 or the record going later does not unbind. The hostname is canonicalized \
                 to lower case.",
            )
            .with(sandbox_param())
            .with(domain_param())
            .takes(
                json!({ "type": "object", "required": ["port"], "properties": {
                    "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "challenge_expires_at": { "type": "integer", "description": "the challenge's `expires_at`; required when DNS verification is on" } } }),
                true,
            )
            .answers(ok(200, "Bound.", Some(binding_schema())))
            .answers(fail(
                400,
                "an invalid hostname (not ASCII DNS, an IP address, one label, or the reserved \
                 sandbox hostname format), port 0, or an invalid sandbox ID",
            ))
            .answers(body_rejection())
            .answers(fail(
                403,
                "the DNS proof is wrong or expired (`challenge_expires_at` must be in the \
                 future and no more than ten minutes ahead), or the owner check failed",
            ))
            .answers(fail(404, "`sandbox does not exist`"))
            .answers(fail(
                409,
                "`domain is already bound to another sandbox`",
            ))
            .answers(fail(
                428,
                "`DNS ownership challenge is required`: the policy is on and the body has no \
                 `challenge_expires_at`",
            ))
            .answers(fail(
                503,
                "the DNS resolver could not be asked or answered too slowly or too much: the \
                 message is the generic `the cluster store is unavailable`",
            ))
            .answers(fail(500, "`internal error`: the store failed"))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "unbind_sandbox_domain",
                "Unbind A Custom Domain From A Sandbox",
                "DELETE",
                "/sandboxes/{id}/domains/{domain}",
            )
            .about(
                "Stop routing a hostname to the sandbox. It needs no DNS proof and works \
                 while the resolver is down. A request already in flight may finish. \
                 Binding also goes when the sandbox is deleted or reaped. The second call \
                 is 404.",
            )
            .with(sandbox_param())
            .with(domain_param())
            .answers(ok(204, "Unbound.", None))
            .answers(fail(400, "an invalid hostname"))
            .answers(fail(
                404,
                "`domain binding does not exist for this sandbox`",
            ))
            .answers(fail(500, "`internal error`: the store failed"))
            .answers_all(gate()),
            Spec::new(
                "assign_sandbox_name",
                "Give A Sandbox A Name",
                "PUT",
                "/sandboxes/{id}/names/{name}",
            )
            .about(
                "Reserve a name for a sandbox, so it can be found with `resolve_sandbox_name`. \
                 A name is 1 to 64 ASCII letters, digits, `-`, `_` or `.` (not `.` or \
                 `..`), case-sensitive and never normalized. Names are one namespace for \
                 the whole cluster, not one per team: a name held by another team's \
                 sandbox is 409. A name already held by this sandbox answers 200 again; \
                 a sandbox may hold more than one name. A sandbox with the same name in its \
                 `hm.name` metadata blocks it (409). There is no body. A failed store call \
                 keeps the reservation and answers 503 `name reconciliation required`, which \
                 a later identical call settles.",
            )
            .with(sandbox_param())
            .with(path_param(
                "name",
                "The name to hold",
                json!({ "type": "string", "pattern": "^[A-Za-z0-9._-]{1,64}$" }),
            ))
            .answers(ok(
                200,
                "Held.",
                Some(json!({ "type": "object", "properties": {
                    "name": { "type": "string" }, "sandboxID": { "type": "string" } } })),
            ))
            .answers(fail(400, "an invalid name"))
            .answers(fail(404, "`sandbox is missing`"))
            .answers(fail(
                409,
                "`name is owned or requires reconciliation` (another sandbox holds it), \
                 `name conflicts with legacy sandbox metadata`, or `sandbox or name ownership changed`",
            ))
            .answers(fail(
                503,
                "the store is unavailable: `sandbox lookup unavailable`, `name reservation \
                 unavailable`, `legacy name lookup unavailable`, or `name reconciliation required`",
            ))
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "resolve_sandbox_name",
                "Find A Sandbox By Name",
                "GET",
                "/sandbox-names/{name}",
            )
            .about(
                "The sandbox ID a name was reserved for. Only a sandbox the caller may reach \
                 is answered: a name held by another team's sandbox is 403 `sandbox owner \
                 required`, which tells the caller the name is taken. A name whose creation \
                 outcome is unknown is 409 and a name that a sandbox's `hm.name` metadata \
                 also claims is 409 too. Needs the `sandboxes` scope.",
            )
            .with(path_param(
                "name",
                "The name, 1 to 64 ASCII letters, digits, `-`, `_` or `.`",
                json!({ "type": "string" }),
            ))
            .answers(ok(
                200,
                "The sandbox.",
                Some(json!({ "type": "object", "properties": {
                    "name": { "type": "string" }, "sandboxID": { "type": "string" } } })),
            ))
            .answers(fail(400, "an invalid name"))
            .answers(fail(403, "`sandbox owner required`"))
            .answers(fail(404, "`name is not reserved`, or `named sandbox is missing`"))
            .answers(fail(
                409,
                "`name creation outcome requires reconciliation`, or `name conflicts with \
                 legacy sandbox metadata`",
            ))
            .answers(fail(503, "a name, legacy-name or sandbox lookup is unavailable"))
            .answers_all(control_plane_gate("sandboxes", false))
            .repeatable(),
            Spec::new(
                "refresh_sandbox",
                "Refresh A Sandbox (Keep-Alive)",
                "POST",
                "/sandboxes/{id}/refreshes",
            )
            .about(
                "E2B's keep-alive. Makes the running sandbox end `duration` seconds from this \
                 request (default 300, from 0 to 3600) if that is later than the end it already \
                 has. It only ever extends: a shorter duration changes nothing, and a sandbox's \
                 end is never brought in. The 24-hour cap on a lifetime applies, as it does to \
                 `set_sandbox_timeout`, which is the route that can also shorten. A paused \
                 sandbox is 404: resume or connect to it first. The control plane checks the \
                 caller as it does for `set_sandbox_timeout` and relays the request to the \
                 sandbox's node, which records the new end.",
            )
            .with(sandbox_param())
            .takes(
                json!({
                    "type": "object",
                    "properties": { "duration": { "type": "integer", "minimum": 0,
                        "maximum": 3600, "description": "seconds from now; default 300" } }
                }),
                false,
            )
            .answers(ok(204, "The end is extended, or already was that late.", None))
            .answers(fail(
                400,
                "`duration` is not an integer from 0 to 3600, or the body is not JSON",
            ))
            .answers(fail(
                409,
                "its registration is unsettled: reconcile it before refreshing",
            ))
            .answers_all(super::HyperMachineOntology::sandbox_access_responses(false, true)),
        ];
        specs.shrink_to_fit();
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }

    /// The private network tags a sandbox carries, which only the control
    /// plane serves (the node only receives the connections they allow).
    pub(super) fn private_network_membership_operations() -> Vec<Operation> {
        let sandbox = sandbox_param;
        let gate = || control_plane_gate("sandboxes", true);
        let tags = json!({ "type": "array", "maxItems": 8, "items": {
            "type": "string", "pattern": "^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$" },
            "description": "lowercase DNS labels; sorted in answers" });
        let specs = vec![
            Spec::new(
                "get_sandbox_private_networks",
                "Read A Sandbox's Private Network Tags",
                "GET",
                "/sandboxes/{id}/private-networks",
            )
            .about(
                "The tags that put a sandbox in private networks, with the revision to send \
                 back when changing them. Sandboxes of one owner that share a tag reach each \
                 other's ports by DNS name (`<sandbox-id>.<tag>.hv2.internal`) over the \
                 cluster, with no route through the Internet; tags never join sandboxes of \
                 different owners. An untouched sandbox answers `{\"revision\": null, \
                 \"tags\": []}`; one whose membership was removed answers no tags and \
                 keeps its revision. Needs a key with a configured `principal_id` that is \
                 the sandbox's owner (a teammate may).",
            )
            .with(sandbox())
            .answers(ok(
                200,
                "The membership.",
                Some(json!({ "type": "object", "properties": {
                    "revision": { "type": ["string", "null"] }, "tags": tags.clone() } })),
            ))
            .answers(fail(
                503,
                "`private membership store unavailable; read current state before retrying`, or \
                 `private membership outcome uncertain; retry the exact revision and request`",
            ))
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
            Spec::new(
                "set_sandbox_private_networks",
                "Replace A Sandbox's Private Network Tags",
                "PUT",
                "/sandboxes/{id}/private-networks",
            )
            .about(
                "Replace the whole set of tags a sandbox carries, at most 8 distinct \
                 lowercase DNS labels; an empty list removes the membership (a revision is \
                 kept so an old request cannot revive it). A compare-and-set: send the \
                 `expectedRevision` read by GET (`null` for a sandbox never tagged) and a new \
                 canonical UUID v4 `revision`; an exact retry of an uncertain request \
                 succeeds while that state is current, and a stale revision or a sandbox that \
                 restarted is 409. The tags are required, and the body is at most 4 KiB. \
                 New connections are checked against the new tags; a connection already \
                 open is closed by its receiving node when its recheck, made every \
                 second, no longer holds (one measured closure took about a second, which \
                 is a sample, not a limit), and none lives beyond 10 minutes. The tags work only on a cluster \
                 whose nodes have mutual TLS, a cluster token and guest networking. Needs a \
                 key with a configured `principal_id` that is the sandbox's owner.",
            )
            .with(sandbox())
            .takes(
                json!({ "type": "object", "required": ["revision", "tags"], "properties": {
                    "expectedRevision": { "type": ["string", "null"], "description": "canonical lower-case UUID v4, or null" },
                    "revision": { "type": "string", "description": "a new canonical lower-case UUID v4" },
                    "tags": tags } }),
                true,
            )
            .answers(ok(
                200,
                "Applied.",
                Some(json!({ "type": "object", "properties": {
                    "revision": { "type": "string" }, "tags": { "type": "array", "items": { "type": "string" } } } })),
            ))
            .answers(fail(
                400,
                "`invalid expected membership revision`, a `revision` that is not a canonical \
                 UUID v4, duplicate tags, or more than 8; a tag that is not a lowercase DNS \
                 label or an unknown field is the framework's own 422, and a body over 4 KiB 413",
            ))
            .answers(body_rejection())
            .answers(fail(
                409,
                "`membership revision or sandbox incarnation changed; read current state`",
            ))
            .answers(fail(
                503,
                "the store is unavailable (`read current state before retrying`) or the \
                 outcome is uncertain (`retry the exact revision and request`)",
            ))
            .answers_all(owner_bound_responses())
            .answers_all(gate())
            .repeatable(),
        ];
        specs.into_iter().map(|spec| spec.build(false)).collect()
    }
}
