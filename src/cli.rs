use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[cfg(not(feature = "local-only"))]
const DEFAULT_HOST_NAME: &str = "turingpi.local";
#[cfg(feature = "local-only")]
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
    /// Specify the Turing-pi host to connect to. Note: ipv6 addresses must be wrapped in square
    /// brackets e.g. `[::1]`
    #[arg(default_value = DEFAULT_HOST_NAME, long, global = true)]
    pub host: Option<String>,
    #[arg(long, global = true, help = "print results formatted as json")]
    pub json: bool,
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
}

#[derive(ValueEnum, Clone, PartialEq)]
pub enum GetSet {
    Get,
    Set,
}

#[derive(ValueEnum, Clone, PartialEq)]
pub enum UsbCmd {
    Device,
    Host,
    Status,
}

#[derive(ValueEnum, Clone, PartialEq)]
pub enum PowerCmd {
    On,
    Off,
    Reset,
    Get,
}

#[derive(Args, Clone)]
pub struct EthArgs {
    /// Reset ethernet switch
    #[arg(short, long)]
    pub reset: bool,
}

#[derive(Args)]
pub struct UartArgs {
    pub action: GetSet,
    /// [possible values: 1-4], Not specifying a node
    /// selects all nodes.
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
    // /// instead of USB-A, route usb-bus to the BMC chip.
    #[arg(short, long)]
    pub bmc: bool,
    /// Set the boot pin, referred to as 'rpiboot pin' high
    #[arg(short, long)]
    pub usb_boot: bool,
    /// [possible values: 1-4], Not specifying a node
    /// selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: u8,
}

#[derive(Args, Clone)]
pub struct FirmwareArgs {
    #[arg(short, long)]
    pub file: PathBuf,
}

#[derive(Args, Clone)]
#[group(required = true)]
pub struct FlashArgs {
    /// Update a node with an image local on the disk.
    #[cfg(not(feature = "local-only"))]
    #[arg(short, long)]
    pub local: bool,
    /// Update a node with the given image.
    #[arg(short, long)]
    pub image_path: PathBuf,
    /// [possible values: 1-4], Not specifying a node
    /// selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: u8,
}

#[derive(Args)]
pub struct PowerArgs {
    // specify command
    pub cmd: PowerCmd,
    /// [possible values: 1-4], Not specifying a node
    /// selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..5))]
    pub node: Option<u8>,
}
