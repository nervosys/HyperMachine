//! Operator-run native TCP/UDP gateway. Credentials are environment-only.
use hv2_cluster::{
    mtls::{Mtls, DEFAULT_NODE_NAME},
    native_budget::NativeSessionBudget,
    native_gateway::NativeGateway,
    native_node::NativeNodeConnector,
    native_tcp::TcpRelayLimits,
    native_udp::UdpRelayLimits,
    store::RedisStore,
};
use std::{net::IpAddr, path::PathBuf, sync::Arc, time::Duration};
const HELP: &str = "usage: hv2-native-gateway --bind-ip IP --mtls-ca F --mtls-cert F --mtls-key F
    [--namespace N] [--mtls-node-name N] [--max-ports N] [--max-sessions N]
    [--tcp-connections N] [--udp-peers N] [--poll-ms N] [--tcp-age-secs N]
    [--udp-idle-secs N] [--check]
HV2_STORE_URL and HV2_CLUSTER_TOKEN are required environment variables.
The gateway consumes existing authorized reservations; it does not create them.";
struct Options {
    bind: IpAddr,
    namespace: String,
    store: String,
    token: String,
    ca: PathBuf,
    cert: PathBuf,
    key: PathBuf,
    node_name: String,
    max_ports: usize,
    max_sessions: usize,
    tcp_connections: usize,
    udp_peers: usize,
    poll: Duration,
    tcp_age: Duration,
    udp_idle: Duration,
    check: bool,
}
fn parse(
    args: impl IntoIterator<Item = String>,
    store: Option<String>,
    token: Option<String>,
) -> Result<Option<Options>, String> {
    let mut args = args.into_iter();
    let mut bind = None;
    let mut ca = None;
    let mut cert = None;
    let mut key = None;
    let mut namespace = "default".to_owned();
    let mut node_name = DEFAULT_NODE_NAME.to_owned();
    let mut max_ports = 256;
    let mut max_sessions = 128;
    let mut tcp_connections = 64;
    let mut udp_peers = 64;
    let mut poll = 250;
    let mut tcp_age = 3600;
    let mut udp_idle = 30;
    let mut check = false;
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--help" | "-h") {
            return Ok(None);
        }
        if flag == "--check" {
            check = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--bind-ip"
                | "--namespace"
                | "--mtls-ca"
                | "--mtls-cert"
                | "--mtls-key"
                | "--mtls-node-name"
                | "--max-ports"
                | "--max-sessions"
                | "--tcp-connections"
                | "--udp-peers"
                | "--poll-ms"
                | "--tcp-age-secs"
                | "--udp-idle-secs"
        ) {
            return Err("unknown gateway option; see --help".into());
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let number = || {
            value
                .parse::<usize>()
                .map_err(|_| format!("invalid numeric value for {flag}"))
        };
        match flag.as_str() {
            "--bind-ip" => bind = Some(value.parse::<IpAddr>().map_err(|_| "invalid --bind-ip")?),
            "--namespace" => namespace = value,
            "--mtls-ca" => ca = Some(PathBuf::from(value)),
            "--mtls-cert" => cert = Some(PathBuf::from(value)),
            "--mtls-key" => key = Some(PathBuf::from(value)),
            "--mtls-node-name" => node_name = value,
            "--max-ports" => max_ports = number()?,
            "--max-sessions" => max_sessions = number()?,
            "--tcp-connections" => tcp_connections = number()?,
            "--udp-peers" => udp_peers = number()?,
            "--poll-ms" => poll = number()?,
            "--tcp-age-secs" => tcp_age = number()?,
            "--udp-idle-secs" => udp_idle = number()?,
            _ => unreachable!(),
        }
    }
    let store = store
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or("HV2_STORE_URL is required")?;
    if !["redis://", "rediss://", "redis+unix://"]
        .iter()
        .any(|scheme| store.starts_with(scheme))
    {
        return Err("HV2_STORE_URL must use Redis or Redis TLS".into());
    }
    redis::Client::open(store.as_str()).map_err(|_| "invalid HV2_STORE_URL")?;
    let token = token
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
        .ok_or("HV2_CLUSTER_TOKEN is required")?;
    if namespace.is_empty()
        || namespace.len() > 64
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("invalid gateway namespace".into());
    }
    if !(1..=4096).contains(&max_ports) || !(20..=60000).contains(&poll) {
        return Err("gateway port/poll limit out of range".into());
    }
    NativeSessionBudget::new(max_sessions).map_err(|_| "gateway session limit out of range")?;
    let tcp_age = Duration::from_secs(tcp_age as u64);
    let udp_idle = Duration::from_secs(udp_idle as u64);
    TcpRelayLimits::new(tcp_connections, Duration::from_secs(5), tcp_age)
        .map_err(|_| "gateway TCP limits out of range")?;
    UdpRelayLimits::new(
        udp_peers,
        Duration::from_secs(5),
        udp_idle,
        Duration::from_secs(5),
    )
    .map_err(|_| "gateway UDP limits out of range")?;
    Ok(Some(Options {
        bind: bind.ok_or("--bind-ip is required")?,
        namespace,
        store,
        token,
        ca: ca.ok_or("--mtls-ca is required")?,
        cert: cert.ok_or("--mtls-cert is required")?,
        key: key.ok_or("--mtls-key is required")?,
        node_name,
        max_ports,
        max_sessions,
        tcp_connections,
        udp_peers,
        poll: Duration::from_millis(poll as u64),
        tcp_age,
        udp_idle,
        check,
    }))
}
async fn start(options: Options) -> Result<(), &'static str> {
    let tls = Mtls::load(&options.ca, &options.cert, &options.key, &options.node_name)
        .map_err(|_| "could not load gateway mutual TLS identity")?;
    // Validate the environment credential even in offline configuration checks.
    let empty = Arc::new(hv2_cluster::store::MemoryStore::new());
    NativeNodeConnector::new(empty, &tls, &options.token)
        .map_err(|_| "invalid gateway node credential/configuration")?;
    if options.check {
        println!("native gateway configuration valid (store not contacted)");
        return Ok(());
    }
    let store = tokio::time::timeout(
        Duration::from_secs(5),
        RedisStore::connect(&options.store, &options.namespace),
    )
    .await
    .map_err(|_| "gateway store connection timed out")?
    .map_err(|_| "could not connect to gateway store")?;
    let store = Arc::new(store);
    let connector = Arc::new(
        NativeNodeConnector::new(store.clone(), &tls, &options.token)
            .map_err(|_| "invalid gateway node configuration")?,
    );
    let mut gateway = NativeGateway::new(
        store,
        options.bind,
        options.max_ports,
        connector.clone(),
        connector,
        NativeSessionBudget::new(options.max_sessions)
            .map_err(|_| "invalid gateway session budget")?,
        TcpRelayLimits::new(
            options.tcp_connections,
            Duration::from_secs(5),
            options.tcp_age,
        )
        .map_err(|_| "invalid gateway TCP limits")?,
        UdpRelayLimits::new(
            options.udp_peers,
            Duration::from_secs(5),
            options.udp_idle,
            Duration::from_secs(5),
        )
        .map_err(|_| "invalid gateway UDP limits")?,
    )
    .map_err(|_| "invalid gateway configuration")?;
    #[cfg(unix)]
    let shutdown = {
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .map_err(|_| "could not register gateway shutdown signal")?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| "could not register gateway shutdown signal")?;
        async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        }
    };
    #[cfg(not(unix))]
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    println!("native gateway started");
    gateway
        .run(options.poll, shutdown)
        .await
        .map_err(|_| "gateway supervisor failed")
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let options = match parse(
        std::env::args().skip(1),
        std::env::var("HV2_STORE_URL").ok(),
        std::env::var("HV2_CLUSTER_TOKEN").ok(),
    ) {
        Ok(None) => {
            println!("{HELP}");
            return std::process::ExitCode::SUCCESS;
        }
        Ok(Some(options)) => options,
        Err(error) => {
            eprintln!("hv2-native-gateway: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    match start(options).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hv2-native-gateway: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(extra: &[&str]) -> Vec<String> {
        [
            "--bind-ip",
            "127.0.0.1",
            "--mtls-ca",
            "ca.pem",
            "--mtls-cert",
            "client.pem",
            "--mtls-key",
            "client.key",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn startup_requires_explicit_bind_store_identity_and_bounded_limits() {
        assert!(parse(vec!["--help".into()], None, None).unwrap().is_none());
        assert!(parse(args(&[]), None, Some("owned-token".into())).is_err());
        assert!(parse(
            args(&[]),
            Some("memory:".into()),
            Some("owned-token".into())
        )
        .is_err());
        for extra in [
            ["--max-sessions", "0"],
            ["--max-ports", "4097"],
            ["--udp-peers", "1025"],
            ["--poll-ms", "19"],
            ["--tcp-age-secs", "0"],
            ["--udp-idle-secs", "3601"],
        ] {
            assert!(parse(
                args(&extra),
                Some("redis+unix:///owned.sock".into()),
                Some("owned-token".into())
            )
            .is_err());
        }
        let options = parse(
            args(&["--namespace", "owned-test", "--check"]),
            Some("redis+unix:///owned.sock".into()),
            Some("owned-token".into()),
        )
        .unwrap()
        .unwrap();
        assert!(options.check);
        assert_eq!(options.namespace, "owned-test");
        assert_eq!(options.max_sessions, 128);
    }
}
