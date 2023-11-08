// Copyright 2023 Turing Machines
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
    let mut host = host.to_string();
    // connect to specific port if specified.
    if let Some(port) = cli.port {
        host.push_str(&format!(":{}", port));
    }

    LegacyHandler::new(host, cli)?.handle_cmd(command).await
}
