use crate::cli::{Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd, UartArgs, UsbArgs};
use crate::cli::{FlashArgs, UsbCmd};
use anyhow::{bail, Context};
use anyhow::{ensure, Ok};
use reqwest::{
    multipart::{Form, Part},
    Client, Method,
};
use url::Url;

pub struct LegacyHandler {
    url: Url,
    json: bool,
    response_printer: Option<fn(&serde_json::Value)>,
}

impl LegacyHandler {
    pub async fn new(host: String, json: bool) -> anyhow::Result<Self> {
        let mut url = Url::parse(&format!("http://{}", host))?;
        url.set_path("api/bmc");
        Ok(Self {
            url,
            json,
            response_printer: None,
        })
    }

    /// Handler for CLI commands. Responses are printed to stdout and need to be formatted
    /// using the json format with a key `response`.
    pub async fn handle_cmd(mut self, command: &Commands) -> anyhow::Result<()> {
        let form = match command {
            Commands::Power(args) => self.handle_power_nodes(args)?,
            Commands::Usb(args) => self.handle_usb(args)?,
            Commands::Firmware(args) => self.handle_firmware(args).await?,
            Commands::Flash(args) => self.handle_flash(args).await?,
            Commands::Eth(args) => self.handle_eth(args)?,
            Commands::Uart(args) => self.handle_uart(args)?,
        };

        let request = if let Some(form) = form {
            Client::new().post(self.url).multipart(form)
        } else {
            Client::new().request(Method::GET, self.url)
        };

        let response = request.send().await.context("http request error")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "request unsuccessful: {}",
                status.canonical_reason().unwrap_or("unknown reason")
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
            .ok_or(anyhow::anyhow!("expected 'reponse' key in json"))
            .map(|response_key| {
                self.response_printer.map_or_else(
                    || {
                        // In this case there is no printer set, fallback on
                        // printing the http response body as text.
                        println!("{}", response_key);
                    },
                    |f| f(response_key),
                )
            })
    }

    fn handle_uart(&mut self, args: &UartArgs) -> anyhow::Result<Option<Form>> {
        let mut serializer = self.url.query_pairs_mut();
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
        }
        Ok(None)
    }

    fn handle_eth(&mut self, args: &EthArgs) -> anyhow::Result<Option<Form>> {
        if args.reset {
            self.url
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "network")
                .append_pair("cmd", "reset");
        } else {
            bail!("eth subcommand called without any actions");
        }
        Ok(None)
    }

    async fn handle_firmware(&mut self, args: &FirmwareArgs) -> anyhow::Result<Option<Form>> {
        let file_name = args
            .file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let bytes = tokio::fs::read(&args.file).await?;
        let part = Part::stream(bytes)
            .mime_str("application/octet-stream")?
            .file_name(file_name);
        Ok(Some(reqwest::multipart::Form::new().part("file", part)))
    }

    async fn handle_flash(&mut self, args: &FlashArgs) -> anyhow::Result<Option<Form>> {
        let mut serializer = self.url.query_pairs_mut();

        #[cfg(feature = "local-only")]
        {
            serializer.append_pair("file", &args.image_path.to_string_lossy());
            return Ok(None);
        }

        serializer
            .append_pair("opt", "set")
            .append_pair("type", "flash")
            .append_pair("node", &(args.node - 1).to_string());

        #[cfg(not(feature = "local-only"))]
        if args.local {
            serializer.append_pair("file", &args.image_path.to_string_lossy());
            Ok(None)
        } else {
            println!("Warning: large files will very likely to fail to be uploaded in the current version");
            let file_name = args
                .image_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let bytes = tokio::fs::read(&args.image_path).await?;
            let part = Part::stream(bytes)
                .mime_str("application/octet-stream")?
                .file_name(file_name);
            Ok(Some(reqwest::multipart::Form::new().part("file", part)))
        }
    }

    fn handle_usb(&mut self, args: &UsbArgs) -> anyhow::Result<Option<Form>> {
        if args.bmc {
            bail!("--bmc argument not implemented yet!");
        }

        let mut serializer = self.url.query_pairs_mut();
        if args.mode == UsbCmd::Status {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "usb");
            return Ok(None);
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
        Ok(None)
    }

    fn handle_power_nodes(&mut self, args: &PowerArgs) -> anyhow::Result<Option<Form>> {
        let mut serializer = self.url.query_pairs_mut();
        if args.cmd == PowerCmd::Get {
            serializer
                .append_pair("opt", "get")
                .append_pair("type", "power");
            return Ok(None);
        } else if args.cmd == PowerCmd::Reset {
            ensure!(args.node.is_some(), "`--node` argument must be set.");
            serializer
                .append_pair("opt", "set")
                .append_pair("type", "reset")
                .append_pair("node", &args.node.unwrap().to_string());
            return Ok(None);
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
        Ok(None)
    }
}
