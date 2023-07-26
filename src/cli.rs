use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Commandline interface that controls turing-pi's BMC. An ethernet
/// connection to the board is required in order for this tool to function. All
/// commands are persisted by the BMC. Please be aware that if no hostname is
/// specified, it will try to resolve the hostname by testing a predefined sequence
/// of options.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    #[arg(
        help = "Optional turing-pi host to connect to. Host will be determind given the following order:
1. Explicitly passed via the Cli
2. Using hostname 'turing-pi.local'
3. First host to respond to redfish service discovery
"
    )]
    #[arg(default_value = "turingpi.local")]
    pub host: Option<String>,
    /// [possible values: 1-4], Not specifying a node
    /// selects all nodes.
    #[arg(short, long)]
    #[arg(value_parser = clap::value_parser!(u8).range(1..4))]
    pub node: Option<u8>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Power or reset specific nodes.
    Power(PowerArgs),
    /// Change the USB device/host configuration. The USB-bus can only be routed to one
    /// node simultaniously.
    Usb(UsbArgs),
    /// Upgrade the firmware of the BMC or an specified node.
    Firmware(FirwmareArgs),
    /// configure the on-board ethernet switch.
    Eth(EthArgs),
    /// [depricated] forward a uart command or get a line from the serial.
    Uart(UartArgs),
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
    Status,
}

#[derive(Args, Clone)]
pub struct EthArgs {
    /// reset ethernet switch
    #[arg(short, long)]
    pub reset: bool,
}

#[derive(Args)]
pub struct UartArgs {}

#[derive(Args, Clone)]
pub struct UsbArgs {
    /// specify which mode to set the given node in.
    pub mode: UsbCmd,
    // #[arg(short, long)]
    // /// instead of USB-A, route usb-bus to the BMC chip.
    // pub bmc: bool,
    /// Set the boot pin, referred to as 'rpiboot pin' high
    #[arg(short, long)]
    pub boot_mode: bool,
}

#[derive(Args, Clone)]
#[group(required = true)]
pub struct FirwmareArgs {
    /// Update the firmware of the BMC itself with the given path.
    #[arg(short, long)]
    pub bmc: Option<PathBuf>,
    /// Update a node with an image local on the disk.
    #[arg(short, long)]
    pub local: Option<PathBuf>,
    /// Update a node with the given image.
    #[arg(short, long)]
    pub flash: Option<PathBuf>,
}

/// wasdfads
#[derive(Args)]
pub struct PowerArgs {
    // specify command
    pub cmd: PowerCmd,
}
