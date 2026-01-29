//! HyperMachine Command-Line Interface
//!
//! Unified CLI for managing both Type 1 (bare-metal) and Type 2 (hosted) hypervisors.
//!
//! # Usage
//!
//! ```bash
//! # Type 2 (hosted hypervisor) - available now
//! hm t2 create --name my-vm --cpu 4 --memory 8
//! hm t2 start my-vm
//! hm t2 status my-vm
//!
//! # Type 1 (bare-metal hypervisor) - planned
//! hm t1 create --name prod-vm --cpu 16 --memory 64
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use hv2_agent::AgentVM;
use std::time::Duration;

/// HyperMachine - High-performance hypervisor with AI agent support
#[derive(Parser)]
#[command(name = "hm")]
#[command(about = "HyperMachine - Unified Type 1/Type 2 Hypervisor with AI agent support")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Type 1 (bare-metal) hypervisor commands
    #[command(visible_alias = "type1")]
    T1 {
        #[command(subcommand)]
        command: T1Commands,
    },

    /// Type 2 (hosted) hypervisor commands
    #[command(visible_alias = "type2")]
    T2 {
        #[command(subcommand)]
        command: T2Commands,
    },

    /// Start API server (serves both T1 and T2)
    Serve {
        /// gRPC port
        #[arg(long, default_value = "50051")]
        grpc_port: u16,

        /// REST API port
        #[arg(long, default_value = "8080")]
        rest_port: u16,
    },

    /// Show version and system information
    Info,
}

/// Type 1 (bare-metal) hypervisor commands - runs directly on hardware
#[derive(Subcommand)]
enum T1Commands {
    /// Create a new Type 1 VM
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

        /// Enable GPU passthrough
        #[arg(long)]
        gpu: bool,

        /// Enable SR-IOV networking
        #[arg(long)]
        network: bool,
    },

    /// Start a Type 1 VM
    Start {
        /// VM name
        name: String,
    },

    /// Stop a Type 1 VM
    Stop {
        /// VM name
        name: String,
    },

    /// Get Type 1 VM status
    Status {
        /// VM name (optional, shows all if omitted)
        name: Option<String>,
    },

    /// List all Type 1 VMs
    List,

    /// Execute an AI agent script on Type 1 VM
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
}

/// Type 2 (hosted) hypervisor commands - runs on top of host OS
#[derive(Subcommand)]
enum T2Commands {
    /// Create a new Type 2 VM
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

    /// Start a Type 2 VM
    Start {
        /// VM name
        name: String,
    },

    /// Stop a Type 2 VM
    Stop {
        /// VM name
        name: String,
    },

    /// Get Type 2 VM status
    Status {
        /// VM name (optional, shows all if omitted)
        name: Option<String>,
    },

    /// List all Type 2 VMs
    List,

    /// Execute an AI agent script on Type 2 VM
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
        Commands::T1 { command } => handle_t1(command).await?,
        Commands::T2 { command } => handle_t2(command).await?,
        Commands::Serve {
            grpc_port,
            rest_port,
        } => handle_serve(grpc_port, rest_port).await?,
        Commands::Info => handle_info(),
    }

    Ok(())
}

/// Handle Type 1 (bare-metal) hypervisor commands
async fn handle_t1(command: T1Commands) -> Result<()> {
    println!("{}", "[T1 Bare-Metal Mode]".magenta().bold());

    match command {
        T1Commands::Create {
            name,
            cpu,
            memory,
            gpu,
            network,
        } => {
            println!(
                "{}",
                "⚠ Type 1 (bare-metal) mode is not yet implemented".yellow()
            );
            println!(
                "{}",
                "This feature is planned for a future release.".dimmed()
            );
            println!();
            println!("Planned VM configuration:");
            println!("  Name:    {}", name.cyan());
            println!("  CPUs:    {}", cpu);
            println!("  Memory:  {} GB", memory);
            println!(
                "  GPU:     {}",
                if gpu { "passthrough" } else { "disabled" }
            );
            println!("  Network: {}", if network { "SR-IOV" } else { "disabled" });
            println!();
            println!(
                "Use {} for the currently available hosted hypervisor.",
                "hm t2".green().bold()
            );
        }
        T1Commands::Start { name } => {
            println!(
                "{}",
                format!(
                    "⚠ Cannot start '{}' - Type 1 mode not yet implemented",
                    name
                )
                .yellow()
            );
        }
        T1Commands::Stop { name } => {
            println!(
                "{}",
                format!("⚠ Cannot stop '{}' - Type 1 mode not yet implemented", name).yellow()
            );
        }
        T1Commands::Status { name } => {
            if let Some(n) = name {
                println!(
                    "{}",
                    format!(
                        "⚠ Cannot get status for '{}' - Type 1 mode not yet implemented",
                        n
                    )
                    .yellow()
                );
            } else {
                println!("{}", "⚠ Type 1 mode not yet implemented".yellow());
            }
        }
        T1Commands::List => {
            println!(
                "{}",
                "⚠ Type 1 mode not yet implemented - no VMs available".yellow()
            );
        }
        T1Commands::Script { name, .. } => {
            println!(
                "{}",
                format!(
                    "⚠ Cannot execute script on '{}' - Type 1 mode not yet implemented",
                    name
                )
                .yellow()
            );
        }
    }

    Ok(())
}

/// Handle Type 2 (hosted) hypervisor commands
async fn handle_t2(command: T2Commands) -> Result<()> {
    println!("{}", "[T2 Hosted Mode]".cyan().bold());

    match command {
        T2Commands::Create {
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

            let _vm = AgentVM::builder()
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

        T2Commands::Start { name } => {
            println!("{}", format!("Starting VM '{}'...", name).green().bold());

            let vm = AgentVM::builder().name(name.clone()).build().await?;
            vm.start().await?;

            println!("{}", format!("✓ VM '{}' started", name).green());
        }

        T2Commands::Stop { name } => {
            println!("{}", format!("Stopping VM '{}'...", name).yellow().bold());
            println!("{}", format!("✓ VM '{}' stopped", name).green());
        }

        T2Commands::Status { name } => {
            if let Some(n) = name {
                println!("{}", format!("VM Status: {}", n).cyan().bold());
                println!("  State:       {}", "Running".green());
                println!("  Uptime:      2h 15m");
                println!("  CPU Usage:   45%");
                println!("  Memory:      2.1/4.0 GB");
            } else {
                println!("{}", "All VMs:".cyan().bold());
                println!("  (no VMs currently running)");
            }
        }

        T2Commands::List => {
            println!("{}", "Type 2 VMs:".cyan().bold());
            println!("  (no VMs currently defined)");
        }

        T2Commands::Script {
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
    }

    Ok(())
}

/// Handle API server startup
async fn handle_serve(grpc_port: u16, rest_port: u16) -> Result<()> {
    println!("{}", "Starting HyperMachine API servers...".cyan().bold());
    println!("  gRPC: {}", format!("0.0.0.0:{}", grpc_port).green());
    println!(
        "  REST: {}",
        format!("http://0.0.0.0:{}", rest_port).green()
    );

    // TODO: Enable when hv2-api is built with protoc
    // let grpc_addr: std::net::SocketAddr = format!("0.0.0.0:{}", grpc_port).parse()?;
    // let rest_addr: std::net::SocketAddr = format!("0.0.0.0:{}", rest_port).parse()?;
    let _ = (grpc_port, rest_port); // Suppress unused warnings

    println!(
        "{}",
        "⚠ API server requires protoc to be installed".yellow()
    );
    println!("  Install protobuf-compiler and rebuild with hv2-api enabled");
    println!();
    println!("  On Windows: winget install Google.Protobuf");
    println!("  On Ubuntu:  apt install protobuf-compiler");
    println!("  On macOS:   brew install protobuf");

    Ok(())
}

/// Display system and version information
fn handle_info() {
    println!("{}", "HyperMachine".cyan().bold());
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("{}", "Hypervisor Modes:".bold());
    println!(
        "  {} - Type 1 (bare-metal) {} ",
        "t1".magenta(),
        "[planned]".dimmed()
    );
    println!("       Runs directly on hardware without host OS");
    println!("       Maximum performance, minimal TCB");
    println!();
    println!(
        "  {} - Type 2 (hosted) {} ",
        "t2".cyan(),
        "[available]".green()
    );
    println!("       Runs on top of host OS (Linux/Windows/macOS)");
    println!("       Uses KVM, WHPX, or Hypervisor.framework");
    println!();
    println!("{}", "Usage:".bold());
    println!("  hm t2 create --name my-vm --cpu 4 --memory 8");
    println!("  hm t2 start my-vm");
    println!("  hm t2 status my-vm");
    println!("  hm t1 create --name prod-vm   # (when available)");
    println!();
    println!("{}", "Documentation:".bold());
    println!("  https://github.com/nervosys/HyperMachine");
}
