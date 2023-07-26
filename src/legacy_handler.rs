use crate::cli::UsbCmd;
use crate::cli::{Commands, EthArgs, FirwmareArgs, PowerArgs, PowerCmd, UartArgs, UsbArgs};
use anyhow::{anyhow, ensure};
use anyhow::{bail, Context};
use reqwest::{Client, Method, Request};
use url::form_urlencoded::Serializer;
use url::Url;
use url::UrlQuery;

pub struct LegacyHandler {
    base_url: Url,
}

impl LegacyHandler {
    pub fn new(host: String) -> anyhow::Result<Self> {
        let mut base_url = Url::parse(&format!("http://{}", host))?;
        base_url.set_path("api/bmc");
        Ok(Self { base_url })
    }

    /// Simple handler for CLI commands. Responses are printed to stdout and need to be formatted
    /// using the json format with a key `response`.
    pub async fn handle_cmd(mut self, node: Option<u8>, command: Commands) -> anyhow::Result<()> {
        match command {
            Commands::Power(args) => {
                handle_power_nodes(args, node, &mut self.base_url.query_pairs_mut())
            }
            Commands::Usb(args) => handle_usb(args, node, &mut self.base_url.query_pairs_mut())?,
            Commands::Firmware(args) => {
                handle_firmware(args, node, &mut self.base_url.query_pairs_mut())?
            }
            Commands::Eth(args) => handle_eth(args, &mut self.base_url.query_pairs_mut()),
            Commands::Uart(args) => handle_uart(args, &mut self.base_url.query_pairs_mut())?,
        }

        let response = Client::new()
            .execute(Request::new(Method::GET, self.base_url))
            .await
            .context("http request error")?;

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
}

fn handle_uart(
    args: UartArgs,
    serializer: &mut Serializer<'_, UrlQuery<'_>>,
) -> anyhow::Result<()> {
    bail!("not yet implemented")
}

fn handle_eth(args: EthArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    if args.reset {
        serializer
            .append_pair("opt", "set")
            .append_pair("type", "network")
            .append_pair("cmd", "reset");
    }
}

fn handle_firmware(
    args: FirwmareArgs,
    node: Option<u8>,
    serializer: &mut Serializer<'_, UrlQuery<'_>>,
) -> anyhow::Result<()> {
    ensure!(args.bmc.is_none(), "not yet implemented");
    ensure!(args.flash.is_none(), "not yet implemented");
    ensure!(node.is_some(), "`node` argument must be set.");
    serializer
        .append_pair("opt", "set")
        .append_pair("type", "flash")
        .append_pair("file", &args.local.unwrap().to_string_lossy())
        .append_pair("node", &node.unwrap_or_default().to_string());
    Ok(())
}

fn handle_usb(
    args: UsbArgs,
    node: Option<u8>,
    serializer: &mut Serializer<'_, UrlQuery<'_>>,
) -> anyhow::Result<()> {
    if args.mode == UsbCmd::Status {
        serializer
            .append_pair("opt", "get")
            .append_pair("type", "usb");
        return Ok(());
    }

    ensure!(node.is_some(), "`node` argument must be set.");
    serializer
        .append_pair("opt", "set")
        .append_pair("type", "usb")
        .append_pair("node", &node.unwrap_or_default().to_string());
    if args.mode == UsbCmd::Host {
        serializer.append_pair("mode", "0");
    } else {
        serializer.append_pair("mode", "1");
    }

    if args.boot_mode {
        serializer.append_pair("boot_pin", "1");
    }
    Ok(())
}

fn handle_power_nodes(
    args: PowerArgs,
    node: Option<u8>,
    serializer: &mut Serializer<'_, UrlQuery<'_>>,
) {
    if args.cmd == PowerCmd::Status {
        serializer
            .append_pair("opt", "get")
            .append_pair("type", "power");
        return;
    }

    serializer
        .append_pair("opt", "set")
        .append_pair("type", "power");

    let on_bit = if args.cmd == PowerCmd::On { "1" } else { "0" };

    if let Some(node) = node {
        serializer.append_pair(&format!("node{}", node), on_bit);
    } else {
        serializer.append_pair("node1", on_bit);
        serializer.append_pair("node2", on_bit);
        serializer.append_pair("node3", on_bit);
        serializer.append_pair("node4", on_bit);
    }
}
