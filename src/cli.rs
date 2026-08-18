//! GAR CLI - command-line interface definitions
//!
//! Uses clap derive for ergonomic argument parsing. Subcommands are
//! organized by domain (image, server, user, group, client, branding).

use clap::{Parser, Subcommand};

/// GAR — Unified manager for GAROS diskless clients and NixOS server.
#[derive(Debug, Parser)]
#[command(name = "gar", version, about, long_about = None)]
pub struct Cli {
    /// Output JSON instead of human-readable text
    #[arg(long, global = true, env = "GAR_JSON_OUTPUT")]
    pub json: bool,

    /// Verbose logging
    #[arg(long, global = true, short = 'v', env = "GAR_VERBOSE")]
    pub verbose: bool,

    /// Disable colored output
    #[arg(long, global = true, env = "GAR_NO_COLOR")]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage client diskless images (build, rollback, list, status, gc, doctor)
    #[command(subcommand)]
    Image(ImageCmd),

    /// Manage NixOS server (sync, switch, update, clean, check, etc)
    #[command(subcommand)]
    Server(ServerCmd),

    /// Manage users (add, resize, list, delete, doctor, quota-sync, activity)
    #[command(subcommand)]
    User(UserCmd),

    /// Manage groups (add, list, delete, chmod, members, permissions, ensure-defaults)
    #[command(subcommand)]
    Group(GroupCmd),

    /// Client diagnostics (session-doctor)
    #[command(subcommand)]
    Client(ClientCmd),

    /// Branding diagnostics (doctor)
    #[command(subcommand)]
    Branding(BrandingCmd),
}

/// Image subcommands (era ragc).
#[derive(Debug, Subcommand)]
pub enum ImageCmd {
    /// Build a new image and atomically promote it
    #[command(alias = "deploy")]
    Build {
        /// Target client type (desktop-generic, desktop-lab, hyperv-debug, rescue-minimal)
        #[arg(long, value_enum)]
        target: Option<ImageTarget>,

        /// Channel (generic, lab, rescue)
        #[arg(long, value_enum)]
        channel: Option<Channel>,
    },

    /// Rollback to the previous generation
    Rollback {
        /// Optional target version (e.g. v20260305-120000) or 'previous'
        target: Option<String>,

        /// Channel to rollback
        #[arg(long, value_enum)]
        channel: Option<Channel>,
    },

    /// List all generations
    #[command(alias = "ls")]
    List,

    /// Show status of active generation
    Status,

    /// Garbage collect old generations (with BTRFS snapshot prior)
    Gc {
        /// Number of generations to keep (default: GAR_KEEP_VERSIONS env var or 5)
        keep: Option<u32>,
    },

    /// Run health checks on infrastructure
    Doctor,
}

/// Server subcommands (era ragos top-level).
#[derive(Debug, Subcommand)]
pub enum ServerCmd {
    /// Sync operational checkout via Git
    Sync,
    /// Apply NixOS configuration (nixos-rebuild switch)
    #[command(alias = "apply")]
    Switch,
    /// Apply NixOS configuration without making it boot default (nixos-rebuild test)
    Test,
    /// Rollback to previous NixOS generation
    Rollback,
    /// Update flake inputs + check + switch
    Update,
    /// Clean old NixOS generations (nh clean or fallback)
    Clean,
    /// Run nix flake check
    Check {
        /// Validate the given clients inventory file (e.g. clients.nix)
        #[arg(long)]
        inventory: Option<std::path::PathBuf>,

        /// Allow empty inventory during validation (sets requireNonEmpty to false)
        #[arg(long)]
        allow_empty: bool,
    },
    /// Open nix repl on local flake
    Repl,
    /// Print operational flake path
    Path,
    /// cd to operational flake path + exec bash
    Enter,
    /// Show server status (flake/host/generation)
    Status,
}

/// User subcommands (era ragos user).
#[derive(Debug, Subcommand)]
pub enum UserCmd {
    Add {
        username: String,
        #[arg(long)]
        quota: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long = "password-hash")]
        password_hash: Option<String>,
        #[arg(long)]
        group: Option<String>,
    },
    Resize {
        username: String,
        #[arg(long)]
        quota: String,
        #[arg(long)]
        force: bool,
    },
    List,
    Delete {
        username: String,
        #[arg(long)]
        archive: bool,
    },
    Doctor {
        username: String,
    },
    #[command(name = "quota-sync")]
    QuotaSync,
    Activity {
        username: String,
    },
}

/// Group subcommands (era ragos group).
#[derive(Debug, Subcommand)]
pub enum GroupCmd {
    Add {
        groupname: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "storage-quota")]
        storage_quota: Option<String>,
    },
    List,
    Delete {
        groupname: String,
        #[arg(long)]
        archive: bool,
    },
    Chmod {
        groupname: String,
        perms: String,
    },
    Members {
        groupname: String,
        #[arg(long)]
        add: Option<String>,
        #[arg(long)]
        remove: Option<String>,
    },
    Permissions {
        groupname: String,
    },
    #[command(name = "ensure-defaults")]
    EnsureDefaults,
}

/// Client subcommands.
#[derive(Debug, Subcommand)]
pub enum ClientCmd {
    /// Diagnose session/home/published version of client
    #[command(name = "session-doctor")]
    SessionDoctor,

    /// List known clients from the GAROS inventory (best-effort).
    #[command(name = "list", alias = "ls")]
    List {
        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },

    /// Send Wake-on-LAN magic packet(s) to a client MAC address.
    #[command(name = "wake")]
    Wake {
        /// Target MAC address (XX:XX:XX:XX:XX:XX or XX-XX-XX-XX-XX-XX)
        mac: String,

        /// UDP destination port (default 9 — canonical WOL port)
        #[arg(long, default_value_t = 9)]
        port: u16,

        /// Number of magic packets to send (default 3 — most NICs accept any)
        #[arg(long, default_value_t = 3)]
        count: u8,

        /// Broadcast address override (default 255.255.255.255)
        #[arg(long)]
        broadcast: Option<String>,

        /// Emit JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

/// Branding subcommands.
#[derive(Debug, Subcommand)]
pub enum BrandingCmd {
    /// Diagnose Plymouth/SDDM/Plasma branding
    Doctor(crate::commands::branding::DoctorFlags),
}

/// Client target types (from ragc).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ImageTarget {
    DesktopGeneric,
    DesktopLab,
    HypervDebug,
    RescueMinimal,
}

impl ImageTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesktopGeneric => "desktop-generic",
            Self::DesktopLab => "desktop-lab",
            Self::HypervDebug => "hyperv-debug",
            Self::RescueMinimal => "rescue-minimal",
        }
    }
}

/// Channel types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Channel {
    Generic,
    Lab,
    Rescue,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Lab => "lab",
            Self::Rescue => "rescue",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_image_target_strings() {
        assert_eq!(ImageTarget::DesktopGeneric.as_str(), "desktop-generic");
        assert_eq!(ImageTarget::DesktopLab.as_str(), "desktop-lab");
        assert_eq!(ImageTarget::HypervDebug.as_str(), "hyperv-debug");
        assert_eq!(ImageTarget::RescueMinimal.as_str(), "rescue-minimal");
    }

    #[test]
    fn test_channel_strings() {
        assert_eq!(Channel::Generic.as_str(), "generic");
        assert_eq!(Channel::Lab.as_str(), "lab");
        assert_eq!(Channel::Rescue.as_str(), "rescue");
    }

    #[test]
    fn test_cli_image_build_parse() {
        let cli = Cli::try_parse_from([
            "gar",
            "image",
            "build",
            "--target",
            "desktop-generic",
            "--channel",
            "generic",
        ])
        .expect("should parse");

        assert!(!cli.json);
        assert!(matches!(
            cli.command,
            Command::Image(ImageCmd::Build { .. })
        ));
    }

    #[test]
    fn test_cli_image_build_deploy_alias() {
        let cli = Cli::try_parse_from(["gar", "image", "deploy"]).expect("alias should work");
        assert!(matches!(
            cli.command,
            Command::Image(ImageCmd::Build { .. })
        ));
    }

    #[test]
    fn test_cli_global_flags() {
        let cli = Cli::try_parse_from(["gar", "--json", "--verbose", "image", "status"])
            .expect("global flags should work");
        assert!(cli.json);
        assert!(cli.verbose);
    }
}
