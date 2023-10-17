mod cli;
mod legacy_handler;
mod prompt;
mod request;

use crate::legacy_handler::LegacyHandler;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::Cli;
use std::{io, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
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

    if let Err(e) = execute_cli_command(&cli).await {
        if let Some(error) = e.downcast_ref::<reqwest::Error>() {
            println!("{error}");
        } else {
            println!("{e}");
        }
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

async fn execute_cli_command(cli: &Cli) -> anyhow::Result<()> {
    let command = cli.command.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "subcommand must be specified!\n\n{}",
            Cli::command().render_long_help()
        )
    })?;

    let host = url::Host::parse(cli.host.as_ref().expect("host has a default set"))
        .map_err(|_| anyhow::anyhow!("please enter a valid hostname"))?;

    LegacyHandler::new(host.to_string(), cli.json, cli.api_version.unwrap())
        .await?
        .handle_cmd(command)
        .await
}
