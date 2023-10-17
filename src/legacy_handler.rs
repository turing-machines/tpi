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
    AdvancedArgs, ApiVersion, Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd,
    UartArgs, UsbArgs,
};
use crate::cli::{FlashArgs, UsbCmd};
use crate::request::Request;

use anyhow::{bail, ensure, Context};
use bytes::BytesMut;
use humansize::{format_size, DECIMAL};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use reqwest::multipart::Part;
use reqwest::{Client, ClientBuilder, Version};
use std::fmt::Write;
use std::path::Path;
use std::str::from_utf8;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc::channel;

const DEFAULT_FLOW_CONTROL_WINDOW_SIZE: u64 = 65535;

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

    pub fn new(host: String, json: bool, version: ApiVersion) -> anyhow::Result<Self> {
        let request = Request::new(host, version)?;
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

        let response = self.request.send(&self.client).await?;
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
            .ok_or_else(|| anyhow::anyhow!("expected 'reponse' key in JSON payload"))
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
            self.handle_file_upload_v1_1(&mut file, size).await
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
        if args.local {
            self.request
                .url_mut()
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "flash")
                .append_key_only("local")
                .append_pair("file", &args.image_path.to_string_lossy());
            return Ok(());
        }

        // Opt out of the global request/response handler as we implement an
        // alternative flow here.
        self.skip_request = true;
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
            self.handle_file_upload_v1_1(&mut file, file_size).await
        }
    }

    async fn handle_file_upload_v1(
        &self,
        file: &mut File,
        file_name: String,
    ) -> anyhow::Result<()> {
        println!("Warning: large files will very likely to fail to be uploaded in version 1");

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await?;
        let part = Part::stream(bytes)
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

    async fn handle_file_upload_v1_1(&self, file: &mut File, file_size: u64) -> anyhow::Result<()> {
        let mut req = self.request.clone();
        *req.as_mut().version_mut() = Version::HTTP_2;
        let response = req.send(&self.client).await.context("flash request")?;

        if !response.status().is_success() {
            bail!("could not execute flashing: {}", response.text().await?);
        }

        println!("started transfer of {}..", format_size(file_size, DECIMAL));

        let (sender, mut receiver) = channel::<bytes::Bytes>(256);
        let read_task = async move {
            let mut bytes_read = 0;
            while bytes_read < file_size {
                let read_len: usize = DEFAULT_FLOW_CONTROL_WINDOW_SIZE
                    .min(file_size - bytes_read)
                    .try_into()?;
                let mut buffer = BytesMut::zeroed(read_len);
                let read = file.read(&mut buffer).await?;
                if 0 == read {
                    // end_of_file
                    break;
                }

                bytes_read += read as u64;
                buffer.truncate(read);
                sender.send(buffer.into()).await?;
            }
            Ok(())
        };

        let send_task = async move {
            // try to keep the additional header sizes as low as possible within
            // the constrains of the chosen legacy API format (with queries).
            let host = self.request.url().host().expect("request has no host");
            let mut post_req = Request::new_post(host.to_string(), self.version)?;

            *post_req.as_mut().version_mut() = Version::HTTP_2;

            post_req
                .url_mut()
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "flash");

            let pb = build_progress_bar(file_size);
            let mut bytes_send = 0u64;
            while let Some(bytes) = receiver.recv().await {
                bytes_send += bytes.len() as u64;
                let mut req = post_req.clone();
                *req.as_mut().body_mut() = Some(reqwest::Body::from(bytes));
                let rsp = req.send(&self.client).await?;

                pb.set_position(bytes_send);

                if !rsp.status().is_success() {
                    bail!("{}", rsp.text().await.unwrap());
                }
            }
            pb.finish();
            println!("finished uploading. awaiting bmc..");
            Ok(())
        };

        // Sending task runs decoupled from the reading task for 2 reasons:
        // * To spend as much time as possible sending data over the TCP
        // socket.
        // * Buffering of data smooths out any hick ups in reading or sending
        // data. This comes with a small memory penalty, tune [BUFFER_SIZE] if
        // your target platform is memory constrained.
        tokio::try_join!(read_task, send_task).map(|_| ())
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
                let response = self.request.clone().send(&self.client).await?;

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
                let response = self.request.clone().send(&self.client).await?;

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
