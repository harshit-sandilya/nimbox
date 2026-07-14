use clap::{Parser, Subcommand};

// Release builds can inject this (e.g. v0.1.4) from CI.
const NIMBOX_BUILD_VERSION: &str = match option_env!("NIMBOX_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(
    name = "nimbox",
    version = NIMBOX_BUILD_VERSION,
    about = "Cross-provider AI API testing and compatibility proxy"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the Nimbox proxy server
    Start {
        #[arg(short, long, default_value_t = 11500)]
        port: u16,
    },

    /// Stop Nimbox proxy server
    Stop,

    /// Add an API key for the active provider
    Add {
        #[arg(short, long)]
        name: String,

        key: String,
    },

    /// Remove an API key from the active provider
    Remove {
        #[arg(short, long)]
        name: Option<String>,

        #[arg(long)]
        all: bool,
    },

    /// List API keys for the active provider
    List,

    /// Set chat/completion model
    Model {
        model: String,
    },

    /// Set embedding model
    Embed {
        model: String,
    },

    Provider {
        provider: Option<String>,

        #[arg(long)]
        list: bool,
    },

    Info,
    Update,
}
