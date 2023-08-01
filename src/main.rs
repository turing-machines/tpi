mod cli;
mod legacy_handler;
use crate::legacy_handler::LegacyHandler;
use anyhow::anyhow;
use clap::Parser;
use cli::Cli;
use log::LevelFilter;
use simple_logger::SimpleLogger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let level = if cfg!(debug_assertions) {
        LevelFilter::Trace
    } else {
        LevelFilter::Info
    };

    SimpleLogger::new()
        .with_level(level)
        .with_colors(true)
        .env()
        .init()
        .expect("failed to initialize logger");

    let cli = Cli::parse();

    // validate host input
    let host = url::Host::parse(&cli.host.expect("host has a default set"))
        .map_err(|_| anyhow!("please enter a valid hostname"))?;

    LegacyHandler::new(host.to_string())
        .await?
        .handle_cmd(cli.command)
        .await
}
