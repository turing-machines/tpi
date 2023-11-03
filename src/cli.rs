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

use clap::{builder::NonEmptyStringValueParser, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[cfg(not(feature = "localhost"))]
const DEFAULT_HOST_NAME: &str = "turingpi.local";
#[cfg(feature = "localhost")]
const DEFAULT_HOST_NAME: &str = "127.0.0.1";

/// Commandline interface that controls turing-pi's BMC. The BMC must be connected to a network
/// that is reachable over TCP/IP in order for this tool to function. All commands are persisted by
/// the BMC. Please be aware that if no hostname is specified, it will try to resolve the hostname
/// by testing a predefined sequence of options.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true, arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Specify the Turing-pi host to connect to. Note: IPv6 addresses must be wrapped in square
    /// brackets e.g. `[::1]`
    #[arg(default_value = DEFAULT_HOST_NAME, value_parser = NonEmptyStringValueParser::new(), long, global = true)]
    pub host: Option<String>,

    /// Specify a user name to log in as. If unused, an interactive prompt will ask for credentials
    /// unless a cached token file is present.
    #[arg(long, global = true)]
    pub user: Option<String>,

    /// Same as `--username`
    #[arg(long, name = "PASS", global = true)]
    pub password: Option<String>,

    /// Print results formatted as JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Force which version of the BMC API to use. Try lower the version if you are running
    /// older BMC firmware.
    #[arg(default_value = "v1-1", short, global = true)]
    pub api_version: Option<ApiVersion>,

    #[arg(short, name = "gen completion", exclusive = true)]
    pub gencompletion: Option<clap_complete::shells::Shell>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Power on/off or reset specific nodes.
    #[command(arg_required_else_help = true)]
    Power(PowerArgs),

    /// Change the USB device/host configuration. The USB-bus can only be routed to one
    /// node simultaneously.
    #[command(arg_required_else_help = true)]
    Usb(UsbArgs),

    /// Upgrade the firmware of the BMC
    #[command(arg_required_else_help = true)]
    Firmware(FirmwareArgs),

    /// Flash a given node
    #[command(arg_required_else_help = true)]
    Flash(FlashArgs),

    /// Configure the on-board Ethernet switch.
    #[command(arg_required_else_help = true)]
    Eth(EthArgs),

    /// Read or write over UART
    #[command(arg_required_else_help = true)]
    Uart(UartArgs),

    /// Advanced node modes
    #[command(arg_required_else_help = true)]
    Advanced(AdvancedArgs),

    /// Print turing-pi info
    Info,
}

#[derive(ValueEnum, Clone, PartialEq, Eq)]
pub enum GetSet {
    Get,
    Set,
}

#[derive(ValueEnum, Clone, PartialEq, Eq)]
pub enum ModeCmd {
    /// Clear any advanced mode
    Normal,
    /// reboots supported compute modules and expose its eMMC storage as a mass
    /// storage device
    Msd,
    /// Setting the recovery pin high will cause the module to halt after its
    /// respective bootROM code. A manual restart is required.
    Recovery,
}

#[derive(ValueEnum, Clone, PartialEq, Eq)]
pub enum UsbCmd {
    /// Configure the specified node as USB device. The `BMC` itself or USB-A port is USB host
    Device,
    /// Configure the specified node as USB Host. USB devices can be attached to
    /// the USB-A port on the board.
    Host,
    Status,
}

#[derive(ValueEnum, Clone, PartialEq, Eq)]
pub enum PowerCmd {
    On,
    Off,
    Reset,
    Get,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum ApiVersion {
    V1,
    V1_1,
}

impl ApiVersion {
    pub fn scheme(&self) -> &str {
        match self {
            ApiVersion::V1 => "http",
            ApiVersion::V1_1 => "https",
        }
    }
}

#[derive(Args, Clone)]
pub struct EthArgs {
    /// Reset Ethernet switch
    #[arg(short, long)]
    pub reset: bool,
}

#[derive(Args)]
pub struct AdvancedArgs {
    pub mode: ModeCmd,
    /// [possible values: 1-4]
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: u8,
}

#[derive(Args)]
pub struct UartArgs {
    pub action: GetSet,
    /// [possible values: 1-4], Not specifying a node selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: u8,
    #[arg(short, long)]
    pub cmd: Option<String>,
}

#[derive(Args, Clone)]
pub struct UsbArgs {
    /// specify which mode to set the given node in.
    #[arg(short, long)]
    pub mode: UsbCmd,
    /// instead of USB-A, route the USB-bus to the BMC chip.
    #[arg(short, long)]
    pub bmc: bool,
    /// [possible values: 1-4]
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: Option<u8>,
}

#[derive(Args, Clone)]
pub struct FirmwareArgs {
    #[arg(short, long)]
    pub file: PathBuf,
}

#[derive(Args, Clone)]
#[group(required = true)]
pub struct FlashArgs {
    /// Update a node with an image accessible from the local filesystem, typically a BMC-visible
    /// microSD card.
    #[arg(short, long)]
    pub local: bool,
    /// Update a node with the given image.
    #[arg(short, long)]
    pub image_path: PathBuf,
    /// [possible values: 1-4]
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: u8,
}

#[derive(Args)]
pub struct PowerArgs {
    /// Specify command
    pub cmd: PowerCmd,
    /// [possible values: 1-4], Not specifying a node selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: Option<u8>,
}
