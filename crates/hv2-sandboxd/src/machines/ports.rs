//! Published ports: a way in to a machine from the network.
//!
//! A machine's guest listens on a port; publishing it gives that port one of
//! the node's. A connection to the node's port is carried to the guest's.
//!
//! - `POST   /machines/{name}/ports` `{"guestPort", "hostPort"}` publishes
//!   one. Without `hostPort` the node chooses.
//! - `GET    /machines/{name}/ports` lists them.
//! - `DELETE /machines/{name}/ports/{hostPort}` takes one back.
//!
//! # How a connection gets in
//!
//! Through the guest agent, over the channel `exec` uses: the agent connects
//! to the port on the guest's loopback and copies bytes both ways. The
//! machine needs no NIC for it, and one that has a NIC is reached without
//! its egress rules being opened to anything. It does need the agent.
//!
//! # What an operator decides
//!
//! Which of the node's ports may be given out, and on which of its addresses
//! they listen: `--machine-ports FIRST-LAST` and `--machine-port-address`.
//! A caller chooses a port inside that range and nothing else. The address
//! is the node's loopback unless the operator says otherwise, so publishing
//! a port does not by itself put a guest on the Internet.
//!
//! # A published port and a stopped machine
//!
//! The node listens for as long as the port is published, whether the
//! machine is running or not. A connection made while it is not, or to a
//! guest port nothing listens on, is accepted and closed. The port is kept
//! with the machine and is there again when the node restarts.
//!
//! This is TCP. Nothing here is TLS or HTTP: the bytes are the guest's.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use axum::http::HeaderMap;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::task::AbortHandle;

use super::super::{
    api_error, forwards, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State,
    StatusCode,
};
use super::{all, live, named, save, Stored, OPS};

/// How long the guest has to answer that its port is open.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// The most ports one machine may have published.
pub(crate) const MOST: usize = 16;

/// One published port, as a machine's record keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Published {
    #[serde(rename = "hostPort")]
    host: u16,
    #[serde(rename = "guestPort")]
    guest: u16,
    /// Reachable through the node's sandbox proxy too, at
    /// `{guestPort}-{machineID}.{domain}`, as a sandbox's ports are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    proxy: bool,
}

/// The listeners open on this node: machine ID, then the node's port.
static LISTENERS: StdMutex<Option<HashMap<String, HashMap<u16, AbortHandle>>>> =
    StdMutex::new(None);

fn listeners<T>(f: impl FnOnce(&mut HashMap<String, HashMap<u16, AbortHandle>>) -> T) -> T {
    let mut guard = LISTENERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// One connection: into the guest through its agent, and back.
async fn carry(machine_id: &str, guest: u16, tcp: tokio::net::TcpStream) {
    // Whatever machine is running under this ID now: it may have been
    // stopped and started since the listener was made.
    let Some(vm) = live(|l| {
        l.get(machine_id)
            .filter(|m| m.agent)
            .map(|m| Arc::clone(&m.vm))
    }) else {
        return;
    };
    let (stream, early) = match vm.forward_port(guest, OPEN_TIMEOUT).await {
        Ok(opened) => opened,
        Err(e) => {
            tracing::debug!("machine {machine_id}: guest port {guest}: {e}");
            return;
        }
    };
    let Ok(tcp) = tcp.into_std() else {
        stream.close();
        return;
    };
    if tcp.set_nonblocking(false).is_err() {
        stream.close();
        return;
    }
    let _ = tcp.set_nodelay(true);
    let _ = tokio::task::spawn_blocking(move || forwards::splice(&stream, tcp, &early)).await;
}

/// Listen on `address` for `published`, carrying what connects to the
/// machine `machine_id`.
async fn listen(address: IpAddr, machine_id: &str, published: Published) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(address, published.host)).await?;
    let id = machine_id.to_string();
    let accepting = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let id = id.clone();
            tokio::spawn(async move { carry(&id, published.guest, tcp).await });
        }
    });
    let replaced = listeners(|all| {
        all.entry(machine_id.to_string())
            .or_default()
            .insert(published.host, accepting.abort_handle())
    });
    if let Some(old) = replaced {
        old.abort();
    }
    Ok(())
}

/// Listen again for every machine's published ports, when the node starts.
pub(crate) async fn restore(state: &AppState) {
    let address = state.opts.machine_port_address;
    for machine in all(state) {
        for published in &machine.ports {
            if let Err(e) = listen(address, &machine.id, *published).await {
                tracing::warn!(
                    "machine {}: port {} is published and could not be listened on: {e}",
                    machine.name,
                    published.host
                );
            }
        }
    }
}

/// Stop listening for a machine, when it is deleted.
pub(crate) fn forget(machine_id: &str) {
    if let Some(open) = listeners(|all| all.remove(machine_id)) {
        for handle in open.into_values() {
            handle.abort();
        }
    }
}

/// A published port as it is described.
fn describe(state: &AppState, machine: &Stored, published: Published) -> Value {
    let listening = listeners(|all| {
        all.get(&machine.id)
            .is_some_and(|open| open.contains_key(&published.host))
    });
    json!({
        "hostPort": published.host,
        "guestPort": published.guest,
        "address": state.opts.machine_port_address,
        // False when the node could not take the port back after a restart.
        "listening": listening,
        "proxy": published.proxy,
        // What the proxy is asked for: the first label of its host name.
        "proxyLabel": published.proxy.then(|| format!("{}-{}", published.guest, machine.id)),
    })
}

/// Where the node's proxy sends a request for `port` of the machine
/// `machine_id`: the published port's listener, when one was published for
/// that guest port with `proxy`. Read from the machine's record each time,
/// so a port published or taken back is routed or not at once.
pub(crate) fn proxied(state: &AppState, machine_id: &str, port: u16) -> Option<SocketAddr> {
    if !machine_id.starts_with("vm-") {
        return None;
    }
    let machine = super::load(state, machine_id)?;
    let published = machine
        .ports
        .iter()
        .find(|published| published.guest == port && published.proxy)?;
    let listening = listeners(|all| {
        all.get(&machine.id)
            .is_some_and(|open| open.contains_key(&published.host))
    });
    if !listening {
        return None;
    }
    // A listener on every address is reached on loopback.
    let address = match state.opts.machine_port_address {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        other => other,
    };
    Some(SocketAddr::new(address, published.host))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Publish {
    #[serde(rename = "guestPort")]
    guest: u16,
    /// One of the node's ports, inside the range its operator gives out;
    /// the node chooses when this is absent.
    #[serde(rename = "hostPort")]
    host: Option<u16>,
    /// Route the port through the node's sandbox proxy as well: a name
    /// there, and the proxy's TLS when it has a certificate. Off unless
    /// asked, since the proxy is commonly on a public address.
    #[serde(default)]
    proxy: bool,
}

/// Why `machine` cannot have `asked` published, when it cannot. Everything
/// that can be said without trying the port.
fn refused(machine: &Stored, asked: &Publish, range: (u16, u16)) -> Option<String> {
    if asked.guest == 0 {
        return Some("guestPort is 1 to 65535".to_string());
    }
    if machine.image.is_some() && !machine.guest_agent {
        return Some(format!(
            "machine {} was made from an image without guestAgent, and a port is carried in \
             by the guest agent",
            machine.name
        ));
    }
    if machine.ports.len() >= MOST {
        return Some(format!("a machine has at most {MOST} published ports"));
    }
    match asked.host {
        Some(host) if !(range.0..=range.1).contains(&host) => Some(format!(
            "hostPort is {} to {} on this node, not {host}",
            range.0, range.1
        )),
        _ => None,
    }
}

/// `POST /machines/{name}/ports`.
pub(crate) async fn publish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(asked): Json<Publish>,
) -> Response {
    let _ops = OPS.lock().await;
    let mut machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let range = state.opts.machine_ports;
    if let Some(why) = refused(&machine, &asked, range) {
        return api_error(StatusCode::BAD_REQUEST, why);
    }
    let address = state.opts.machine_port_address;
    // Whether a port is free is found by taking it: every published port is
    // listened on from the moment it is, so one in use is one that will not
    // bind, whoever has it.
    let candidates: Vec<u16> = match asked.host {
        Some(host) => vec![host],
        None => (range.0..=range.1).collect(),
    };
    let mut taken = None;
    for host in candidates {
        let published = Published {
            host,
            guest: asked.guest,
            proxy: asked.proxy,
        };
        match listen(address, &machine.id, published).await {
            Ok(()) => {
                taken = Some(published);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("listening on {address}:{host}: {e}"),
                )
            }
        }
    }
    let Some(published) = taken else {
        return api_error(
            StatusCode::CONFLICT,
            match asked.host {
                Some(host) => format!("port {host} of this node is in use"),
                None => format!("every port from {} to {} is in use", range.0, range.1),
            },
        );
    };
    machine.ports.push(published);
    if let Err(e) = save(&state, &machine) {
        // Not kept, so not listened on: the two never disagree.
        if let Some(handle) = listeners(|all| {
            all.get_mut(&machine.id)
                .and_then(|open| open.remove(&published.host))
        }) {
            handle.abort();
        }
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    (
        StatusCode::CREATED,
        Json(describe(&state, &machine, published)),
    )
        .into_response()
}

/// `GET /machines/{name}/ports`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    match named(&state, &headers, &name) {
        Ok(machine) => Json(
            machine
                .ports
                .iter()
                .map(|published| describe(&state, &machine, *published))
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(response) => response,
    }
}

/// `DELETE /machines/{name}/ports/{hostPort}`.
pub(crate) async fn unpublish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, host)): Path<(String, u16)>,
) -> Response {
    let _ops = OPS.lock().await;
    let mut machine = match named(&state, &headers, &name) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let before = machine.ports.len();
    machine.ports.retain(|published| published.host != host);
    if machine.ports.len() == before {
        return api_error(
            StatusCode::NOT_FOUND,
            format!("machine {name} has no published port {host}"),
        );
    }
    if let Err(e) = save(&state, &machine) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    // New connections stop here. One already carried is the guest's to end.
    if let Some(handle) =
        listeners(|all| all.get_mut(&machine.id).and_then(|open| open.remove(&host)))
    {
        handle.abort();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::super::super::machine_port_range as parse_range;
    use super::*;

    fn machine(image: Option<&str>, guest_agent: bool, ports: usize) -> Stored {
        let ports: Vec<Value> = (0..ports)
            .map(|n| json!({"hostPort": 20000 + n, "guestPort": 80}))
            .collect();
        serde_json::from_value(json!({
            "machineID": "m-1", "name": "web-01", "templateID": "base", "cpuCount": 1,
            "memoryMB": 512, "diskGiB": 8, "autostart": true, "restartPolicy": "always",
            "desired": "stopped", "createdAt": 1, "image": image, "guestAgent": guest_agent,
            "ports": ports,
        }))
        .unwrap()
    }

    fn ask(guest: u16, host: Option<u16>) -> Publish {
        Publish {
            guest,
            host,
            proxy: false,
        }
    }

    const RANGE: (u16, u16) = (20000, 20099);

    #[test]
    fn a_port_is_published_only_inside_the_range_the_operator_gives_out() {
        let template = machine(None, false, 0);
        for host in [None, Some(20000), Some(20050), Some(20099)] {
            assert_eq!(refused(&template, &ask(80, host), RANGE), None, "{host:?}");
        }
        for host in [1, 22, 80, 19999, 20100, 65535] {
            let why = refused(&template, &ask(80, Some(host)), RANGE).unwrap();
            assert!(why.contains("20000 to 20099"), "{host}: {why}");
        }
        assert!(refused(&template, &ask(0, None), RANGE)
            .unwrap()
            .contains("guestPort"));
    }

    #[test]
    fn a_machine_without_the_agent_has_no_way_in_and_is_told() {
        let why = refused(&machine(Some("noble.raw"), false, 0), &ask(80, None), RANGE).unwrap();
        assert!(why.contains("guestAgent"), "{why}");
        assert_eq!(
            refused(&machine(Some("noble.raw"), true, 0), &ask(80, None), RANGE),
            None
        );
        // No more than a machine may have.
        assert_eq!(
            refused(&machine(None, false, MOST - 1), &ask(80, None), RANGE),
            None
        );
        let why = refused(&machine(None, false, MOST), &ask(80, None), RANGE).unwrap();
        assert!(why.contains("at most"), "{why}");
    }

    #[test]
    fn a_record_without_ports_is_one_with_none_and_says_nothing_of_them() {
        let old = machine(None, false, 0);
        assert!(old.ports.is_empty());
        let written = serde_json::to_value(&old).unwrap();
        assert!(written.get("ports").is_none(), "{written}");
        let kept = serde_json::to_value(machine(None, false, 2)).unwrap();
        assert_eq!(
            kept["ports"],
            json!([{"hostPort": 20000, "guestPort": 80}, {"hostPort": 20001, "guestPort": 80}])
        );
    }

    #[test]
    fn a_range_is_two_ports_in_order() {
        assert_eq!(parse_range("20000-29999"), Ok((20000, 29999)));
        assert_eq!(parse_range("8080-8080"), Ok((8080, 8080)));
        assert_eq!(parse_range(" 1 - 65535 "), Ok((1, 65535)));
        for bad in ["", "8080", "0-10", "10-0", "9-8", "a-b", "1-65536", "1-2-3"] {
            assert!(parse_range(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_connection_to_a_machine_that_is_not_running_is_closed() {
        use tokio::io::AsyncReadExt;
        // A port of this test's own, then freed for the listener.
        let host = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().port()
        };
        let published = Published {
            host,
            guest: 80,
            proxy: false,
        };
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        listen(loopback, "ports-test-machine", published)
            .await
            .unwrap();
        // The port is taken for as long as it is published.
        let again = listen(loopback, "ports-test-other", published).await;
        assert_eq!(again.unwrap_err().kind(), std::io::ErrorKind::AddrInUse);
        let mut tcp = tokio::net::TcpStream::connect((loopback, host))
            .await
            .unwrap();
        let mut buf = [0u8; 8];
        let read = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut buf)).await;
        assert!(matches!(read, Ok(Ok(0)) | Ok(Err(_))), "{read:?}");
        // Forgotten, the port is free again.
        forget("ports-test-machine");
        let mut freed = false;
        for _ in 0..50 {
            if std::net::TcpListener::bind((loopback, host)).is_ok() {
                freed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(freed, "the port was not given back");
    }
}
