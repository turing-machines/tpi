mod cli;
mod legacy_handler;
use crate::legacy_handler::LegacyHandler;
use anyhow::anyhow;
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
        match e.downcast_ref::<reqwest::Error>() {
            Some(error) if error.is_connect() => {
                println!(
                    "Cannot connect to `{}`",
                    error
                        .url()
                        .and_then(|u| u.host_str())
                        .unwrap_or(&cli.host.unwrap())
                );
            }
            _ => println!("{e}"),
        }
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

async fn execute_cli_command(cli: &Cli) -> anyhow::Result<()> {
    let command = cli.command.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "subcommand must be specified!\n\n{}",
            Cli::command().render_help()
        )
    })?;

    // validate host input
    let host = url::Host::parse(cli.host.as_ref().expect("host has a default set"))
        .map_err(|_| anyhow!("please enter a valid hostname"))?;

    LegacyHandler::new(host.to_string(), cli.json)
        .await?
        .handle_cmd(command)
        .await
}
