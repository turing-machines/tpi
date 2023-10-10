use crate::cli::{
    AdvancedArgs, ApiVersion, Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd,
    UartArgs, UsbArgs,
};
use crate::cli::{FlashArgs, UsbCmd};
use crate::utils::{ProgressPrinter, PROGRESS_REPORT_PERCENT};
use anyhow::{bail, Context};
use anyhow::{ensure, Ok};
use bytes::BytesMut;
use reqwest::multipart::Part;
use reqwest::{Client, Method, RequestBuilder, Version};
use reqwest::{ClientBuilder, Request};
use std::path::Path;
use std::str::from_utf8;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::channel;
use tokio::time::Instant;
use url::Url;
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
    fn url_from_host(host: String, scheme: &str) -> anyhow::Result<Url> {
        let mut url = Url::parse(&format!("{}://{}", scheme, host))?;
        url.set_path("api/bmc");
        Ok(url)
    }

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

    pub async fn new(host: String, json: bool, version: ApiVersion) -> anyhow::Result<Self> {
        let url = Self::url_from_host(host, version.scheme())?;
        let client = Self::create_client(version)?;
        Ok(Self {
            request: Request::new(Method::GET, url),
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

        let response = self
            .client
            .execute(self.request)
            .await
            .context("http request error")?;
        let status = response.status();
        let bytes = response.bytes().await?;

        let body: serde_json::Value = match serde_json::from_slice(&bytes) {
            core::result::Result::Ok(b) => b,
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
            .ok_or(anyhow::anyhow!("expected 'reponse' key in JSON payload"))
            .map(|response| {
                let extracted = &response.as_array().unwrap()[0];
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
                })
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
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .await
            .with_context(|| format!("cannot open file {}", path.to_string_lossy()))?;

        let file_size = file.metadata().await?.len();
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
        let response =
            RequestBuilder::from_parts(self.client.clone(), self.request.try_clone().unwrap())
                .version(Version::HTTP_2)
                .send()
                .await
                .context("flash request")?;

        if !response.status().is_success() {
            bail!("could not execute flashing: {}", response.text().await?);
        }

        println!("started transfer of {} MiB ..", file_size / 1024 / 1024);

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
            let mut url = Self::url_from_host(
                self.request.url().host().unwrap().to_string(),
                self.version.scheme(),
            )?;
            url.query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "flash");

            let mut progress_printer =
                ProgressPrinter::new(file_size, Instant::now(), PROGRESS_REPORT_PERCENT);

            while let Some(bytes) = receiver.recv().await {
                progress_printer.update_progress(bytes.len());
                let rsp = RequestBuilder::from_parts(
                    self.client.clone(),
                    Request::new(Method::POST, url.clone()),
                )
                .version(Version::HTTP_2)
                .body(bytes)
                .send()
                .await?;

                if !rsp.status().is_success() {
                    bail!("{}", rsp.text().await.unwrap());
                }
            }
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

        let Some(node) = args.node else { bail!("`--node` argument missing") };

        serializer
            .append_pair("opt", "set")
            .append_pair("type", "usb")
            .append_pair("node", &(node - 1).to_string());

        let mut mode = if args.mode == UsbCmd::Host { 0 } else { 1 };
        mode |= (args.bmc as u8) << 1;
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
                let response = RequestBuilder::from_parts(
                    self.client.clone(),
                    self.request.try_clone().unwrap(),
                )
                .send()
                .await?;

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
                let response = RequestBuilder::from_parts(
                    self.client.clone(),
                    self.request.try_clone().unwrap(),
                )
                .send()
                .await?;

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
