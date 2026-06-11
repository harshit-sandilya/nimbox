use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "nimbox",
    version,
    about = "NVIDIA NIM local compatibility proxy"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start Nimbox proxy server
    Start {
        #[arg(short, long, default_value_t = 11434)]
        port: u16,
    },

    /// Stop Nimbox proxy server
    Stop,

    /// Add an NVIDIA API key
    Add {
        #[arg(short, long)]
        name: String,

        key: String,
    },

    /// Remove an NVIDIA API key
    Remove {
        #[arg(short, long)]
        name: Option<String>,

        #[arg(long)]
        all: bool,
    },

    /// List configured API keys
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
