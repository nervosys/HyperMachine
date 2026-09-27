//! A stateless control plane for a cluster of `hv2-sandboxd` nodes.
//!
//! ```text
//! hv2-control-plane --store redis://127.0.0.1:6379 --port 5980 --proxy-port 5981 \
//!     --api-key "$E2B_API_KEY" --cluster-token "$HV2_CLUSTER_TOKEN"
//! ```
//!
//! Run as many as you like against one store. Point the E2B SDK at any of
//! them: `E2B_API_URL=http://host:5980 E2B_SANDBOX_URL=http://host:5981`.

use std::sync::Arc;
use std::time::Duration;

use hv2_cluster::control::{self, ClusterRoutes, ControlConfig, ControlPlane};
use hv2_cluster::store;

struct Options {
    store: String,
    namespace: String,
    port: u16,
    proxy_port: u16,
    api_key: Option<String>,
    cluster_token: Option<String>,
    reap_interval: Duration,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

fn parse() -> Result<Options, String> {
    let mut opts = Options {
        store: "memory:".to_string(),
        namespace: "default".to_string(),
        port: 5980,
        proxy_port: 5981,
        // From the environment by default, so a key need not sit in `ps`.
        api_key: std::env::var("HV2_API_KEY").ok().filter(|k| !k.is_empty()),
        cluster_token: std::env::var("HV2_CLUSTER_TOKEN")
            .ok()
            .filter(|k| !k.is_empty()),
        reap_interval: Duration::from_secs(5),
        tls_cert: None,
        tls_key: None,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let mut value = || {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--store" => opts.store = value()?,
            "--namespace" => opts.namespace = value()?,
            "--port" => opts.port = value()?.parse().map_err(|e| format!("--port: {e}"))?,
            "--proxy-port" => {
                opts.proxy_port = value()?.parse().map_err(|e| format!("--proxy-port: {e}"))?;
            }
            "--api-key" => opts.api_key = Some(value()?),
            "--cluster-token" => opts.cluster_token = Some(value()?),
            "--reap-interval" => {
                opts.reap_interval = Duration::from_secs(
                    value()?
                        .parse()
                        .map_err(|e| format!("--reap-interval: {e}"))?,
                );
            }
            "--tls-cert" => opts.tls_cert = Some(value()?),
            "--tls-key" => opts.tls_key = Some(value()?),
            "--help" | "-h" => {
                println!(
                    "usage: hv2-control-plane [--store memory:|redis://host:port] [--namespace N] \
                     [--port N] [--proxy-port N] [--api-key K] [--cluster-token T] \
                     [--reap-interval SECS] [--tls-cert F --tls-key F]\n\
                     HV2_API_KEY and HV2_CLUSTER_TOKEN are read from the environment too."
                );
                std::process::exit(0);
            }
            other => return Err(format!("unrecognised argument {other}")),
        }
        i += 1;
    }
    Ok(opts)
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let opts = match parse() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("hv2-control-plane: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let store = match store::open(&opts.store, &opts.namespace).await {
        Ok(store) => store,
        Err(e) => {
            eprintln!("hv2-control-plane: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if opts.api_key.is_none() {
        tracing::warn!("no --api-key: anyone who can reach this port can create sandboxes");
    }

    // Envd traffic, routed to whichever node holds the sandbox.
    let routes = Arc::new(ClusterRoutes::new(
        Arc::clone(&store),
        Duration::from_secs(2),
    ));
    let proxy_addr = std::net::SocketAddr::from(([0, 0, 0, 0], opts.proxy_port));
    let tls = match (&opts.tls_cert, &opts.tls_key) {
        (Some(cert), Some(key)) => match hv2_api::sandbox_proxy::tls_config(
            std::path::Path::new(cert),
            std::path::Path::new(key),
        ) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("hv2-control-plane: TLS: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
        (None, None) => None,
        _ => {
            eprintln!("hv2-control-plane: --tls-cert and --tls-key go together");
            return std::process::ExitCode::FAILURE;
        }
    };
    let (_proxy_shutdown, proxy_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = match tls {
            Some(config) => {
                hv2_api::sandbox_proxy::serve_tls(proxy_addr, routes, config, proxy_rx).await
            }
            None => hv2_api::sandbox_proxy::serve(proxy_addr, routes, proxy_rx).await,
        };
        if let Err(e) = result {
            tracing::error!("envd proxy on {proxy_addr} stopped: {e}");
        }
    });

    let control = ControlPlane::new(
        store,
        ControlConfig {
            api_key: opts.api_key,
            cluster_token: opts.cluster_token,
            proxy_port: opts.proxy_port,
            create_timeout: Duration::from_secs(60),
        },
    );
    tokio::spawn(control::reaper(Arc::clone(&control), opts.reap_interval));
    let addr = format!("0.0.0.0:{}", opts.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("hv2-control-plane: could not bind {addr}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!(
        "hv2-control-plane: E2B API on {addr}, envd proxy on {proxy_addr}, store {}",
        opts.store
    );
    if let Err(e) = axum::serve(listener, control::router(control)).await {
        eprintln!("hv2-control-plane: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
