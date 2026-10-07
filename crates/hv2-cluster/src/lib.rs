//! Many hosts, one sandbox API.
//!
//! Phase 4 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`: CubeMaster's shape. Node
//! daemons (`hv2-sandboxd`) run the VMs and record what they run in a shared
//! [`store`]; any number of stateless control planes (`hv2-control-plane`)
//! serve E2B's API for the whole cluster from that store, [`scheduler`]
//! creations across nodes, forward per-sandbox calls to the owning node, and
//! route envd traffic there.

pub mod audit;
pub mod control;
pub mod domain_verification;
pub mod domains;
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
pub mod ports;
pub mod private_addresses;
pub mod private_networks;
pub mod private_node;
pub mod private_router;
pub mod scheduler;
pub mod sso;
pub mod store;
pub mod udp_socket;
pub mod web_access;
pub mod web_sharing;

pub mod ownership;
