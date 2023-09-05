use crate::cli::{Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd, UartArgs, UsbArgs};
use crate::cli::{FlashArgs, UsbCmd};
use anyhow::{bail, Context};
use anyhow::{ensure, Ok};
use bytes::BytesMut;
use reqwest::{Client, Method, RequestBuilder};
use reqwest::{ClientBuilder, Request};
use tokio::fs::OpenOptions;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::channel;
use url::Url;

const DEFAULT_FLOW_CONRTOL_WINDOW_SIZE: u64 = 65535;

type ResponsePrinter = fn(&serde_json::Value) -> anyhow::Result<()>;

pub struct LegacyHandler {
    request: Request,
    response_printer: Option<ResponsePrinter>,
    json: bool,
    skip_request: bool,
}

impl LegacyHandler {
    fn url_from_host(host: String) -> anyhow::Result<Url> {
        let mut url = Url::parse(&format!("http://{}", host))?;
        url.set_path("api/bmc");
        Ok(url)
    }

    pub async fn new(host: String, json: bool) -> anyhow::Result<Self> {
        let url = Self::url_from_host(host)?;
        Ok(Self {
            request: Request::new(Method::GET, url),
            response_printer: None,
            json,
            skip_request: false,
        })
    }

    /// Handler for CLI commands. Responses are printed to stdout and need to be formatted
    /// using the json format with a key `response`.
    pub async fn handle_cmd(mut self, command: &Commands) -> anyhow::Result<()> {
        match command {
            Commands::Power(args) => self.handle_power_nodes(args)?,
            Commands::Usb(args) => self.handle_usb(args)?,
            Commands::Firmware(args) => self.handle_firmware(args).await?,
            Commands::Flash(args) => self.handle_flash(args).await?,
            Commands::Eth(args) => self.handle_eth(args)?,
            Commands::Uart(args) => self.handle_uart(args)?,
        }

        if self.skip_request {
            return Ok(());
        }

        let response = Client::new()
            .execute(self.request)
            .await
            .context("http request error")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "{}:\n{}",
                status.canonical_reason().unwrap_or("unknown reason"),
                response
                    .text()
                    .await
                    .unwrap_or("error parsing server response".to_string())
            );
        }

        let body = response
            .json::<serde_json::Value>()
            .await
            .context("json respones parse error")?;

        if self.json {
            println!("{}", &body.to_string());
            return Ok(());
        }

        body.get("response")
            .ok_or(anyhow::anyhow!("expected 'reponse' key in json payload"))
            .map(|response| {
                let extracted = &response.as_array().unwrap()[0];
                let default_print = || {
                    // In this case there is no printer set, fallback on
                    // printing the http response body as text.
                    println!("{}", extracted);
                };

                self.response_printer.map_or_else(default_print, |f| {
                    if let Err(e) = f(&extracted) {
                        default_print();
                        println!("{}", e);
                    }
                })
            })
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

    async fn handle_firmware(&mut self, _: &FirmwareArgs) -> anyhow::Result<()> {
        bail!("`firmware` argument not implemented yet!");
    }

    async fn handle_flash(&mut self, args: &FlashArgs) -> anyhow::Result<()> {
        if cfg!(feature = "local-only") {
            self.request
                .url_mut()
                .query_pairs_mut()
                .append_pair("file", &args.image_path.to_string_lossy());
            return Ok(());
        }

        #[cfg(not(feature = "local-only"))]
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

        let mut file = OpenOptions::new().read(true).open(&args.image_path).await?;
        let file_size = file.metadata().await?.len();
        let file_name = args
            .image_path
            .file_name()
            .ok_or(anyhow::anyhow!("file_name could not be extracted"))?
            .to_string_lossy()
            .to_string();

        self.request
            .url_mut()
            .query_pairs_mut()
            .append_pair("opt", "set")
            .append_pair("type", "flash")
            .append_pair("file", &file_name)
            .append_pair("length", &file_size.to_string())
            .append_pair("node", &(args.node - 1).to_string());

        let client = ClientBuilder::new().gzip(true).build()?;

        let response =
            RequestBuilder::from_parts(client.clone(), self.request.try_clone().unwrap())
                // .version(Version::HTTP_2)
                .send()
                .await
                .context("flash request")?;

        if !response.status().is_success() {
            bail!("could not execute flashing :{}", response.text().await?);
        }

        let (sender, mut receiver) =
            channel::<bytes::Bytes>(DEFAULT_FLOW_CONRTOL_WINDOW_SIZE as usize);

        let read_task = async move {
            let mut bytes_read = 0;
            while bytes_read < file_size {
                let read_len: usize = DEFAULT_FLOW_CONRTOL_WINDOW_SIZE
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
            // the constrains of the chosen legacy api format (with queries).
            let mut url = Self::url_from_host(self.request.url().host().unwrap().to_string())?;
            url.query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "flash");

            while let Some(bytes) = receiver.recv().await {
                let rsp = RequestBuilder::from_parts(
                    client.clone(),
                    Request::new(Method::POST, url.clone()),
                )
                // .version(Version::HTTP_2)
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
        // * To spend as much time as possible sending data over the tcp
        // socket.
        // * Buffering of data smooths out any hick ups in reading or sending
        // data. This comes with a small memory penalty, tune [BUFFER_SIZE] if
        // your target platform is memory constrained.
        tokio::try_join!(read_task, send_task).map(|_| ())
    }

    fn handle_usb(&mut self, args: &UsbArgs) -> anyhow::Result<()> {
        if args.bmc {
            bail!("--bmc argument not implemented yet!");
        }

        let mut serializer = self.request.url_mut().query_pairs_mut();
        if args.mode == UsbCmd::Status {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "usb");
            self.response_printer = Some(print_usb_status);
            return Ok(());
        }

        serializer
            .append_pair("opt", "set")
            .append_pair("type", "usb")
            .append_pair("node", &(args.node - 1).to_string());

        if args.mode == UsbCmd::Host {
            serializer.append_pair("mode", "0");
        } else {
            serializer.append_pair("mode", "1");
        }

        if args.usb_boot {
            serializer.append_pair("boot_pin", "1");
        }

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
                .append_pair("node", &args.node.unwrap().to_string());
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
}

fn print_power_status_nodes(map: &serde_json::Value) -> anyhow::Result<()> {
    let results = map
        .get("result")
        .context("api error")?
        .as_array()
        .context("api error")?[0]
        .as_object()
        .context("response parse error")?;

    for (key, value) in results {
        let number = value.as_str().context("api error")?.parse::<u8>()?;
        let status = if number == 1 { "On" } else { "off" };
        println!("{}: {}", key, status);
    }

    Ok(())
}

fn result_printer(result: &serde_json::Value) -> anyhow::Result<()> {
    let res = result.get("result").context("api error")?;
    println!("{}", res.as_str().context("api error")?);
    Ok(())
}

fn print_usb_status(map: &serde_json::Value) -> anyhow::Result<()> {
    let results = map
        .get("result")
        .context("api error")?
        .as_array()
        .context("api error")?[0]
        .as_object()
        .context("response parse error")?;

    println!(
        "Usb bus is routed to {} and acting as a USB {}",
        results["node"].as_str().unwrap().to_lowercase(),
        results["mode"].as_str().unwrap().to_lowercase()
    );

    Ok(())
}
