//! Many hosts, one sandbox API.
//!
//! Phase 4 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`: CubeMaster's shape. Node
//! daemons (`hv2-sandboxd`) run the VMs and record what they run in a shared
//! [`store`]; any number of stateless control planes (`hv2-control-plane`)
//! serve E2B's API for the whole cluster from that store, [`scheduler`]
//! creations across nodes, forward per-sandbox calls to the owning node, and
//! route envd traffic there.

pub mod allowlist;
pub mod audit;
pub mod control;
pub mod domain_verification;
pub mod domains;
pub mod environments;
pub mod events;
pub mod keys;
pub mod metrics;
pub mod model;
pub mod mtls;
pub mod names;
pub mod native_budget;
pub mod native_gateway;
pub mod native_node;
pub mod native_ports;
pub mod native_tcp;
pub mod native_udp;
pub mod node;
pub mod otlp;
pub mod ports;
pub mod private_addresses;
pub mod private_networks;
pub mod private_node;
pub mod private_router;
pub mod scheduler;
pub mod sso;
pub mod sso_guest;
pub mod sso_login;
pub mod store;
pub mod team_telemetry;
pub mod udp_socket;
pub mod usage;
pub mod web_access;
pub mod web_sharing;

pub mod ownership;

#[cfg(test)]
pub(crate) mod test_ports {
    use std::net::{IpAddr, SocketAddr};

    /// A port on `ip` that is bindable for both TCP and UDP right now.
    ///
    /// Windows holds back different port ranges per protocol, so a port the
    /// TCP ephemeral allocator hands out can be forbidden for UDP (WSAEACCES)
    /// and the reverse. Tests that need one port for both must probe both.
    pub(crate) async fn dual_port(ip: IpAddr) -> u16 {
        let mut last = String::new();
        for _ in 0..100 {
            let tcp = match tokio::net::TcpListener::bind(SocketAddr::new(ip, 0)).await {
                Ok(tcp) => tcp,
                Err(error) => {
                    last = format!("TCP: {error}");
                    continue;
                }
            };
            let port = tcp.local_addr().unwrap().port();
            match crate::udp_socket::bind(SocketAddr::new(ip, port)).await {
                Ok(udp) => {
                    drop(udp);
                    drop(tcp);
                    return port;
                }
                Err(error) => last = format!("UDP {ip}:{port}: {error}"),
            }
        }
        panic!("no port on {ip} was bindable for both TCP and UDP in 100 tries; last: {last}");
    }
}
