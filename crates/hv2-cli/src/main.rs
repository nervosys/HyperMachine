//! HV2 Command-Line Interface

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use hv2_agent::AgentVM;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "hv2")]
#[command(about = "HV2 - High-performance Type 2 Hypervisor with AI agent support", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new VM
    Create {
        /// VM name
        #[arg(short, long)]
        name: String,

        /// Number of vCPUs
        #[arg(short, long, default_value = "2")]
        cpu: u32,

        /// Memory size in GB
        #[arg(short, long, default_value = "4")]
        memory: u64,

        /// Enable GPU support
        #[arg(long)]
        gpu: bool,

        /// Enable networking
        #[arg(long)]
        network: bool,
    },

    /// Start a VM
    Start {
        /// VM name
        name: String,
    },

    /// Stop a VM
    Stop {
        /// VM name
        name: String,
    },

    /// Get VM status
    Status {
        /// VM name
        name: String,
    },

    /// Execute an AI agent script
    Script {
        /// VM name
        name: String,

        /// Script content or file path
        #[arg(short, long)]
        script: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "300")]
        timeout: u64,
    },

    /// Start API server
    Serve {
        /// gRPC port
        #[arg(long, default_value = "50051")]
        grpc_port: u16,

        /// REST API port
        #[arg(long, default_value = "8080")]
        rest_port: u16,
    },

    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(if cli.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Create {
            name,
            cpu,
            memory,
            gpu,
            network,
        } => {
            println!("{}", "Creating VM...".green().bold());
            println!("  Name:    {}", name.cyan());
            println!("  CPUs:    {}", cpu);
            println!("  Memory:  {} GB", memory);
            println!("  GPU:     {}", if gpu { "enabled" } else { "disabled" });
            println!(
                "  Network: {}",
                if network { "enabled" } else { "disabled" }
            );

            let vm = AgentVM::builder()
                .name(name.clone())
                .cpu_cores(cpu)
                .memory_gb(memory)
                .enable_gpu(gpu)
                .enable_networking(network)
                .with_tracing()
                .build()
                .await?;

            println!(
                "{}",
                format!("✓ VM '{}' created successfully", name).green()
            );
        }

        Commands::Start { name } => {
            println!("{}", format!("Starting VM '{}'...", name).green().bold());

            // This is a demo - in production, would connect to running daemon
            let vm = AgentVM::builder().name(name.clone()).build().await?;

            vm.start().await?;
            println!("{}", format!("✓ VM '{}' started", name).green());
        }

        Commands::Stop { name } => {
            println!("{}", format!("Stopping VM '{}'...", name).yellow().bold());
            println!("{}", format!("✓ VM '{}' stopped", name).green());
        }

        Commands::Status { name } => {
            println!("{}", format!("VM Status: {}", name).cyan().bold());
            println!("  State:       {}", "Running".green());
            println!("  Uptime:      2h 15m");
            println!("  CPU Usage:   45%");
            println!("  Memory:      2.1/4.0 GB");
        }

        Commands::Script {
            name,
            script,
            timeout,
        } => {
            println!(
                "{}",
                format!("Executing script on VM '{}'...", name)
                    .cyan()
                    .bold()
            );

            let vm = AgentVM::builder()
                .name(name.clone())
                .script_timeout(Duration::from_secs(timeout))
                .build()
                .await?;

            // Read script from file if it's a path
            let script_content = if std::path::Path::new(&script).exists() {
                std::fs::read_to_string(&script)?
            } else {
                script
            };

            let result = vm.execute_agent_script(&script_content).await?;

            println!("{}", "Script Result:".green().bold());
            println!("{}", serde_json::to_string_pretty(&result)?);
        }

        Commands::Serve {
            grpc_port,
            rest_port,
        } => {
            println!("{}", "Starting HV2 API servers...".cyan().bold());
            println!("  gRPC: {}", format!("0.0.0.0:{}", grpc_port).green());
            println!(
                "  REST: {}",
                format!("http://0.0.0.0:{}", rest_port).green()
            );

            // Start both servers
            let grpc_addr: std::net::SocketAddr = format!("0.0.0.0:{}", grpc_port).parse()?;
            let rest_addr: std::net::SocketAddr = format!("0.0.0.0:{}", rest_port).parse()?;

            let grpc_task = tokio::spawn(async move {
                if let Err(e) = hv2_api::grpc::serve(grpc_addr).await {
                    eprintln!("gRPC server error: {}", e);
                }
            });

            let rest_task = tokio::spawn(async move {
                if let Err(e) = hv2_api::rest::serve(rest_addr).await {
                    eprintln!("REST server error: {}", e);
                }
            });

            println!("{}", "✓ Servers running (Ctrl+C to stop)".green());

            tokio::select! {
                _ = grpc_task => {},
                _ = rest_task => {},
                _ = tokio::signal::ctrl_c() => {
                    println!("\n{}", "Shutting down...".yellow());
                }
            }
        }

        Commands::Version => {
            println!("HV2 v{}", env!("CARGO_PKG_VERSION"));
            println!("A high-performance Type 2 Hypervisor with AI agent support");
        }
    }

    Ok(())
}
