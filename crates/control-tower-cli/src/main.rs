//! Control Tower CLI
//!
//! Command line interface for proxy management

mod profile;
mod proxy;
mod service;
mod config;
mod mode;
mod connections;
mod rule;
mod settings;
mod update;
use update::UpdateAction;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ctctl")]
#[command(version = "0.1.0")]
#[command(about = "Control Tower - Command line interface for proxy management")]
#[command(long_about = "Control Tower - Command line interface for proxy management

Quick Start:
  ctctl service start            # Start the service
  ctctl proxy list              # List available proxies
  ctctl proxy select \"香港 101\"   # Select a proxy
  ctctl mode set global         # Switch to global mode

Examples:
  # Profile management
  ctctl profile add https://example.com/sub.yaml
  ctctl profile list
  ctctl profile activate <ID>   # Activate a profile
  ctctl profile cron <ID> 60    # Set auto-update every 60 minutes
  ctctl profile remove <ID>     # Remove a profile

  # Proxy management
  ctctl proxy list
  ctctl proxy select \"香港 101\"
  ctctl proxy test

  # Mode management
  ctctl mode get
  ctctl mode set rule

  # Service management
  ctctl service start
  ctctl service status
  ctctl service restart

  # Connections management
  ctctl connections list
  ctctl connections top         # Real-time monitor
  ctctl connections detail 1
  ctctl connections close 1

  # Rule management
  ctctl rule list
  ctctl rule add DOMAIN-SUFFIX,google.com,香港 101
  ctctl rule remove 5

  # Update
  ctctl update all               # Update mihomo and databases
")]
struct Cli {
    /// Path to configuration file
    ///
    /// Default: settings.yaml in the same directory as the executable,
    /// or ~/.config/control-tower/settings.yaml
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Profile (subscription) management
    ///
    /// Manage subscription profiles for proxy providers.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Proxy management
    ///
    /// List, select, and test proxy nodes.
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },
    /// Mode management (rule/global/direct)
    ///
    /// Switch between proxy modes.
    Mode {
        #[command(subcommand)]
        action: ModeAction,
    },
    /// Service management
    ///
    /// Start, stop, restart, and check status of the Control Tower service.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Rule management
    ///
    /// Manage routing rules for proxy.
    Rule {
        #[command(subcommand)]
        action: RuleAction,
    },
    /// Connections viewer
    ///
    /// View and manage active connections.
    Connections {
        #[command(subcommand)]
        action: ConnectionsAction,
    },
    /// Update mihomo binary and geoip/geosite databases
    ///
    /// Downloads the latest versions from GitHub releases.
    ///
    /// Examples:
    ///   ctctl update all        # Update everything
    ///   ctctl update mihomo    # Update mihomo binary only
    ///   ctctl update geoip     # Update geoip database only
    ///   ctctl update geosite   # Update geosite database only
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
}

#[derive(Subcommand, Debug)]
enum ProfileAction {
    /// Add a new subscription profile
    ///
    /// Examples:
    ///   ctctl profile add https://example.com/sub.yaml
    ///   ctctl profile add https://example.com/sub.yaml "My Profile"
    Add {
        /// Subscription URL
        url: String,
        /// Profile name (optional)
        name: Option<String>,
    },
    /// List all profiles
    ///
    /// Shows all configured subscription profiles with their status.
    List,
    /// Remove a profile
    ///
    /// Example:
    ///   ctctl profile remove 12345678
    Remove {
        /// Profile ID
        id: String,
    },
    /// Update a profile
    ///
    /// Fetches the latest subscription data.
    /// If no ID is specified, updates the currently active profile.
    Update {
        /// Profile ID (optional, updates current if not specified)
        id: Option<String>,
    },
    /// Activate a profile
    ///
    /// Sets the specified profile as the active profile.
    /// The active profile is used by the proxy service.
    ///
    /// Example:
    ///   ctctl profile activate 12345678
    Activate {
        /// Profile ID to activate
        id: String,
    },
    /// Set or show cron schedule for automatic updates
    ///
    /// Without a schedule argument, shows the current cron setting.
    ///
    /// Schedule format: number (minutes)
    /// Update runs every N minutes.
    ///
    /// Examples:
    ///   ctctl profile cron 12345678    # Show current schedule
    ///   ctctl profile cron 12345678 5  # Update every 5 minutes
    ///   ctctl profile cron 12345678 1  # Update every 1 minute
    ///   ctctl profile cron 12345678 off # Disable auto update
    Cron {
        /// Profile ID
        id: String,
        /// Minutes between updates, or "off" to disable
        schedule: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ProxyAction {
    /// List all proxy nodes
    ///
    /// Displays all available proxy nodes organized by groups.
    List,
    /// Select a proxy node
    ///
    /// Sets the active proxy in the GLOBAL selector group.
    /// Supports index selection (prefix with #), exact name, or partial match.
    /// Use `ctctl proxy list` to see available proxies with their index numbers.
    ///
    /// Examples:
    ///   ctctl proxy select "#7"          # Select by index
    ///   ctctl proxy select "香港 101"    # Select by name
    ///   ctctl proxy select "日本"        # Partial match (selects first match)
    Select {
        /// Proxy name or index (e.g., "#7" or "香港 101")
        name: String,
    },
    /// Test proxy latency
    ///
    /// Tests the delay of a specific proxy or all proxies in the GLOBAL group.
    /// Uses http://cp.cloudflare.com/generate_204 as the test URL.
    ///
    /// Examples:
    ///   ctctl proxy test                    # Test all proxies
    ///   ctctl proxy test "香港 101"         # Test specific proxy
    Test {
        /// Proxy name (optional, tests all if not specified)
        name: Option<String>,
    },
    /// Show proxy statistics
    ///
    /// Displays statistics for all proxies including latency, health status,
    /// and connection count. This helps identify the best performing nodes.
    ///
    /// Example:
    ///   ctctl proxy stats
    Stats,
}

#[derive(Subcommand, Debug)]
enum ModeAction {
    /// Get current mode
    ///
    /// Shows the currently active proxy mode.
    Get,
    /// Set proxy mode
    ///
    /// Modes:
    ///   rule   - Route traffic based on rules (default)
    ///   global - Route all traffic through a selected proxy
    ///   direct - Direct connection, bypass proxy
    ///
    /// Example:
    ///   ctctl mode set global
    Set {
        /// Mode: rule, global, or direct
        mode: String,
    },
    /// TUN mode management
    ///
    /// TUN mode enables VPN-like functionality to intercept all system traffic.
    /// This requires the service to support TUN (Linux kernel support).
    Tun {
        #[command(subcommand)]
        action: TunAction,
    },
}

#[derive(Subcommand, Debug)]
enum TunAction {
    /// Get TUN mode status
    ///
    /// Shows whether TUN mode is currently enabled.
    ///
    /// Example:
    ///   ctctl mode tun status
    Status,
    /// Enable TUN mode
    ///
    /// Enables VPN-like tunnel mode. This intercepts all system traffic
    /// and routes it through the proxy. Requires service restart.
    ///
    /// Example:
    ///   ctctl mode tun enable
    Enable,
    /// Disable TUN mode
    ///
    /// Disables TUN mode. Regular proxy mode will be used.
    /// Requires service restart.
    ///
    /// Example:
    ///   ctctl mode tun disable
    Disable,
}

#[derive(Subcommand, Debug)]
enum ServiceAction {
    /// Start the Control Tower service
    ///
    /// Starts the proxy service. If not running, spawns the
    /// service daemon and initializes Mihomo with the config file.
    ///
    /// Example:
    ///   ctctl service start
    Start,
    /// Stop the Control Tower service
    ///
    /// Stops the proxy service gracefully.
    ///
    /// Example:
    ///   ctctl service stop
    Stop,
    /// Get service status
    ///
    /// Shows whether the service is running, including PID and uptime.
    ///
    /// Example:
    ///   ctctl service status
    Status,
    /// Restart the service
    ///
    /// Stops then starts the service. Useful for reloading configuration.
    ///
    /// Example:
    ///   ctctl service restart
    Restart,
}

#[derive(Subcommand, Debug)]
enum RuleAction {
    /// List all routing rules
    ///
    /// Shows current rules with their indices.
    /// Use --limit to limit output.
    ///
    /// Example:
    ///   ctctl rule list
    ///   ctctl rule list --limit 20
    List {
        /// Limit output to first N rules
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Add a new rule
    ///
    /// Adds a rule to the end of the rule list.
    /// Rule format: TYPE,VALUE,PROXY
    ///
    /// Supported types:
    ///   DOMAIN         - Exact domain match
    ///   DOMAIN-SUFFIX - Domain suffix match
    ///   DOMAIN-KEYWORD - Domain keyword match
    ///   GEOIP          - IP geolocation (e.g., GEOIP,CN,DIRECT)
    ///   IP-CIDR        - IP range (e.g., IP-CIDR,10.0.0.0/8,DIRECT)
    ///   IP-CIDR6       - IPv6 range
    ///   PROCESS-NAME   - Process name match
    ///   RULE-SET       - External rule set
    ///   MATCH         - Default fallback (must be last)
    ///
    /// Special proxies: DIRECT, REJECT, REJECT-DROP
    ///
    /// Example:
    ///   ctctl rule add "DOMAIN-SUFFIX,google.com,香港 101"
    ///   ctctl rule add "GEOIP,CN,DIRECT"
    ///   ctctl rule add "IP-CIDR,10.0.0.0/8,DIRECT"
    Add {
        /// Rule in TYPE,VALUE,PROXY format
        rule: String,
    },
    /// Remove a rule by index
    ///
    /// Use 'rule list' to see indices.
    ///
    /// Example:
    ///   ctctl rule remove 5
    Remove {
        /// Rule index (1-based, from 'rule list')
        index: usize,
    },
    /// Import rules from a file
    ///
    /// Supports YAML or plain text format.
    /// Each line or YAML entry should be a rule.
    ///
    /// Example:
    ///   ctctl rule import /path/to/rules.yaml
    Import {
        /// Path to rules file
        path: String,
    },
    /// Export rules to a file or stdout
    ///
    /// Example:
    ///   ctctl rule export
    ///   ctctl rule export /path/to/rules.yaml
    Export {
        /// Path to export to (optional, prints to stdout if not specified)
        path: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ConnectionsAction {
    /// List all active connections
    ///
    /// Shows current connections with traffic statistics.
    List,
    /// Close a connection by index
    ///
    /// Terminates the specified connection.
    ///
    /// Example:
    ///   ctctl connections close 1
    Close {
        /// Connection index (use 'connections list' to see indexes)
        index: usize,
    },
    /// Show detailed connection information
    ///
    /// Displays detailed information about a specific connection.
    ///
    /// Example:
    ///   ctctl connections detail 1
    Detail {
        /// Connection index (use 'connections list' to see indexes)
        index: usize,
    },
    /// Real-time connection monitor (like 'top')
    ///
    /// Displays live-updating connection statistics.
    /// Press Ctrl+C to exit.
    ///
    /// Example:
    ///   ctctl connections top
    Top {
        /// Refresh interval in seconds (default: 1)
        #[arg(short, long, default_value = "1")]
        interval: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Set config path if specified
    if let Some(config_path) = &cli.config {
        settings::set_config_path(config_path.clone());
    }

    // Initialize settings
    settings::init();

    // If no config file exists, create default one
    if settings::get_settings_path().map(|p| !p.exists()).unwrap_or(true) {
        if let Err(e) = settings::create_default_settings() {
            tracing::warn!("Could not create default settings: {}", e);
        } else if let Some(path) = settings::get_settings_path() {
            println!("Created default settings at: {}", path.display());
        }
    }

    match cli.command {
        Commands::Profile { action } => profile::handle(action).await?,
        Commands::Proxy { action } => proxy::handle(action).await?,
        Commands::Mode { action } => mode::handle(action).await?,
        Commands::Service { action } => service::handle(action).await?,
        Commands::Rule { action } => rule::handle(action).await?,
        Commands::Connections { action } => connections::handle(action).await?,
        Commands::Update { action } => update::handle(action).await?,
    }

    Ok(())
}
