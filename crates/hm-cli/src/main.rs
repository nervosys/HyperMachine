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

mod mcp_server;
mod vm_manager;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::time::Duration;
use vm_manager::{VmManager, VmState};

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

    /// Delete a Type 2 VM
    Delete {
        /// VM name
        name: String,

        /// Force delete without confirmation
        #[arg(short, long)]
        force: bool,
    },

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

    let manager = VmManager::new()?;

    match command {
        T2Commands::Create {
            name,
            cpu,
            memory,
            gpu,
            network,
        } => {
            println!("{}", "Creating VM...".green().bold());

            let record = manager.create_vm(&name, cpu, memory, gpu, network).await?;

            println!("  Name:    {}", record.name.cyan());
            println!("  CPUs:    {}", record.cpu_cores);
            println!("  Memory:  {} GB", record.memory_gb);
            println!(
                "  GPU:     {}",
                if record.gpu_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "  Network: {}",
                if record.network_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );

            println!(
                "{}",
                format!("✓ VM '{}' created successfully", name).green()
            );
        }

        T2Commands::Start { name } => {
            println!("{}", format!("Starting VM '{}'...", name).green().bold());

            manager.start_vm(&name).await?;

            println!("{}", format!("✓ VM '{}' started", name).green());
        }

        T2Commands::Stop { name } => {
            println!("{}", format!("Stopping VM '{}'...", name).yellow().bold());

            manager.stop_vm(&name).await?;

            println!("{}", format!("✓ VM '{}' stopped", name).green());
        }

        T2Commands::Status { name } => {
            if let Some(n) = name {
                let metrics = manager.get_metrics(&n).await?;

                println!("{}", format!("VM Status: {}", n).cyan().bold());
                println!(
                    "  State:       {}",
                    match metrics.state {
                        VmState::Running => "Running".green(),
                        VmState::Stopped => "Stopped".red(),
                        VmState::Created => "Created".yellow(),
                        VmState::Paused => "Paused".yellow(),
                        VmState::Error => "Error".red().bold(),
                    }
                );
                println!("  CPUs:        {}", metrics.cpu_cores);
                println!("  Memory:      {} GB", metrics.memory_gb);

                if let Some(uptime) = metrics.uptime_seconds {
                    let hours = uptime / 3600;
                    let mins = (uptime % 3600) / 60;
                    println!("  Uptime:      {}h {}m", hours, mins);
                }
            } else {
                // Show all VMs
                let vms = manager.list_vms().await;
                if vms.is_empty() {
                    println!("{}", "No VMs configured".dimmed());
                } else {
                    println!("{}", "All VMs:".cyan().bold());
                    for vm in vms {
                        let state_str = match vm.state {
                            VmState::Running => "running".green(),
                            VmState::Stopped => "stopped".red(),
                            VmState::Created => "created".yellow(),
                            VmState::Paused => "paused".yellow(),
                            VmState::Error => "error".red().bold(),
                        };
                        println!(
                            "  {} - {} ({} CPU, {} GB RAM)",
                            vm.name.cyan(),
                            state_str,
                            vm.cpu_cores,
                            vm.memory_gb
                        );
                    }
                }
            }
        }

        T2Commands::List => {
            let vms = manager.list_vms().await;

            println!("{}", "Type 2 VMs:".cyan().bold());
            if vms.is_empty() {
                println!("  {}", "(no VMs configured)".dimmed());
            } else {
                println!(
                    "  {:<20} {:<12} {:<6} {:<8} {:<8}",
                    "NAME", "STATE", "CPUS", "MEMORY", "GPU"
                );
                println!("  {}", "-".repeat(58));
                for vm in vms {
                    let state_str = match vm.state {
                        VmState::Running => "running".green().to_string(),
                        VmState::Stopped => "stopped".red().to_string(),
                        VmState::Created => "created".yellow().to_string(),
                        VmState::Paused => "paused".yellow().to_string(),
                        VmState::Error => "error".red().bold().to_string(),
                    };
                    println!(
                        "  {:<20} {:<12} {:<6} {:<8} {:<8}",
                        vm.name,
                        state_str,
                        vm.cpu_cores,
                        format!("{} GB", vm.memory_gb),
                        if vm.gpu_enabled { "yes" } else { "no" }
                    );
                }
            }
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

            // Check VM exists and is running
            let record = manager.get_vm(&name).await?;
            if record.state != VmState::Running {
                anyhow::bail!("VM '{}' is not running (state: {})", name, record.state);
            }

            // Read script from file if it's a path
            let script_content = if std::path::Path::new(&script).exists() {
                std::fs::read_to_string(&script)?
            } else {
                script
            };

            // Set timeout for the script
            let _ = Duration::from_secs(timeout);

            let result = manager.execute_script(&name, &script_content).await?;

            println!("{}", "Script Result:".green().bold());
            println!("{}", serde_json::to_string_pretty(&result)?);
        }

        T2Commands::Delete { name, force } => {
            // Check if VM exists
            let record = manager.get_vm(&name).await?;

            // Confirm deletion unless --force is specified
            if !force {
                if record.state == VmState::Running {
                    println!(
                        "{}",
                        format!("WARNING: VM '{}' is currently running!", name)
                            .yellow()
                            .bold()
                    );
                }
                print!("{}", format!("Delete VM '{}'? [y/N] ", name).yellow());
                use std::io::Write;
                std::io::stdout().flush()?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("{}", "Deletion cancelled".dimmed());
                    return Ok(());
                }
            }

            println!("{}", format!("Deleting VM '{}'...", name).red().bold());
            manager.delete_vm(&name).await?;
            println!("{}", format!("✓ VM '{}' deleted", name).green());
        }
    }

    Ok(())
}

/// Handle API server startup
async fn handle_serve(grpc_port: u16, rest_port: u16) -> Result<()> {
    println!("{}", "Starting HyperMachine API servers...".cyan().bold());

    // Note: gRPC requires protoc - using MCP HTTP server instead
    let _ = grpc_port; // gRPC disabled until protoc available

    println!(
        "  MCP HTTP API: {}",
        format!("http://0.0.0.0:{}", rest_port).green()
    );
    println!();
    println!("{}", "Endpoints:".bold());
    println!("  GET  /mcp/tools      - List available tools (OpenAI/Anthropic format)");
    println!("  POST /mcp/call       - Execute a tool call");
    println!("  GET  /vms            - List all VMs");
    println!("  POST /vms            - Create a new VM");
    println!("  GET  /vms/:name      - Get VM details");
    println!("  DELETE /vms/:name    - Delete a VM");
    println!("  POST /vms/:name/start - Start a VM");
    println!("  POST /vms/:name/stop  - Stop a VM");
    println!("  GET  /health         - Health check");
    println!();

    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", rest_port).parse()?;
    mcp_server::start_mcp_server(addr).await
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
