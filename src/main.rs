mod cli;
mod engine;
mod lang;
mod mcp;
mod sdd;

use clap::Parser;

/// Top-level CLI for tokenwise.
#[derive(Parser, Debug)]
#[command(
    name = "tokenwise",
    version,
    about = "tokenwise – Rust code intelligence engine and MCP server"
)]
struct Cli {
    /// Run as MCP server instead of human CLI.
    #[arg(long)]
    mcp_server: bool,

    /// Optional subcommand for human-facing CLI.
    #[command(subcommand)]
    command: Option<cli::Command>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(first_arg) = raw_args.get(1) {
        if first_arg.starts_with("/tw:") || first_arg.starts_with("/yoyo:") {
            cli::run_slash_command(raw_args[1..].to_vec()).await?;
            return Ok(());
        }
    }

    let cli = Cli::parse();

    if cli.mcp_server {
        mcp::run_stdio_server().await?;
    } else {
        cli::run(cli.command).await?;
    }

    Ok(())
}
