use crate::cli::{Commands, EthArgs, FirwmareArgs, OnOff, PowerArgs, UartArgs, UsbArgs};
use anyhow::anyhow;
use anyhow::Context;
use reqwest::{Client, Method, Request};
use url::form_urlencoded::Serializer;
use url::Url;
use url::UrlQuery;

pub struct LegacyHandler {
    base_url: Url,
}

impl LegacyHandler {
    pub fn new(host: String) -> anyhow::Result<Self> {
        let mut base_url = Url::parse(&host)?;
        base_url.set_path("api/bmc");
        Ok(Self { base_url })
    }

    pub async fn handle_cmd(mut self, command: Commands) -> anyhow::Result<()> {
        match command {
            Commands::Power(args) => handle_power_nodes(args, &mut self.base_url.query_pairs_mut()),
            Commands::Usb(args) => handle_usb(args, &mut self.base_url.query_pairs_mut()),
            Commands::Firmware(args) => handle_firmware(args, &mut self.base_url.query_pairs_mut()),
            Commands::Eth(args) => handle_eth(args, &mut self.base_url.query_pairs_mut()),
            Commands::Uart(args) => handle_uart(args, &mut self.base_url.query_pairs_mut()),
        }

        Client::new()
            .execute(Request::new(Method::GET, self.base_url))
            .await
            .context("http request error")
            .and_then(|rsp| {
                let status = rsp.status();
                status.is_success().then_some(()).ok_or_else(|| {
                    anyhow!(
                        "request unsuccessful: {}",
                        status.canonical_reason().unwrap_or_default()
                    )
                })
            })
    }
}

fn handle_uart(args: UartArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    todo!()
}

fn handle_eth(args: EthArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    todo!()
}

fn handle_firmware(args: FirwmareArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    todo!()
}

fn handle_usb(args: UsbArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    serializer
        .append_pair("opt", "set")
        .append_pair("type", "usb");
    todo!()
}

fn handle_power_nodes(args: PowerArgs, serializer: &mut Serializer<'_, UrlQuery<'_>>) {
    serializer
        .append_pair("opt", "set")
        .append_pair("type", "power");
    let on_bit = if args.cmd == OnOff::On { "1" } else { "0" };

    if let Some(node) = args.node {
        serializer.append_pair(&format!("node{}", node), on_bit);
    } else {
        serializer.append_pair("node1", on_bit);
        serializer.append_pair("node2", on_bit);
        serializer.append_pair("node3", on_bit);
        serializer.append_pair("node4", on_bit);
    }
}
