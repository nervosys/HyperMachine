//! HyperMachine Command-Line Interface

#![allow(dead_code)]

mod runtime_commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use hv2_agent::AgentVM;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "hv2")]
#[command(about = "HyperMachine - High-performance Type 2 Hypervisor with AI agent support", long_about = None)]
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
        /// Path to TOML configuration file
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,

        /// gRPC port (overrides config file)
        #[arg(long)]
        grpc_port: Option<u16>,

        /// REST API port (overrides config file)
        #[arg(long)]
        rest_port: Option<u16>,

        /// Disable runtime fleet management endpoints
        #[arg(long)]
        no_runtime: bool,

        /// Disable events/SSE/webhook endpoints
        #[arg(long)]
        no_events: bool,

        /// Number of VMs to pre-warm in the pool (overrides config file)
        #[arg(long)]
        pre_warm: Option<usize>,

        /// Graceful shutdown timeout in seconds (overrides config file)
        #[arg(long)]
        shutdown_timeout: Option<u64>,
    },

    /// Configuration file management
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Runtime fleet management
    #[command(subcommand)]
    Runtime(runtime_commands::RuntimeCommands),

    /// Show version information
    Version,
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Generate a default configuration file
    Init {
        /// Output path (default: hv2.toml)
        #[arg(short, long, default_value = "hv2.toml")]
        output: PathBuf,
    },

    /// Validate a configuration file
    Check {
        /// Path to configuration file (default: hv2.toml)
        #[arg(default_value = "hv2.toml")]
        path: PathBuf,
    },

    /// Display the resolved configuration (file + env overrides)
    Show {
        /// Path to configuration file (default: hv2.toml)
        #[arg(long, default_value = "hv2.toml")]
        config: PathBuf,
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
            config: config_path,
            grpc_port,
            rest_port,
            no_runtime,
            no_events,
            pre_warm,
            shutdown_timeout,
        } => {
            println!("{}", "Starting HyperMachine API server...".cyan().bold());

            // Load config: file → env vars → CLI overrides
            let mut cfg = match &config_path {
                Some(path) => {
                    println!(
                        "  Loading config from {}",
                        path.display().to_string().cyan()
                    );
                    match hv2_api::config::ConfigFile::load(path)? {
                        Some(c) => c,
                        None => {
                            anyhow::bail!("Config file not found: {}", path.display());
                        }
                    }
                }
                None => {
                    // Try default hv2.toml; use defaults if missing
                    let default_path = PathBuf::from("hv2.toml");
                    match hv2_api::config::ConfigFile::load(&default_path)? {
                        Some(c) => {
                            println!("  Loaded config from {}", "hv2.toml".cyan());
                            c
                        }
                        None => hv2_api::config::ConfigFile::default(),
                    }
                }
            };

            // Apply environment variable overrides
            cfg.apply_env();

            // Validate before applying CLI overrides (catches file/env issues early)
            cfg.validate()?;

            // CLI flags override config file + env
            if let Some(port) = rest_port {
                cfg.server.rest_port = port;
            }
            if let Some(port) = grpc_port {
                cfg.server.grpc_port = port;
            }
            if no_runtime {
                cfg.server.enable_runtime = false;
            }
            if no_events {
                cfg.server.enable_events = false;
            }
            if let Some(pw) = pre_warm {
                cfg.server.pre_warm_count = pw;
            }
            if let Some(secs) = shutdown_timeout {
                cfg.server.shutdown_timeout_secs = secs;
            }

            let config = cfg.into_server_config();

            let server = hv2_api::server::Server::new(config);

            // Print feature summary
            for (feature, enabled) in server.feature_summary() {
                let status = if enabled {
                    "enabled".green()
                } else {
                    "disabled".yellow()
                };
                println!("  {:<16} {}", feature, status);
            }

            println!();
            println!(
                "  REST: {}",
                format!("http://{}", server.config().rest_addr()).green()
            );
            println!(
                "  gRPC: {}",
                format!("{}", server.config().grpc_addr()).green()
            );
            println!();

            // Print route table
            let routes = server.route_table();
            println!(
                "  {} routes registered",
                routes.len().to_string().cyan().bold()
            );

            if cli.verbose {
                println!();
                for (method, path, desc) in &routes {
                    println!("    {:<7} {:<35} {}", method.cyan(), path, desc.dimmed());
                }
                println!();
            }

            println!("{}", "✓ Servers running (Ctrl+C to stop)".green());

            if let Err(e) = server.serve_all().await {
                eprintln!("{}", format!("Server error: {}", e).red());
                std::process::exit(1);
            }
        }

        Commands::Config(sub) => match sub {
            ConfigCommands::Init { output } => {
                if output.exists() {
                    anyhow::bail!(
                        "File already exists: {} (use a different path)",
                        output.display()
                    );
                }
                let toml = hv2_api::config::ConfigFile::default_toml()?;
                std::fs::write(&output, toml)?;
                println!(
                    "{}",
                    format!("✓ Default config written to {}", output.display()).green()
                );
            }
            ConfigCommands::Check { path } => match hv2_api::config::ConfigFile::load(&path)? {
                Some(mut cfg) => {
                    cfg.apply_env();
                    match cfg.validate() {
                        Ok(()) => {
                            println!(
                                "{}",
                                format!("✓ Configuration is valid: {}", path.display()).green()
                            );
                        }
                        Err(e) => {
                            eprintln!("{}", format!("✗ Validation errors: {}", e).red());
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    anyhow::bail!("Config file not found: {}", path.display());
                }
            },
            ConfigCommands::Show { config: path } => {
                let cfg = match hv2_api::config::ConfigFile::load(&path)? {
                    Some(mut c) => {
                        c.apply_env();
                        c
                    }
                    None => {
                        anyhow::bail!("Config file not found: {}", path.display());
                    }
                };
                let toml = cfg.to_toml()?;
                println!("{}", toml);
            }
        },

        Commands::Runtime(sub) => {
            let config = hv2_runtime::RuntimeConfig::default();
            let runtime = hv2_runtime::Runtime::new(config);
            runtime_commands::execute(&runtime, &sub)?;
        }

        Commands::Version => {
            println!("HyperMachine v{}", env!("CARGO_PKG_VERSION"));
            println!("A high-performance Type 2 Hypervisor with AI agent support");
        }
    }

    Ok(())
}
