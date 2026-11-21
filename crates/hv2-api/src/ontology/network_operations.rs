//! A sandbox's network: egress rules and the gateway's decisions, the
//! tunnels that carry a connection to or from a guest port, and the networks
//! sandboxes share, as the control plane (`hv2-cluster`, `control.rs`) and the
//! node (`hv2-sandboxd`: `main.rs`, `forwards.rs`, `networks.rs`) serve them.
//! Modelled as data: `hv2-api` depends on neither crate.
//!
//! The network routes the control plane relays (`forward`) are the node's
//! own handler behind an ownership check. The tunnels it does not relay: it
//! checks ownership, finds the node and upgrades a connection of its own to
//! the node's tunnel route, then splices the two. Exposure (public ports, web
//! sharing, domains, names) is in `exposure_operations`; metrics, logs and
//! events in `telemetry_operations`.

use super::spec::{body_rejection, fail, ok, path_param, plain, Spec};
use super::{HyperMachineOntology, Operation, OperationResponse, Parameter};
use serde_json::{json, Value};

/// What can refuse a request to a route the control plane answers itself
/// (not one it relays) that names a sandbox: the key or session check, the
/// scope the route needs, and for a caller limited to a team or an owner the
/// ownership check of the middleware.
pub(super) fn control_plane_gate(scope: &str, names_sandbox: bool) -> Vec<OperationResponse> {
    let mut responses = vec![
        fail(
            401,
            "missing, expired or wrong X-API-Key or session; with single sign-on configured \
             an anonymous request is refused too",
        ),
        fail(
            403,
            &format!(
                "the key or member lacks the `{scope}` scope, is an observer or viewer (those \
                 may read the inventory routes only), or a session-cookie change did not come \
                 from the control plane's own pages"
            ),
        ),
    ];
    if names_sandbox {
        responses.push(fail(
            403,
            "`sandbox owner required`: the sandbox belongs to another team, or to another \
             principal when the key has no team",
        ));
        responses.push(fail(
            400,
            "`invalid sandbox ID` (for a caller limited to a team or an owner)",
        ));
        responses.push(fail(
            404,
            "`sandbox does not exist` (for a caller limited to a team or an owner)",
        ));
    }
    responses.push(fail(
        503,
        "the cluster store is unavailable or the ownership lookup timed out",
    ));
    responses
}

/// The responses of a route that needs a key with a configured creator
/// (`principal_id`), as public ports, web sharing and private networks do.
/// `principal` is what the check is made against.
pub(super) fn owner_bound_responses() -> Vec<OperationResponse> {
    vec![
        fail(
            403,
            "`configured creator identity required`: the key or session has no `principal_id` \
             (a legacy administrator key, anonymous access and a control plane started with \
             --allow-unauthenticated all have none), or the sandbox has no owner",
        ),
        fail(
            403,
            "`sandbox belongs to a different or unassigned creator`: the principal is not the \
             sandbox's owner (a teammate's request is made as the sandbox's creator, so \
             teammates pass)",
        ),
        fail(400, "`invalid sandbox ID`"),
        fail(404, "`sandbox does not exist`"),
    ]
}

/// A path parameter naming a guest port.
fn port_param(what: &str) -> Parameter {
    path_param(
        "port",
        what,
        json!({ "type": "integer", "minimum": 1, "maximum": 65535 }),
    )
}

/// The answer of the gateway's decision log.
fn decisions_schema() -> Value {
    json!({ "type": "object", "properties": {
        "decisions": { "type": "array", "items": { "type": "object", "properties": {
            "seq": { "type": "integer", "description": "from 1, so a gap shows" },
            "at": { "type": "integer", "description": "unix milliseconds" },
            "kind": { "type": "string" },
            "destination": { "type": "string" },
            "name": { "type": ["string", "null"] },
            "verdict": { "type": "string", "description": "`allow` or `deny`, in lower case" },
            "reason": { "type": "string" } } } },
        "recorded": { "type": "integer", "description": "every decision made" },
        "dropped": { "type": "integer", "description": "those not in `decisions`" },
        "stats": { "type": "object", "properties": {
            "connectionsAllowed": { "type": "integer" },
            "connectionsRefused": { "type": "integer" },
            "dnsAnswered": { "type": "integer" },
            "dnsRefused": { "type": "integer" },
            "intercepted": { "type": "integer" } } }
    } })
}

impl HyperMachineOntology {
    /// The control plane's network routes of every kind this module and its
    /// siblings describe.
    pub(super) fn build_network_control_plane_operations() -> Vec<Operation> {
        Self::network_operations(false)
    }

    /// The node's: the same routes, and those only a node serves.
    pub(super) fn build_network_node_operations() -> Vec<Operation> {
        Self::network_operations(true)
    }

    fn network_operations(node: bool) -> Vec<Operation> {
        let mut operations = Self::egress_and_tunnel_operations(node);
        operations.extend(Self::exposure_operations(node));
        operations.extend(Self::telemetry_operations(node));
        if node {
            operations.extend(Self::private_and_local_network_node_operations());
        } else {
            operations.extend(Self::private_network_membership_operations());
        }
        operations
    }

    fn egress_and_tunnel_operations(node: bool) -> Vec<Operation> {
        let sandbox = || Self::sandbox_id_parameter(node);
        let access = |named: bool| Self::sandbox_access_responses(node, named);
        let no_such_sandbox = || {
            fail(
                404,
                "no running sandbox of that ID on the node (a paused one is not running)",
            )
        };

        let mut specs = vec![
            Spec::new(
                "update_sandbox_network",
                "Replace A Running Sandbox's Egress Rules",
                "PUT",
                "/sandboxes/{SB}/network",
            )
            .about(if node {
                "Replace the egress rules of a running sandbox, without restarting it. The \
                 body is E2B's `SandboxNetworkUpdateConfig` and it is the whole new policy, \
                 not a patch: what it leaves out is cleared, so a `denyOut` set when the \
                 sandbox was made is gone unless it is sent again. The rules are the \
                 create route's `network`: `allowOut` takes names (`example.com`, \
                 `*.example.com`), addresses and CIDRs; `denyOut` takes addresses and CIDRs \
                 only (a name there is 400); `allow_internet_access: false` (snake case, \
                 as on create) denies everything `allowOut` does not name; `rules` maps a \
                 host pattern to header transforms the gateway puts on matching requests; \
                 and `egressProxy` sends the sandbox's connections through a SOCKS5 proxy \
                 whose address must resolve and, unless the node runs \
                 --allow-private-egress-proxy, must not resolve into a private or internal \
                 range. The policy and the proxy are set together. The rules mean what \
                 they mean at creation: an `allowOut` sent alone is exclusive only under \
                 the node's deny default (`--egress-default deny`), and under `allow` it \
                 restricts nothing, so send `denyOut: [\"0.0.0.0/0\", \"::/0\"]` with it to \
                 hold on either kind of node. Private, link-local and cloud metadata \
                 addresses stay refused whatever the rules say (and the host's own \
                 interface addresses, as read when the node started), unless the operator \
                 granted the range. The `iam` tokens set at creation are kept; they are not \
                 part of an update. The handler swaps the gateway's policy and does not \
                 itself cut connections already open. The sandbox needs a network: one made \
                 on a node started without `--network`, or without `network`, is 400. \
                 Replacing with the same body is harmless."
            } else {
                "Replace the egress rules of a running sandbox, without restarting it, \
                 through the node that runs it. Only PUT is served: the control plane \
                 answers any other method 405 itself, as JSON with `Allow: PUT`, after the \
                 key and ownership checks and without contacting the node. The body is E2B's `SandboxNetworkUpdateConfig` and it is the \
                 whole new policy, not a patch: what it leaves out is cleared, so a \
                 `denyOut` set when the sandbox was made is gone unless it is sent again. \
                 `allowOut` takes names, addresses and CIDRs; `denyOut` takes addresses and \
                 CIDRs only; `allow_internet_access: false` (snake case, as on create) \
                 denies everything `allowOut` does not name; `rules` maps a host pattern \
                 to header transforms for matching requests; `egressProxy` is a SOCKS5 \
                 proxy that must resolve and, unless the node runs \
                 --allow-private-egress-proxy, must not resolve into a private or internal \
                 range. An `allowOut` sent alone is exclusive only on a node whose default \
                 is deny: where it is allow (`--egress-default allow`) it restricts \
                 nothing, so send `denyOut: [\"0.0.0.0/0\", \"::/0\"]` with it. The \
                 control plane cannot know which default a node has and does not say. \
                 Private, link-local and cloud metadata addresses stay refused whatever the \
                 rules say, unless the operator granted the range. An address opened only \
                 because an `allowOut` name resolved to it carries that name alone: a TLS \
                 SNI or plain-HTTP `Host` naming anything else is refused, also when the \
                 guest sends it after the gateway's 300 ms look or a byte at a time. Not \
                 covered: a connection there that names nothing (no SNI, not HTTP, such as \
                 SSH) is allowed; only the first HTTP request on a connection is checked, \
                 so a later request on a kept-alive plain-HTTP connection can name another \
                 host the same address serves; and an `egressProxy` is sent the name and \
                 resolves it itself. The `iam` tokens set at \
                 creation are kept. A sandbox needs a network to have rules: one made \
                 without is 400. A caller limited to a team or an owner reaches only \
                 its own sandboxes. Replacing with the same body is harmless."
            })
            .with(sandbox())
            .takes(
                json!({
                    "type": "object",
                    "properties": {
                        "allowOut": { "type": "array", "items": { "type": "string" } },
                        "denyOut": { "type": "array", "items": { "type": "string" }, "description": "addresses and CIDRs; a name is refused" },
                        "allow_internet_access": { "type": "boolean", "description": "snake case; false denies all that allowOut does not name" },
                        "rules": { "type": "object", "description": "host pattern to a list of {transform: {headers: {name: value}}}; later entries override earlier ones; a header name or value with CR, LF or NUL is refused" },
                        "egressProxy": { "type": "object", "required": ["address"], "properties": {
                            "address": { "type": "string" },
                            "username": { "type": "string" },
                            "password": { "type": "string" } } }
                    }
                }),
                true,
            )
            .answers(ok(204, "The sandbox's gateway now decides by the new rules.", None))
            .answers(fail(
                400,
                "`this sandbox has no network`; an `allowOut`, `denyOut` or `rules` entry that \
                 does not parse (the first one is named), including a name in `denyOut` and \
                 a bad header; or an `egressProxy` that does not resolve or resolves into a \
                 private or internal range",
            ))
            .answers(fail(
                405,
                "a method other than PUT (the control plane answers it itself, as JSON \
                 with `Allow: PUT`; a node answers with an empty body)",
            ))
            .answers(body_rejection())
            .answers(no_such_sandbox())
            .answers_all(access(true))
            .repeatable(),
            Spec::new(
                "get_sandbox_network_decisions",
                "Read What A Sandbox's Gateway Decided",
                "GET",
                "/sandboxes/{SB}/network/decisions",
            )
            .about(
                "What the egress gateway behind a running sandbox's NIC allowed and refused: \
                 the first 256 and the most recent 256 decisions, each with a `seq` (from 1, \
                 so a gap shows), `at` (unix milliseconds), the kind, destination and name, a \
                 verdict and a reason; `recorded` is every decision made and `dropped` those \
                 not returned; `stats` are the gateway's counters. It is not E2B's. The \
                 destinations and names are everything the guest tried to reach, so it is \
                 read by whoever may read the sandbox. A paused sandbox answers 404 and a \
                 sandbox made without a network answers 400.",
            )
            .with(sandbox())
            .answers(ok(200, "The decisions.", Some(decisions_schema())))
            .answers(fail(400, "`this sandbox has no network`"))
            .answers(no_such_sandbox())
            .answers_all(access(true))
            .repeatable(),
        ];

        let tunnel = |id: &'static str,
                      name: &'static str,
                      path: &'static str,
                      protocol: &str,
                      what: &str| {
            let reverse = id == "open_sandbox_reverse_tunnel";
            let mut spec = Spec::new(id, name, "GET", path)
                .about(format!(
                    "Upgrade the request to `{protocol}` (a bodyless HTTP/1.1 GET with \
                     `Connection: Upgrade` and `Upgrade: {protocol}`) and {what} The tunnel \
                     adds no TLS, HTTP or login: what the guest service does with the \
                     bytes is its own, and the only checks are the API's. It goes over the \
                     guest agent's vsock connection, with no network device, so a sandbox \
                     with no network serves it all the same. While one is open the sandbox \
                     counts as in use, so an idle pause does not freeze it, and the tunnel \
                     is closed when the sandbox pauses or ends. {} Port 0 is 400.",
                    if node {
                        "A sandbox that is paused, or not on this node, is 404."
                    } else {
                        "The control plane checks the caller owns the sandbox, finds its \
                         node and upgrades a connection of its own to the node's route, \
                         then splices the two; it waits 15 seconds for the node, and a \
                         paused sandbox is 409 (resume it first). A refusal from the node \
                         comes back with its status and a generic message."
                    }
                ))
                .with(sandbox())
                .with(port_param(if reverse {
                    "The guest port to listen on, on the guest's loopback"
                } else {
                    "The guest port to carry the connection to"
                }))
                .answers(plain(101, "Switching protocols: the tunnel is open."))
                .answers(fail(
                    400,
                    "the request is not that upgrade (not a bodyless HTTP/1.1 GET with the \
                     right `Connection` and `Upgrade`), or the port is 0",
                ))
                .answers(plain(
                    400,
                    "`port` is not a port number: the framework's own rejection",
                ));
            if node {
                spec = spec
                    .answers(fail(404, "`sandbox is not running on this node`"))
                    .answers(fail(
                        502,
                        if reverse {
                            "the guest could not listen on the port; a guest image built \
                             before reverse ports has an agent that does not know how to, \
                             and must be rebuilt"
                        } else {
                            "`guest port unavailable`: nothing in the guest accepted the \
                             connection within 5 seconds"
                        },
                    ))
                    .answers(fail(500, "the node could not pair the stream"));
                if reverse {
                    spec = spec.answers(fail(
                        429,
                        "the sandbox already has 64 connections waiting for a guest program \
                         to connect, across its ports",
                    ));
                }
            } else {
                spec = spec
                    .answers(fail(409, "`resume the sandbox before opening a tunnel`: it is paused"))
                    .answers(fail(
                        502,
                        "`port tunnel node connection failed`, or the node answered without \
                         the upgrade asked for",
                    ))
                    .answers(fail(
                        504,
                        "`port tunnel node connection timed out`: the node did not answer in 15 seconds",
                    ))
                    .answers(fail(
                        404,
                        "no such sandbox, or the node answered 404 (the sandbox is not running there)",
                    ))
                    .answers(fail(
                        503,
                        "the sandbox's node is not registered (`sandbox node is unavailable`), \
                         the cluster store is unavailable, or the node answered 503",
                    ))
                    .answers(fail(
                        429,
                        "a reverse tunnel only: the node refused because 64 are waiting; any other \
                         status the node gives (500, 502) arrives as `node could not open the guest port`",
                    ));
            }
            spec.answers_all(if node {
                access(false)
            } else {
                control_plane_gate("sandboxes", true)
            })
        };
        specs.push(tunnel(
            "open_sandbox_tcp_tunnel",
            "Open A TCP Connection To A Sandbox's Guest Port",
            "/sandboxes/{SB}/ports/{port}/tcp",
            "hv2-tcp/1",
            "carry one TCP connection to the guest's `127.0.0.1:{port}`: the guest agent \
             connects there, and the stream then carries bytes both ways. One upgrade \
             carries one connection; a caller wanting more opens more. It is how the CLI \
             reaches SSH, a database or a dev server in the guest.",
        ));
        specs.push(tunnel(
            "open_sandbox_udp_tunnel",
            "Open A UDP Tunnel To A Sandbox's Guest Port",
            "/sandboxes/{SB}/ports/{port}/udp",
            "hv2-udp/1",
            "carry datagrams to a UDP socket the guest agent makes on the guest's \
             `127.0.0.1`. Each datagram is framed as a big-endian two-byte length and its \
             bytes, at most 65,507, and an empty one is valid. It is UDP over a stream, \
             so it keeps order and a slow reader delays the rest.",
        ));
        specs.push(tunnel(
            "open_sandbox_udp6_tunnel",
            "Open An IPv6 UDP Tunnel To A Sandbox's Guest Port",
            "/sandboxes/{SB}/ports/{port}/udp6",
            "hv2-udp6/1",
            "carry datagrams as `open_sandbox_udp_tunnel` does, to the guest's `::1`; it \
             never falls back to IPv4.",
        ));
        specs.push(tunnel(
            "open_sandbox_reverse_tunnel",
            "Receive A Connection A Sandbox's Guest Makes",
            "/sandboxes/{SB}/ports/{port}/reverse",
            "hv2-reverse/1",
            "wait for a program in the guest to connect to `127.0.0.1:{port}` there, which \
             the guest agent listens on: the stream's first byte, `+`, says one did and \
             its bytes follow, both ways. One upgrade carries one connection, so a caller \
             keeps several waiting. A sandbox has at most 64 waiting, and the guest's \
             listener closes 5 seconds after the last one goes. It needs a guest agent \
             that knows reverse ports.",
        ));
        specs.into_iter().map(|spec| spec.build(node)).collect()
    }

    /// The private-network routes only a node serves: two the other nodes
    /// call, and the node's own local networks.
    fn private_and_local_network_node_operations() -> Vec<Operation> {
        let sandbox = || Self::sandbox_id_parameter(true);
        let access = || Self::sandbox_access_responses(true, false);
        let network = || {
            path_param(
                "name",
                "The network's name, as created",
                json!({ "type": "string" }),
            )
        };
        let members_schema = || {
            json!({ "type": "object", "properties": {
                "name": { "type": "string" },
                "members": { "type": "array", "items": { "type": "object", "properties": {
                    "sandboxID": { "type": "string" },
                    "alias": { "type": "string" },
                    "address": { "type": "string", "description": "`10.200.{network index}.{host}`" } } } } } })
        };
        let private_tunnel = |id: &'static str, path: &'static str, protocol: &str, what: &str| {
            Spec::new(id, "Receive A Private Connection To A Sandbox", "GET", path)
                .about(format!(
                    "The receiving end of a private network connection, which another node \
                     (or this one) opens for a sandbox in the same owner's network: \
                     upgrade to `{protocol}` with the cluster's mutual TLS and token and the \
                     private route context in `x-hv2-private-route` (the route claim, as \
                     JSON, at most 2048 bytes) and `x-hv2-private-source-node`, and {what} \
                     Before it opens the guest port, and again before it accepts, the node \
                     rereads both sandboxes' memberships, placement and registration from \
                     the shared store and refuses unless the claim still holds (403 `private \
                     route refused`), and every second after that, for at most 10 minutes: \
                     a connection whose membership is removed is closed, and none lives \
                     beyond 10 minutes. The node carries 128 such connections at once. The \
                     source label is trusted only inside the operator's mutually \
                     authenticated cluster: it is not bound to a node's own certificate. \
                     Not a route for a caller to use; an agent uses private networks \
                     through the guest's DNS (`<sandbox-id>.<tag>.hv2.internal`)."
                ))
                .with(sandbox())
                .with(port_param(
                    "The guest port, which the route claim names too",
                ))
                .answers(plain(101, "Switching protocols: the tunnel is open."))
                .answers(fail(
                    400,
                    "not that upgrade, port 0, a repeated or missing route header, a claim \
                     that does not parse or does not name this sandbox and port, or a source \
                     node that is not a plain identifier",
                ))
                .answers(fail(
                    401,
                    "the cluster token is repeated or missing or wrong (`unambiguous cluster \
                     authentication required`)",
                ))
                .answers(fail(
                    403,
                    "`private route refused`: the memberships, owners, placement or \
                     registration no longer match the claim",
                ))
                .answers(fail(404, "`sandbox is not running on this node`"))
                .answers(fail(
                    502,
                    "`guest port unavailable`: nothing in the guest accepted within 5 seconds",
                ))
                .answers(fail(
                    503,
                    "`private routes require an authenticated mutual-TLS cluster` (the node \
                     has no cluster, no token or no complete mTLS), the shared store could \
                     not be read in 3 seconds, or the node already carries 128 private \
                     connections",
                ))
                .answers(plain(
                    400,
                    "`port` is not a port number: the framework's own rejection",
                ))
                .answers_all(access())
        };

        let specs = vec![
            private_tunnel(
                "open_sandbox_private_tcp_tunnel",
                "/sandboxes/{SB}/private-ports/{port}/tcp",
                "hv2-tcp/1",
                "carry one TCP connection to the guest's `127.0.0.1:{port}`.",
            ),
            private_tunnel(
                "open_sandbox_private_udp_tunnel",
                "/sandboxes/{SB}/private-ports/{port}/udp",
                "hv2-udp/1",
                "carry IPv4 datagrams, each a big-endian two-byte length and at most 65,507 \
                 bytes (empty is valid).",
            ),
            Spec::new(
                "list_local_networks",
                "List The Node's Local Networks",
                "GET",
                "/networks",
            )
            .about(
                "Every network on this node, with its members, in name order. A local \
                 network joins sandboxes on this one node to each other by a DNS alias, as \
                 `docker network` does, with no cluster, no mTLS and no control plane. It is \
                 held in the node's memory: a node restart forgets every network. The \
                 control plane does not route `/networks`. The module that does the work \
                 states in its own header that it has been unit-tested for its bookkeeping \
                 and not run against live guests.",
            )
            .answers(ok(
                200,
                "The networks.",
                Some(json!({ "type": "array", "items": members_schema() })),
            ))
            .answers_all(access())
            .repeatable(),
            Spec::new(
                "create_local_network",
                "Create A Local Network",
                "POST",
                "/networks",
            )
            .about(
                "Make a named network on this node, which takes one /24 of `10.200.0.0/16`, \
                 chosen clear of the gateway's own range and never reused while the node \
                 runs, even after the network is removed. Names are first come: there is no \
                 team or owner, so whoever reaches the node names its networks and joins \
                 any sandbox to them. A repeat of a name is 409. Not idempotent.",
            )
            .takes(
                json!({ "type": "object", "required": ["name"], "properties": {
                    "name": { "type": "string", "description": "not empty or blank" } } }),
                true,
            )
            .answers(ok(
                201,
                "The network, with no members.",
                Some(members_schema()),
            ))
            .answers(fail(400, "`name must not be empty`"))
            .answers(fail(409, "`a network of that name already exists`"))
            .answers(fail(
                507,
                "`no addresses or network slots are left`: the node has made as many networks as it can",
            ))
            .answers(body_rejection())
            .answers_all(access()),
            Spec::new(
                "get_local_network",
                "Inspect A Local Network",
                "GET",
                "/networks/{name}",
            )
            .about(
                "One local network of this node with its members: each member's sandbox ID, \
                 the DNS alias it answers to inside the network, and its address in the \
                 network's /24 of `10.200.0.0/16`. A network that does not exist is 404. The \
                 node holds networks in memory only, so a restart forgets them.",
            )
            .with(network())
            .answers(ok(200, "The network.", Some(members_schema())))
            .answers(fail(404, "`no such network`"))
            .answers_all(access())
            .repeatable(),
            Spec::new(
                "delete_local_network",
                "Remove A Local Network",
                "DELETE",
                "/networks/{name}",
            )
            .about(
                "Remove a network that has no member, as `docker network rm` does: one with \
                 a member is 409, so that no gateway is left holding a network that is \
                 gone. Its /24 is not reused. The second call is 404.",
            )
            .with(network())
            .answers(ok(204, "Removed.", None))
            .answers(fail(404, "`no such network`"))
            .answers(fail(409, "`the network still has a member; disconnect it first`"))
            .answers_all(access()),
            Spec::new(
                "connect_sandbox_to_local_network",
                "Connect A Sandbox To A Local Network",
                "POST",
                "/networks/{name}/connect",
            )
            .about(
                "Join a sandbox to a network under a DNS alias, and give it the lowest free \
                 address in the network's /24 (from `.2`). It takes effect on the guest's \
                 next lookup, with nothing to restart, and works while the sandbox runs or \
                 not. A sandbox may be in several networks, once each, under an alias of \
                 its own in each; the guest resolves an alias only for networks it is in. \
                 The handler does not check that the sandbox exists or who owns it: any \
                 caller that reaches the node can join any sandbox ID, including one it \
                 does not own, to a network, and so reach it by alias from the other \
                 members; the sandbox's membership is dropped when it is deleted. Not \
                 idempotent: a second call is 409.",
            )
            .with(network())
            .takes(
                json!({ "type": "object", "required": ["sandboxID", "alias"], "properties": {
                    "sandboxID": { "type": "string" },
                    "alias": { "type": "string", "description": "not empty or blank; one holder in the network" } } }),
                true,
            )
            .answers(ok(
                200,
                "The membership.",
                Some(json!({ "type": "object", "properties": {
                    "network": { "type": "string" }, "sandboxID": { "type": "string" },
                    "alias": { "type": "string" }, "address": { "type": "string" } } })),
            ))
            .answers(fail(400, "`alias must not be empty`"))
            .answers(fail(404, "`no such network`"))
            .answers(fail(
                409,
                "the sandbox is already connected to this network, or the alias is taken there",
            ))
            .answers(fail(507, "the network has no address left"))
            .answers(body_rejection())
            .answers_all(access()),
            Spec::new(
                "disconnect_sandbox_from_local_network",
                "Disconnect A Sandbox From A Local Network",
                "POST",
                "/networks/{name}/disconnect",
            )
            .about(
                "Take a sandbox out of a node's local network, freeing its alias and its \
                 address on the network. The guest's very next lookup of the alias is \
                 refused. A sandbox that is not connected is 409, so a second call is 409. \
                 The node does not check who owns the sandbox.",
            )
            .with(network())
            .takes(
                json!({ "type": "object", "required": ["sandboxID"], "properties": {
                    "sandboxID": { "type": "string" } } }),
                true,
            )
            .answers(ok(204, "Disconnected.", None))
            .answers(fail(404, "`no such network`"))
            .answers(fail(409, "`that sandbox is not connected to this network`"))
            .answers(body_rejection())
            .answers_all(access()),
        ];
        specs.into_iter().map(|spec| spec.build(true)).collect()
    }
}
