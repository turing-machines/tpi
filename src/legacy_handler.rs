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

use crate::cli::{
    AdvancedArgs, ApiVersion, Cli, Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd,
    UartArgs, UsbArgs,
};
use crate::cli::{FlashArgs, UsbCmd};
use crate::request::Request;
use anyhow::{bail, ensure, Context};
use indicatif::{HumanBytes, ProgressBar, ProgressState, ProgressStyle};
use reqwest::multipart::Part;
use reqwest::{Body, Client, ClientBuilder};
use std::fmt::Write;
use std::path::Path;
use std::str::from_utf8;
use std::time::Duration;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::sleep;
use tokio::{spawn, task::JoinHandle};
use tokio_util::io::ReaderStream;

type ResponsePrinter = fn(&serde_json::Value) -> anyhow::Result<()>;

pub struct LegacyHandler {
    request: Request,
    client: Client,
    response_printer: Option<ResponsePrinter>,
    json: bool,
    skip_request: bool,
    version: ApiVersion,
}

impl LegacyHandler {
    fn create_client(version: ApiVersion) -> anyhow::Result<Client> {
        if version == ApiVersion::V1 {
            return Ok(Client::new());
        }

        let client = ClientBuilder::new()
            .gzip(true)
            .danger_accept_invalid_certs(true)
            .http2_prior_knowledge()
            .https_only(true)
            .use_rustls_tls()
            .build()?;
        Ok(client)
    }

    pub fn new(host: String, args: &Cli) -> anyhow::Result<Self> {
        let json = args.json;
        let version = args.api_version.expect("Missing API version");
        let creds = (args.user.clone(), args.password.clone());
        let request = Request::new(host, version, creds)?;
        let client = Self::create_client(version)?;

        Ok(Self {
            request,
            client,
            response_printer: None,
            json,
            skip_request: false,
            version,
        })
    }

    /// Handler for CLI commands. Responses are printed to stdout and need to be formatted
    /// using the JSON format with a key `response`.
    pub async fn handle_cmd(mut self, command: &Commands) -> anyhow::Result<()> {
        match command {
            Commands::Power(args) => self.handle_power_nodes(args)?,
            Commands::Usb(args) => self.handle_usb(args)?,
            Commands::Firmware(args) => self.handle_firmware(args).await?,
            Commands::Flash(args) => self.handle_flash(args).await?,
            Commands::Eth(args) => self.handle_eth(args)?,
            Commands::Uart(args) => self.handle_uart(args)?,
            Commands::Advanced(args) => self.handle_advanced(args).await?,
            Commands::Info => self.handle_info(),
        }

        if self.skip_request {
            return Ok(());
        }

        let response = self.request.send(self.client).await?;
        let status = response.status();
        let bytes = response.bytes().await?;

        let body: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(b) => b,
            Err(_) => bail!(
                "{}:\n{}",
                status.canonical_reason().unwrap_or("unknown reason"),
                from_utf8(&bytes).unwrap_or("error parsing server response")
            ),
        };

        if self.json {
            println!("{}", &body.to_string());
            return Ok(());
        }

        body.get("response")
            .ok_or_else(|| anyhow::anyhow!("expected 'response' key in JSON payload"))
            .map(|response| {
                let extracted = response
                    .as_array()
                    .unwrap_or_else(|| panic!("API error: `response` is not an array"))
                    .first()
                    .unwrap_or_else(|| panic!("API error: `response` is empty"));
                let default_print = || {
                    // In this case there is no printer set, fallback on
                    // printing the http response body as text.
                    println!("{}", extracted);
                };

                self.response_printer.map_or_else(default_print, |f| {
                    if let Err(e) = f(extracted) {
                        default_print();
                        println!("{}", e);
                    }
                });
            })
    }

    fn handle_info(&mut self) {
        self.request
            .url_mut()
            .query_pairs_mut()
            .append_pair("opt", "get")
            .append_pair("type", "other");

        self.response_printer = Some(info_printer);
    }

    fn handle_uart(&mut self, args: &UartArgs) -> anyhow::Result<()> {
        let mut serializer = self.request.url_mut().query_pairs_mut();
        if args.action == GetSet::Get {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "uart")
                .append_pair("node", &(args.node - 1).to_string());
        } else {
            ensure!(
                args.cmd.is_some(),
                "uart set command requires `--cmd` argument."
            );
            serializer
                .append_pair("opt", "set")
                .append_pair("type", "uart")
                .append_pair("node", &(args.node - 1).to_string())
                .append_pair("cmd", args.cmd.as_ref().unwrap());
            self.response_printer = Some(result_printer);
        }
        Ok(())
    }

    fn handle_eth(&mut self, args: &EthArgs) -> anyhow::Result<()> {
        if args.reset {
            self.request
                .url_mut()
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "network")
                .append_pair("cmd", "reset");
        } else {
            bail!("eth subcommand called without any actions");
        }

        self.response_printer = Some(result_printer);
        Ok(())
    }

    async fn handle_firmware(&mut self, args: &FirmwareArgs) -> anyhow::Result<()> {
        let (mut file, file_name, size) = Self::open_file(&args.file).await?;
        if self.version == ApiVersion::V1 {
            // Opt out of the global request/response handler as we implement an
            // alternative flow here.
            self.skip_request = true;
            self.request
                .url_mut()
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "firmware")
                .append_pair("file", &file_name);
            self.handle_file_upload_v1(&mut file, file_name).await
        } else {
            self.skip_request = true;
            self.request
                .url_mut()
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "firmware")
                .append_pair("file", &file_name)
                .append_pair("length", &size.to_string());
            self.handle_file_upload_v1_1(file, size).await
        }
    }

    async fn open_file(path: &Path) -> anyhow::Result<(File, String, u64)> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(path)
            .await
            .with_context(|| format!("cannot open file {}", path.to_string_lossy()))?;

        let file_size = file.seek(std::io::SeekFrom::End(0)).await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;

        let file_name = path
            .file_name()
            .ok_or(anyhow::anyhow!("file_name could not be extracted"))?
            .to_string_lossy()
            .to_string();
        Ok((file, file_name, file_size))
    }

    async fn handle_flash(&mut self, args: &FlashArgs) -> anyhow::Result<()> {
        // Opt out of the global request/response handler as we implement an alternative flow here.
        self.skip_request = true;

        if args.local {
            return self.handle_local_file_upload(args).await;
        }

        let (mut file, file_name, file_size) = Self::open_file(&args.image_path).await?;
        println!("request flashing of {file_name} to node {}", args.node);

        self.request
            .url_mut()
            .query_pairs_mut()
            .append_pair("opt", "set")
            .append_pair("type", "flash")
            .append_pair("file", &file_name)
            .append_pair("length", &file_size.to_string())
            .append_pair("node", &(args.node - 1).to_string());

        if self.version == ApiVersion::V1 {
            self.handle_file_upload_v1(&mut file, file_name).await
        } else {
            self.handle_file_upload_v1_1(file, file_size).await
        }
    }

    async fn handle_local_file_upload(&mut self, args: &FlashArgs) -> anyhow::Result<()> {
        self.request
            .url_mut()
            .query_pairs_mut()
            .append_pair("opt", "set")
            .append_pair("type", "flash")
            .append_key_only("local")
            .append_pair("file", &args.image_path.to_string_lossy())
            .append_pair("node", &(args.node - 1).to_string());

        let response = self.request.clone().send(self.client.clone()).await?;
        let status = response.status();
        let json_res = response.json::<serde_json::Value>().await;

        if !status.is_success() {
            if let Ok(json) = &json_res {
                if let Some(err) = json.get("response") {
                    println!("Error: {}", err);
                }
            }
            bail!("Failed to begin flashing: {}", status);
        }

        let handle_id = get_json_num(&json_res?, "handle");
        let (_, _, file_size) = Self::open_file(&args.image_path).await?;

        println!("Flashing from image file {}...", args.image_path.display());

        let progress_watcher = self.create_progress_watching_thread(file_size, handle_id);

        tokio::try_join!(progress_watcher).expect("failed to wait for thread");

        Ok(())
    }

    fn create_progress_watching_thread(&self, file_size: u64, handle_id: u64) -> JoinHandle<()> {
        let initial_delay = Duration::from_secs(3);
        let update_period = Duration::from_millis(2500);

        let client = self.client.clone();
        let mut req = self.request.clone();

        req.url_mut()
            .query_pairs_mut()
            .clear()
            .append_pair("opt", "get")
            .append_pair("type", "flash");

        spawn(async move {
            let bar = build_progress_bar(file_size);
            let mut verifying = false;

            sleep(initial_delay).await;

            loop {
                let response = req
                    .clone()
                    .send(client.clone())
                    .await
                    .expect("Failed to send progress status request");

                let status = response.status();
                let json = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("Failed to parse response as JSON");

                if !status.is_success() {
                    if let Some(err) = json.get("response") {
                        println!("Error: {}", err);
                    }
                    panic!("Failed to get flashing progress: {}", status);
                }

                if let Some(map) = json.get("Transferring") {
                    let id = get_json_num(map, "id");
                    assert_eq!(id, handle_id, "Invalid flashing handle");

                    let bytes_written = get_json_num(map, "bytes_written");

                    if bytes_written == file_size {
                        if !verifying {
                            bar.finish_and_clear();
                            println!("Verifying checksum...");
                            verifying = true;
                        }
                    } else {
                        bar.set_position(bytes_written);
                    }

                    sleep(update_period).await;
                    continue;
                }

                if json.get("Done").is_some() {
                    println!("Done");
                    break;
                }

                if let Some(map) = json.get("Error") {
                    panic!("Error occured during flashing: {}", map);
                }

                panic!("Unexpected response: {:#?}", json);
            }
        })
    }

    async fn handle_file_upload_v1(
        &self,
        file: &mut File,
        file_name: String,
    ) -> anyhow::Result<()> {
        println!("Warning: large files will very likely to fail to be uploaded in version 1");

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await?;
        let part = Part::bytes(bytes)
            .mime_str("application/octet-stream")?
            .file_name(file_name);
        let form = reqwest::multipart::Form::new().part("file", part);
        self.client
            .post(self.request.url().clone())
            .multipart(form)
            .send()
            .await?;
        Ok(())
    }

    async fn handle_file_upload_v1_1(&self, file: File, file_size: u64) -> anyhow::Result<()> {
        let req = self.request.clone();
        let response = req
            .send(self.client.clone())
            .await
            .context("flash request")?;

        if !response.status().is_success() {
            bail!("could not execute flashing: {}", response.text().await?);
        }

        let json: serde_json::Value = response.json().await?;
        let handle = json["handle"].as_u64().unwrap_or_default();

        println!("started transfer of {}..", HumanBytes(file_size));
        let pb = build_progress_bar(file_size);
        let stream = ReaderStream::new(pb.wrap_async_write(file));
        let stream_part =
            reqwest::multipart::Part::stream_with_length(Body::wrap_stream(stream), file_size)
                .mime_str("application/octet-stream")?;

        let mut multipart_request = self.request.to_post()?;
        multipart_request
            .url_mut()
            .path_segments_mut()
            .unwrap()
            .push("upload")
            .push(&handle.to_string());

        let form = reqwest::multipart::Form::new().part("file", stream_part);
        multipart_request.set_multipart(form);
        multipart_request.send(self.client.clone()).await?;

        let mut request = self.request.clone();
        request
            .url_mut()
            .query_pairs_mut()
            .append_pair("opt", "get")
            .append_pair("type", "flash");

        let status = request
            .send(self.client.clone())
            .await
            .context("flash request")?;

        println!("{}", status.text().await?);
        Ok(())
    }

    fn handle_usb(&mut self, args: &UsbArgs) -> anyhow::Result<()> {
        let mut serializer = self.request.url_mut().query_pairs_mut();
        if args.mode == UsbCmd::Status {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "usb");
            self.response_printer = Some(print_usb_status);
            return Ok(());
        }

        let Some(node) = args.node else {
            bail!("`--node` argument missing")
        };

        serializer
            .append_pair("opt", "set")
            .append_pair("type", "usb")
            .append_pair("node", &(node - 1).to_string());

        let mut mode = if args.mode == UsbCmd::Host { 0 } else { 1 };
        mode |= u8::from(args.bmc) << 1;
        serializer.append_pair("mode", &mode.to_string());

        self.response_printer = Some(result_printer);
        Ok(())
    }

    fn handle_power_nodes(&mut self, args: &PowerArgs) -> anyhow::Result<()> {
        let mut serializer = self.request.url_mut().query_pairs_mut();
        if args.cmd == PowerCmd::Get {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "power");
            self.response_printer = Some(print_power_status_nodes);
            return Ok(());
        } else if args.cmd == PowerCmd::Reset {
            ensure!(args.node.is_some(), "`--node` argument must be set.");
            serializer
                .append_pair("opt", "set")
                .append_pair("type", "reset")
                .append_pair("node", &(args.node.unwrap() - 1).to_string());
            self.response_printer = Some(result_printer);
            return Ok(());
        }

        serializer
            .append_pair("opt", "set")
            .append_pair("type", "power");

        let on_bit = if args.cmd == PowerCmd::On { "1" } else { "0" };

        if let Some(node) = args.node {
            serializer.append_pair(&format!("node{}", node), on_bit);
        } else {
            serializer.append_pair("node1", on_bit);
            serializer.append_pair("node2", on_bit);
            serializer.append_pair("node3", on_bit);
            serializer.append_pair("node4", on_bit);
        }
        self.response_printer = Some(result_printer);
        Ok(())
    }

    async fn handle_advanced(&mut self, args: &AdvancedArgs) -> anyhow::Result<()> {
        match args.mode {
            crate::cli::ModeCmd::Normal => {
                self.request
                    .url_mut()
                    .query_pairs_mut()
                    .append_pair("opt", "set")
                    .append_pair("type", "clear_usb_boot")
                    .append_pair("node", &(args.node - 1).to_string());
                let response = self.request.clone().send(self.client.clone()).await?;

                if !response.status().is_success() {
                    bail!("could not execute Normal mode: {}", response.text().await?);
                }

                return self.handle_power_nodes(&PowerArgs {
                    cmd: PowerCmd::Reset,
                    node: Some(args.node),
                });
            }
            crate::cli::ModeCmd::Msd => {
                self.request
                    .url_mut()
                    .query_pairs_mut()
                    .append_pair("opt", "set")
                    .append_pair("type", "node_to_msd")
                    .append_pair("node", &(args.node - 1).to_string());
            }
            crate::cli::ModeCmd::Recovery => {
                self.request
                    .url_mut()
                    .query_pairs_mut()
                    .append_pair("opt", "set")
                    .append_pair("type", "usb_boot")
                    .append_pair("node", &(args.node - 1).to_string());
                let response = self.request.clone().send(self.client.clone()).await?;

                if !response.status().is_success() {
                    bail!(
                        "could not execute Recovery mode: {}",
                        response.text().await?
                    );
                }

                return self.handle_power_nodes(&PowerArgs {
                    cmd: PowerCmd::Reset,
                    node: Some(args.node),
                });
            }
        }
        self.response_printer = Some(result_printer);

        Ok(())
    }
}

fn print_power_status_nodes(map: &serde_json::Value) -> anyhow::Result<()> {
    let results = map
        .get("result")
        .context("API error")?
        .as_array()
        .context("API error")?[0]
        .as_object()
        .context("response parse error")?;

    for (key, value) in results {
        let number = value.as_str().context("API error")?.parse::<u8>()?;
        let status = if number == 1 { "On" } else { "off" };
        println!("{}: {}", key, status);
    }

    Ok(())
}

fn result_printer(result: &serde_json::Value) -> anyhow::Result<()> {
    let res = result.get("result").context("API error")?;
    println!("{}", res.as_str().context("API error")?);
    Ok(())
}

fn info_printer(map: &serde_json::Value) -> anyhow::Result<()> {
    let results = map
        .get("result")
        .context("API error")?
        .as_array()
        .context("API error")?[0]
        .as_object()
        .context("response parse error")?;

    println!("|{:-^10}|{:-^28}|", "key", "value");
    for (key, value) in results {
        println!(" {:<10}: {}", key, value.as_str().expect("API error"));
    }
    println!("|{:-^10}|{:-^28}|", "", "");
    Ok(())
}

fn print_usb_status(map: &serde_json::Value) -> anyhow::Result<()> {
    let results = map
        .get("result")
        .context("API error")?
        .as_array()
        .context("API error")?[0]
        .as_object()
        .context("response parse error")?;

    let node = results["node"]
        .as_str()
        .expect("API error: Expected `node` attribute")
        .to_lowercase();
    let mode = results["mode"]
        .as_str()
        .expect("API error: Expected `mode` attribute")
        .to_lowercase();
    let route = results["route"]
        .as_str()
        .expect("API error: Expected `mode` attribute")
        .to_lowercase();

    println!("{:^12}-->{:^12}", "USB Host", "USB Device");

    let (host, device) = if mode == "Host" {
        (node, route)
    } else {
        (route, node)
    };

    println!("{:^12}-->{:^12}", host, device);

    Ok(())
}

fn build_progress_bar(size: u64) -> ProgressBar {
    let pb = ProgressBar::new(size);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap();
        })
        .progress_chars("#>-"),
    );
    pb
}

fn get_json_num(map: &serde_json::Value, key: &str) -> u64 {
    map.get(key)
        .unwrap_or_else(|| panic!("API error: expected `{}` key", key))
        .as_u64()
        .unwrap_or_else(|| panic!("API error: `{}` is not a number", key))
}
