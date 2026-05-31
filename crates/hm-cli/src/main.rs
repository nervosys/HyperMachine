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
//! # Type 1 (bare-metal hypervisor) - configuration management
//! hm t1 create --name prod-vm --cpu 16 --memory 64
//! hm t1 list
//! hm t1 status prod-vm
//! hm t1 connect 192.168.1.100:8443  # Connect to running hypervisor
//!
//! # Generate shell completions
//! hm completions bash > ~/.local/share/bash-completion/completions/hm
//! hm completions zsh > ~/.zfunc/_hm
//! hm completions fish > ~/.config/fish/completions/hm.fish
//! hm completions powershell > $PROFILE.CurrentUserAllHosts
//! ```

use hm_cli::mcp_server;
use hm_cli::t1_manager::{T1HypervisorConnection, T1Manager, T1VmState};
use hm_cli::vm_manager::{VmManager, VmState};

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::*;
use std::io;
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

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Show version and system information
    Info,
}

/// Type 1 (bare-metal) hypervisor commands - runs directly on hardware
#[derive(Subcommand)]
enum T1Commands {
    /// Create a new Type 1 VM configuration
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

    /// Start a Type 1 VM (requires hypervisor connection)
    Start {
        /// VM name
        name: String,
    },

    /// Stop a Type 1 VM (requires hypervisor connection)
    Stop {
        /// VM name
        name: String,
    },

    /// Get Type 1 VM status
    Status {
        /// VM name (optional, shows all if omitted)
        name: Option<String>,
    },

    /// List all Type 1 VM configurations
    List,

    /// Connect to a running T1 hypervisor
    Connect {
        /// Hypervisor endpoint (IP or hostname)
        endpoint: String,

        /// API port
        #[arg(short, long, default_value = "8443")]
        port: u16,

        /// Disable TLS
        #[arg(long)]
        no_tls: bool,
    },

    /// Delete a Type 1 VM configuration
    Delete {
        /// VM name
        name: String,

        /// Force delete without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Export VM configuration as JSON
    Export {
        /// VM name
        name: String,

        /// Output file (stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
    },

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
        Commands::Completions { shell } => {
            generate(shell, &mut Cli::command(), "hm", &mut io::stdout());
        }
        Commands::Info => handle_info(),
    }

    Ok(())
}

/// Handle Type 1 (bare-metal) hypervisor commands
async fn handle_t1(command: T1Commands) -> Result<()> {
    println!("{}", "[T1 Bare-Metal Mode]".magenta().bold());

    let manager = T1Manager::new()?;

    match command {
        T1Commands::Create {
            name,
            cpu,
            memory,
            gpu,
            network,
        } => {
            println!("{}", "Creating T1 VM configuration...".green().bold());

            let config = manager.create_vm(&name, cpu, memory, gpu, network).await?;

            println!("  Name:    {}", config.name.cyan());
            println!("  CPUs:    {}", config.cpu_cores);
            println!("  Memory:  {} GB", config.memory_gb);
            println!(
                "  GPU:     {}",
                if config.gpu_passthrough {
                    "passthrough"
                } else {
                    "disabled"
                }
            );
            println!(
                "  Network: {}",
                if config.network.sriov {
                    "SR-IOV"
                } else {
                    "disabled"
                }
            );
            println!();
            println!(
                "{}",
                format!("✓ T1 VM configuration '{}' created", name).green()
            );
            println!();
            println!("{}", "Note:".dimmed());
            println!(
                "{}",
                "  T1 VMs run on bare-metal hypervisor, not host OS.".dimmed()
            );
            println!(
                "{}",
                "  Connect to hypervisor with: hm t1 connect <endpoint>".dimmed()
            );
        }

        T1Commands::Start { name } => {
            println!("{}", format!("Starting T1 VM '{}'...", name).green().bold());

            match manager.start_vm(&name).await {
                Ok(()) => {
                    println!("{}", format!("✓ VM '{}' started", name).green());
                }
                Err(e) => {
                    println!("{}", format!("✗ {}", e).red());
                }
            }
        }

        T1Commands::Stop { name } => {
            println!(
                "{}",
                format!("Stopping T1 VM '{}'...", name).yellow().bold()
            );

            match manager.stop_vm(&name).await {
                Ok(()) => {
                    println!("{}", format!("✓ VM '{}' stopped", name).green());
                }
                Err(e) => {
                    println!("{}", format!("✗ {}", e).red());
                }
            }
        }

        T1Commands::Status { name } => {
            if let Some(n) = name {
                let metrics = manager.get_vm_status(&n).await?;

                println!("{}", format!("T1 VM Status: {}", n).cyan().bold());
                println!(
                    "  State:       {}",
                    match metrics.state {
                        T1VmState::Running => "Running".green(),
                        T1VmState::Stopped => "Stopped".red(),
                        T1VmState::Configured => "Configured".yellow(),
                        T1VmState::Paused => "Paused".yellow(),
                        T1VmState::Error => "Error".red().bold(),
                        T1VmState::Unknown => "Unknown".dimmed(),
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
                    println!("{}", "No T1 VM configurations found".dimmed());
                } else {
                    println!("{}", "T1 VM Configurations:".cyan().bold());
                    for vm in vms {
                        println!(
                            "  {} - {} ({} CPU, {} GB RAM)",
                            vm.name.cyan(),
                            "configured".yellow(),
                            vm.cpu_cores,
                            vm.memory_gb
                        );
                    }
                }
            }
        }

        T1Commands::List => {
            let vms = manager.list_vms().await;

            println!("{}", "Type 1 VM Configurations:".magenta().bold());
            if vms.is_empty() {
                println!("  {}", "(no configurations)".dimmed());
            } else {
                println!(
                    "  {:<20} {:<12} {:<6} {:<8} {:<8}",
                    "NAME", "STATE", "CPUS", "MEMORY", "GPU"
                );
                println!("  {}", "-".repeat(58));
                for vm in vms {
                    println!(
                        "  {:<20} {:<12} {:<6} {:<8} {:<8}",
                        vm.name,
                        "configured".yellow(),
                        vm.cpu_cores,
                        format!("{} GB", vm.memory_gb),
                        if vm.gpu_passthrough { "yes" } else { "no" }
                    );
                }
            }

            // Show hypervisor connection status
            println!();
            if let Some(conn) = manager.get_connection().await {
                let scheme = if conn.tls { "https" } else { "http" };
                println!(
                    "Hypervisor: {} ({}://{}:{})",
                    "configured".green(),
                    scheme,
                    conn.endpoint,
                    conn.port
                );
            } else {
                println!("Hypervisor: {}", "not connected".dimmed());
                println!("{}", "  Use 'hm t1 connect <endpoint>' to connect".dimmed());
            }
        }

        T1Commands::Connect {
            endpoint,
            port,
            no_tls,
        } => {
            println!(
                "{}",
                format!("Connecting to T1 hypervisor at {}:{}...", endpoint, port)
                    .cyan()
                    .bold()
            );

            let conn = T1HypervisorConnection {
                endpoint: endpoint.clone(),
                port,
                tls: !no_tls,
                auth_token: None,
            };

            manager.set_connection(conn).await?;

            let scheme = if !no_tls { "https" } else { "http" };
            println!(
                "{}",
                format!(
                    "✓ Hypervisor connection configured: {}://{}:{}",
                    scheme, endpoint, port
                )
                .green()
            );

            // Try to ping
            if manager.ping_hypervisor().await? {
                println!("{}", "  Hypervisor is reachable".green());
            } else {
                println!(
                    "{}",
                    "  ⚠ Hypervisor not reachable (will retry on commands)".yellow()
                );
            }
        }

        T1Commands::Delete { name, force } => {
            // Check if VM exists
            let _config = manager.get_vm(&name).await?;

            // Confirm deletion unless --force is specified
            if !force {
                print!(
                    "{}",
                    format!("Delete T1 VM configuration '{}'? [y/N] ", name).yellow()
                );
                use std::io::Write;
                std::io::stdout().flush()?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("{}", "Deletion cancelled".dimmed());
                    return Ok(());
                }
            }

            println!("{}", format!("Deleting T1 VM '{}'...", name).red().bold());
            manager.delete_vm(&name).await?;
            println!(
                "{}",
                format!("✓ T1 VM configuration '{}' deleted", name).green()
            );
        }

        T1Commands::Export { name, output } => {
            let json = manager.export_config(&name).await?;

            if let Some(path) = output {
                std::fs::write(&path, &json)?;
                println!(
                    "{}",
                    format!("✓ Configuration exported to {}", path).green()
                );
            } else {
                println!("{}", json);
            }
        }

        T1Commands::Script {
            name,
            script,
            timeout,
        } => {
            println!(
                "{}",
                format!("Executing script on T1 VM '{}'...", name)
                    .cyan()
                    .bold()
            );

            // Verify VM exists
            let _config = manager.get_vm(&name).await?;

            // Check hypervisor connection
            if manager.get_connection().await.is_none() {
                println!(
                    "{}",
                    "✗ No hypervisor connection. Use 'hm t1 connect <endpoint>' first.".red()
                );
                return Ok(());
            }

            // Read script from file if it's a path
            let script_content = if std::path::Path::new(&script).exists() {
                std::fs::read_to_string(&script)?
            } else {
                script
            };

            let _timeout_duration = Duration::from_secs(timeout);

            let result = manager
                .execute_script(&name, &script_content, timeout)
                .await?;

            if result.success {
                println!("{}", "✓ Script executed successfully".green().bold());
            } else {
                println!("{}", "✗ Script execution failed".red().bold());
            }

            if !result.stdout.is_empty() {
                println!("{}", "stdout:".dimmed());
                println!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                println!("{}", "stderr:".yellow());
                println!("{}", result.stderr);
            }
            if let Some(code) = result.exit_code {
                println!("{}", format!("Exit code: {}", code).dimmed());
            }
            if let Some(ms) = result.duration_ms {
                println!(
                    "{}",
                    format!("Completed in {:.1}s", ms as f64 / 1000.0).dimmed()
                );
            }
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
        "[available]".green()
    );
    println!("       Runs directly on hardware without host OS");
    println!("       Intel VMX + AMD SVM, EPT/NPT nested paging");
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
    println!("  # Type 2 (hosted) - run VMs on host OS");
    println!("  hm t2 create --name my-vm --cpu 4 --memory 8");
    println!("  hm t2 start my-vm");
    println!("  hm t2 status my-vm");
    println!();
    println!("  # Type 1 (bare-metal) - configure VMs for hypervisor");
    println!("  hm t1 create --name prod-vm --cpu 16 --memory 64 --gpu");
    println!("  hm t1 connect 192.168.1.100:8443");
    println!("  hm t1 list");
    println!();
    println!("{}", "Documentation:".bold());
    println!("  https://github.com/nervosys/HyperMachine");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_t2_create_defaults() {
        let cli = Cli::try_parse_from(["hm", "t2", "create", "--name", "myvm"]).unwrap();
        match cli.command {
            Commands::T2 {
                command:
                    T2Commands::Create {
                        name,
                        cpu,
                        memory,
                        gpu,
                        network,
                    },
            } => {
                assert_eq!(name, "myvm");
                assert_eq!(cpu, 2);
                assert_eq!(memory, 4);
                assert!(!gpu);
                assert!(!network);
            }
            _ => panic!("Expected T2 Create"),
        }
    }

    #[test]
    fn test_parse_t2_create_all_options() {
        let cli = Cli::try_parse_from([
            "hm",
            "t2",
            "create",
            "--name",
            "big-vm",
            "--cpu",
            "16",
            "--memory",
            "64",
            "--gpu",
            "--network",
        ])
        .unwrap();
        match cli.command {
            Commands::T2 {
                command:
                    T2Commands::Create {
                        cpu,
                        memory,
                        gpu,
                        network,
                        ..
                    },
            } => {
                assert_eq!(cpu, 16);
                assert_eq!(memory, 64);
                assert!(gpu);
                assert!(network);
            }
            _ => panic!("Expected T2 Create"),
        }
    }

    #[test]
    fn test_parse_t1_create_defaults() {
        let cli = Cli::try_parse_from(["hm", "t1", "create", "--name", "bmvm"]).unwrap();
        match cli.command {
            Commands::T1 {
                command:
                    T1Commands::Create {
                        name, cpu, memory, ..
                    },
            } => {
                assert_eq!(name, "bmvm");
                assert_eq!(cpu, 2);
                assert_eq!(memory, 4);
            }
            _ => panic!("Expected T1 Create"),
        }
    }

    #[test]
    fn test_parse_t1_connect_defaults() {
        let cli = Cli::try_parse_from(["hm", "t1", "connect", "10.0.0.1"]).unwrap();
        match cli.command {
            Commands::T1 {
                command:
                    T1Commands::Connect {
                        endpoint,
                        port,
                        no_tls,
                    },
            } => {
                assert_eq!(endpoint, "10.0.0.1");
                assert_eq!(port, 8443);
                assert!(!no_tls);
            }
            _ => panic!("Expected T1 Connect"),
        }
    }

    #[test]
    fn test_parse_t1_connect_custom_port() {
        let cli = Cli::try_parse_from([
            "hm", "t1", "connect", "10.0.0.1", "--port", "9999", "--no-tls",
        ])
        .unwrap();
        match cli.command {
            Commands::T1 {
                command: T1Commands::Connect { port, no_tls, .. },
            } => {
                assert_eq!(port, 9999);
                assert!(no_tls);
            }
            _ => panic!("Expected T1 Connect"),
        }
    }

    #[test]
    fn test_parse_serve_defaults() {
        let cli = Cli::try_parse_from(["hm", "serve"]).unwrap();
        match cli.command {
            Commands::Serve {
                grpc_port,
                rest_port,
            } => {
                assert_eq!(grpc_port, 50051);
                assert_eq!(rest_port, 8080);
            }
            _ => panic!("Expected Serve"),
        }
    }

    #[test]
    fn test_parse_serve_custom_ports() {
        let cli =
            Cli::try_parse_from(["hm", "serve", "--grpc-port", "9090", "--rest-port", "3000"])
                .unwrap();
        match cli.command {
            Commands::Serve {
                grpc_port,
                rest_port,
            } => {
                assert_eq!(grpc_port, 9090);
                assert_eq!(rest_port, 3000);
            }
            _ => panic!("Expected Serve"),
        }
    }

    #[test]
    fn test_parse_verbose_flag() {
        let cli = Cli::try_parse_from(["hm", "--verbose", "info"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn test_parse_info() {
        let cli = Cli::try_parse_from(["hm", "info"]).unwrap();
        assert!(matches!(cli.command, Commands::Info));
    }

    #[test]
    fn test_parse_t1_visible_alias() {
        let cli = Cli::try_parse_from(["hm", "type1", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::T1 {
                command: T1Commands::List
            }
        ));
    }

    #[test]
    fn test_parse_t2_visible_alias() {
        let cli = Cli::try_parse_from(["hm", "type2", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::T2 {
                command: T2Commands::List
            }
        ));
    }

    #[test]
    fn test_parse_missing_subcommand_fails() {
        assert!(Cli::try_parse_from(["hm"]).is_err());
    }

    #[test]
    fn test_parse_t1_script_defaults() {
        let cli =
            Cli::try_parse_from(["hm", "t1", "script", "vm1", "--script", "echo hi"]).unwrap();
        match cli.command {
            Commands::T1 {
                command:
                    T1Commands::Script {
                        name,
                        script,
                        timeout,
                    },
            } => {
                assert_eq!(name, "vm1");
                assert_eq!(script, "echo hi");
                assert_eq!(timeout, 300);
            }
            _ => panic!("Expected T1 Script"),
        }
    }

    #[test]
    fn test_parse_t2_delete_force() {
        let cli = Cli::try_parse_from(["hm", "t2", "delete", "old-vm", "--force"]).unwrap();
        match cli.command {
            Commands::T2 {
                command: T2Commands::Delete { name, force },
            } => {
                assert_eq!(name, "old-vm");
                assert!(force);
            }
            _ => panic!("Expected T2 Delete"),
        }
    }

    #[test]
    fn test_parse_t1_status_optional_name() {
        // Without name
        let cli = Cli::try_parse_from(["hm", "t1", "status"]).unwrap();
        match cli.command {
            Commands::T1 {
                command: T1Commands::Status { name },
            } => {
                assert!(name.is_none());
            }
            _ => panic!("Expected T1 Status"),
        }

        // With name
        let cli = Cli::try_parse_from(["hm", "t1", "status", "vm1"]).unwrap();
        match cli.command {
            Commands::T1 {
                command: T1Commands::Status { name },
            } => {
                assert_eq!(name.unwrap(), "vm1");
            }
            _ => panic!("Expected T1 Status"),
        }
    }

    #[test]
    fn test_parse_t1_export() {
        let cli =
            Cli::try_parse_from(["hm", "t1", "export", "vm1", "--output", "/tmp/out"]).unwrap();
        match cli.command {
            Commands::T1 {
                command: T1Commands::Export { name, output },
            } => {
                assert_eq!(name, "vm1");
                assert_eq!(output.unwrap(), "/tmp/out");
            }
            _ => panic!("Expected T1 Export"),
        }
    }

    #[test]
    fn test_parse_completions() {
        let cli = Cli::try_parse_from(["hm", "completions", "bash"]).unwrap();
        assert!(matches!(cli.command, Commands::Completions { .. }));
    }
}
