use crate::cli::{Commands, EthArgs, FirmwareArgs, GetSet, PowerArgs, PowerCmd, UartArgs, UsbArgs};
use crate::cli::{FlashArgs, UsbCmd};
use anyhow::Context;
use anyhow::{anyhow, ensure, Ok};
use reqwest::{
    multipart::{Form, Part},
    Client, Method,
};
use url::Url;

macro_rules! dispatch_cmd {
   ($self: expr, $command_instance:ident, $($cmd:ident -> $handler:ident),+) => {
       match $command_instance {
           $(Commands::$cmd(args) => {
               $self.$handler(args).await?
           })*
       }
   }
}

pub struct LegacyHandler {
    url: Url,
}

impl LegacyHandler {
    pub async fn new(host: String) -> anyhow::Result<Self> {
        let mut url = Url::parse(&format!("http://{}", host))?;
        url.set_path("api/bmc");
        Ok(Self { url })
    }

    /// Simple handler for CLI commands. Responses are printed to stdout and need to be formatted
    /// using the json format with a key `response`.
    pub async fn handle_cmd(mut self, command: Commands) -> anyhow::Result<()> {
        let form = dispatch_cmd!(
            self,
            command,
            Power -> handle_power_nodes,
            Usb -> handle_usb,
            Flash -> handle_flash,
            Firmware -> handle_firmware,
            Eth -> handle_eth,
            Uart -> handle_uart
        );

        let request = if let Some(form) = form {
            Client::new().post(self.url).multipart(form)
        } else {
            Client::new().request(Method::GET, self.url)
        };

        let response = request.send().await.context("http request error")?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        status
            .is_success()
            .then(|| {
                let txt = body
                    .get("response")
                    .map(ToString::to_string)
                    .unwrap_or("unexpected response body".to_string());
                println!("{}", txt);
            })
            .ok_or(anyhow!(
                "request unsuccessful: {}",
                status.canonical_reason().unwrap_or_default()
            ))
    }

    async fn handle_uart(&mut self, args: UartArgs) -> anyhow::Result<Option<Form>> {
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
                .append_pair("cmd", &args.cmd.unwrap());
        }
        Ok(None)
    }

    async fn handle_eth(&mut self, args: EthArgs) -> anyhow::Result<Option<Form>> {
        if args.reset {
            self.url
                .query_pairs_mut()
                .append_pair("opt", "set")
                .append_pair("type", "network")
                .append_pair("cmd", "reset");
        }
        Ok(None)
    }

    async fn handle_firmware(&mut self, args: FirmwareArgs) -> anyhow::Result<Option<Form>> {
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

    async fn handle_flash(&mut self, args: FlashArgs) -> anyhow::Result<Option<Form>> {
        let mut serializer = self.url.query_pairs_mut();
        serializer
            .append_pair("opt", "set")
            .append_pair("type", "flash")
            .append_pair("node", &(args.node - 1).to_string());

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

    async fn handle_usb(&mut self, args: UsbArgs) -> anyhow::Result<Option<Form>> {
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

    async fn handle_power_nodes(&mut self, args: PowerArgs) -> anyhow::Result<Option<Form>> {
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
