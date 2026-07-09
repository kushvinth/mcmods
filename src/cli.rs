use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mcmod", version, about = "Declarative Minecraft mod manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scan existing installation and generate config + lockfile
    Init {
        /// Path to the Minecraft directory
        #[arg(short, long)]
        minecraft_dir: Option<PathBuf>,
        /// Force overwrite existing config
        #[arg(short, long)]
        force: bool,
    },
    /// Show what would change without modifying anything
    Plan {
        /// Only show actions for this instance
        #[arg(short, long)]
        instance: Option<String>,
    },
    /// Reconcile all instances with the declared config
    Apply {
        /// Only apply changes to this instance
        #[arg(short, long)]
        instance: Option<String>,
    },
    /// Re-resolve newer compatible versions and update the lockfile
    Update,
    /// Show drift between declared state and actual instance mod folders
    Status,
    /// Add a mod to an instance's config
    Add {
        /// Mod slug or name to add
        r#mod: String,
        /// Target instance name
        #[arg(short, long)]
        instance: String,
    },
    /// Remove a mod from an instance's config
    Remove {
        /// Mod slug to remove
        r#mod: String,
        /// Target instance name
        #[arg(short, long)]
        instance: String,
    },
    /// List configured mods
    List {
        /// Only show this instance
        #[arg(short, long)]
        instance: Option<String>,
    },
    /// Search Modrinth for mods
    Search {
        /// Search query
        query: String,
    },
    /// Show Modrinth project info for a mod
    Info {
        /// Mod slug or project ID
        r#mod: String,
    },
    /// Rename a mod slug in an instance's config
    Rename {
        /// Current mod slug
        old: String,
        /// New mod slug
        new: String,
        /// Target instance name
        #[arg(short, long)]
        instance: String,
    },
}
