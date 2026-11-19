//! The capabilities that group the network, exposure, telemetry, event and
//! registration operations, for the control plane and for the node. The
//! operations are in `network_operations`, `exposure_operations` and
//! `telemetry_operations`.

use super::spec::capability;
use super::{Capability, HyperMachineOntology};

impl HyperMachineOntology {
    /// The capabilities of the control plane (`node` false) or the node.
    /// Operations only one of them serves are listed for that one.
    pub(super) fn build_network_capabilities(node: bool) -> Vec<Capability> {
        let (who, auth, perms, running): (&str, &str, &[&str], &str) = if node {
            (
                "the node daemon",
                "It checks only the cluster token when it has one and otherwise answers \
                 every caller that can reach it",
                &["cluster token"],
                "a node (`hv2-sandboxd`)",
            )
        } else {
            (
                "the cluster control plane",
                "A key or session needs the `sandboxes` scope, and a team's key acts on its \
                 own team's sandboxes only",
                &["sandboxes"],
                "a control plane with at least one live node",
            )
        };
        let suffix = if node {
            "node daemon"
        } else {
            "cluster control plane"
        };
        let mut capabilities = vec![
            capability(
                "sandbox_egress_network",
                &format!("Sandbox Egress Rules And Decisions ({suffix})"),
                &format!(
                    "Replace a running sandbox's egress rules (the whole policy, not a patch) \
                     and read what its gateway allowed and refused, through {who}. {auth}. See \
                     docs/SANDBOXES.md and docs/MACHINES.md"
                ),
                &["update_sandbox_network", "get_sandbox_network_decisions"],
                node,
                &[running, "a running sandbox with a network"],
                perms,
            ),
            capability(
                "sandbox_port_tunnels",
                &format!("Sandbox Port Tunnels ({suffix})"),
                &format!(
                    "Carry a TCP connection or UDP datagrams to a port in a running sandbox's \
                     guest, or wait for a program in the guest to connect out to the caller, \
                     over an HTTP/1.1 upgrade, through {who}. {auth}. See docs/TCP_STDIO.md, \
                     docs/UDP_TUNNELS.md and docs/REVERSE_PORTS.md"
                ),
                &[
                    "open_sandbox_tcp_tunnel",
                    "open_sandbox_udp_tunnel",
                    "open_sandbox_udp6_tunnel",
                    "open_sandbox_reverse_tunnel",
                ],
                node,
                &["a running sandbox"],
                perms,
            ),
            capability(
                "sandbox_telemetry",
                &format!("Sandbox Metrics And Logs ({suffix})"),
                &format!(
                    "Read a sandbox's CPU, memory and disk samples (every 5 seconds, the last \
                     hour) and the node's lifecycle log of it, through {who}. {auth}. The \
                     latest-samples route is also open to an inventory key or an observer on \
                     the control plane"
                ),
                &[
                    "list_sandboxes_metrics",
                    "get_sandbox_metrics",
                    "get_sandbox_logs",
                    "get_sandbox_logs_v2",
                ],
                node,
                &[running],
                perms,
            ),
            capability(
                "sandbox_events",
                &format!("Sandbox Lifecycle Events ({suffix})"),
                &format!(
                    "List the cluster's lifecycle events of sandboxes (created, paused, \
                     resumed, updated, killed) as E2B's `SandboxEvent`, all of them or one \
                     sandbox's, through {who}. {} See docs/TEAMS.md and docs/WEBHOOKS.md",
                    if node {
                        "It checks only the cluster token, and does not filter by team."
                    } else {
                        "A key or session needs the `events` scope; a team sees its own \
                         team's events."
                    }
                ),
                &["list_sandbox_events", "get_sandbox_events"],
                node,
                &[running],
                if node {
                    &["cluster token"]
                } else {
                    &["events"]
                },
            ),
            capability(
                "registration_recovery",
                &format!("Uncertain Registration Recovery ({suffix})"),
                &format!(
                    "Find the sandboxes a node runs whose record or event could not be \
                     published to the cluster store, and publish them again, through {who}. {} \
                     See docs/NATIVE_PORT_GATEWAY.md",
                    if node {
                        "It needs the cluster token and a node in a cluster."
                    } else {
                        "It needs an administrator credential and a control plane with a \
                         cluster token."
                    }
                ),
                if node {
                    &[
                        "list_pending_registrations",
                        "reconcile_sandbox_registration",
                    ]
                } else {
                    &[
                        "list_node_pending_registrations",
                        "reconcile_node_sandbox_registration",
                    ]
                },
                node,
                &[if node {
                    "a node in a cluster with a cluster token"
                } else {
                    "an administrator key and a registered node"
                }],
                if node { &["cluster token"] } else { &["admin"] },
            ),
        ];
        if node {
            capabilities.push(capability(
                "sandbox_private_networks",
                "Private Network Connections (node daemon)",
                "Receive the connections other sandboxes of the same owner make to a sandbox \
                 over a private network, as another node (or this one) opens them with the \
                 cluster's mutual TLS. Not for a caller: the control plane does not route \
                 them. The tags are set on the control plane. See docs/PRIVATE_VM_NETWORKS.md",
                &[
                    "open_sandbox_private_tcp_tunnel",
                    "open_sandbox_private_udp_tunnel",
                ],
                true,
                &["a node with a cluster token and complete mutual TLS"],
                &["cluster token"],
            ));
            capabilities.push(capability(
                "local_networks",
                "Local Networks (node daemon)",
                "Create named networks on one node that join its sandboxes to each other by \
                 DNS alias, as `docker network` does, and connect and disconnect sandboxes. \
                 Held in the node's memory, with no owner or team, and not routed by the \
                 control plane. See crates/hv2-sandboxd/src/local_network.rs",
                &[
                    "list_local_networks",
                    "create_local_network",
                    "get_local_network",
                    "delete_local_network",
                    "connect_sandbox_to_local_network",
                    "disconnect_sandbox_from_local_network",
                ],
                true,
                &["a node"],
                &["cluster token"],
            ));
        } else {
            capabilities.push(capability(
                "sandbox_public_ports",
                "Sandbox Public Ports (cluster control plane)",
                "Reserve, list and remove stable public TCP or UDP ports for a sandbox's \
                 guest ports, which the native gateway then listens on. Needs a control \
                 plane started with `--native-port-range`, and a key with a configured \
                 creator (`principal_id`) that owns the sandbox. See \
                 docs/NATIVE_PORT_GATEWAY.md",
                &[
                    "list_sandbox_public_ports",
                    "reserve_sandbox_public_port",
                    "remove_sandbox_public_port",
                ],
                false,
                &["a control plane with --native-port-range and a running hv2-native-gateway"],
                &["sandboxes", "a key with a principal_id"],
            ));
            capabilities.push(capability(
                "sandbox_web_sharing",
                "Sandbox Web Sharing (cluster control plane)",
                "Read and replace the list of browser identities the sandbox's owner has \
                 granted access to its guest URLs, with an expiry each. Needs a key with a \
                 configured creator (`principal_id`) that owns the sandbox. See \
                 docs/PRIVATE_GUEST_URLS.md",
                &["get_sandbox_web_sharing", "set_sandbox_web_sharing"],
                false,
                &["browser login configured (--web-access-file or --sso-guest-urls)"],
                &["sandboxes", "a key with a principal_id"],
            ));
            capabilities.push(capability(
                "sandbox_domains_and_names",
                "Sandbox Custom Domains And Names (cluster control plane)",
                "Bind an operator-managed hostname to a sandbox's HTTP port (with an optional \
                 DNS ownership proof), list and unbind them, and hold a cluster-wide name for \
                 a sandbox and look it up. Needs the `sandboxes` scope. See \
                 docs/CUSTOM_DOMAINS.md",
                &[
                    "list_sandbox_domains",
                    "get_sandbox_domain_challenge",
                    "bind_sandbox_domain",
                    "unbind_sandbox_domain",
                    "assign_sandbox_name",
                    "resolve_sandbox_name",
                ],
                false,
                &["DNS and a certificate for the hostname, which are the operator's"],
                &["sandboxes"],
            ));
            capabilities.push(capability(
                "sandbox_private_networks",
                "Sandbox Private Network Tags (cluster control plane)",
                "Read and replace the tags that put a sandbox in private networks with the \
                 other sandboxes of its owner that share a tag. Needs a key with a configured \
                 creator (`principal_id`) that owns the sandbox. See \
                 docs/PRIVATE_VM_NETWORKS.md",
                &[
                    "get_sandbox_private_networks",
                    "set_sandbox_private_networks",
                ],
                false,
                &["nodes with mutual TLS, a cluster token and guest networking"],
                &["sandboxes", "a key with a principal_id"],
            ));
            capabilities.push(capability(
                "sandbox_refreshes",
                "Sandbox Refresh (cluster control plane)",
                "E2B's refresh route, which the control plane routes and no node serves: it \
                 always ends in 404. Use `set_sandbox_timeout`.",
                &["refresh_sandbox"],
                false,
                &[],
                &["sandboxes"],
            ));
        }
        capabilities
    }
}
