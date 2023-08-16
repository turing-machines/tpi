mod cli;
mod legacy_handler;
use crate::legacy_handler::LegacyHandler;
use anyhow::anyhow;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::Cli;
use log::LevelFilter;
use simple_logger::SimpleLogger;
use std::{io, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
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
    if let Some(shell) = cli.gencompletion {
        generate(
            shell,
            &mut Cli::command(),
            env!("CARGO_PKG_NAME"),
            &mut io::stdout(),
        );
        return ExitCode::SUCCESS;
    }

    if let Err(e) = execute_cli_command(cli).await {
        println!("{e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

async fn execute_cli_command(cli: Cli) -> anyhow::Result<()> {
    let command = cli.command.ok_or_else(|| {
        anyhow::anyhow!(
            "subcommand must be specified!\n\n{}",
            Cli::command().render_help()
        )
    })?;

    // validate host input
    let host = url::Host::parse(&cli.host.expect("host has a default set"))
        .map_err(|_| anyhow!("please enter a valid hostname"))?;
    LegacyHandler::new(host.to_string(), cli.json)
        .await?
        .handle_cmd(command)
        .await
}
