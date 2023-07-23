use clap::{Args, Parser, Subcommand, ValueEnum};

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
    #[arg(default_value = "turing-pi.local")]
    pub host: Option<String>,
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

#[derive(ValueEnum, Clone)]
pub enum UsbConfig {
    Device,
    Host,
}

#[derive(ValueEnum, Clone, PartialEq)]
pub enum OnOff {
    On,
    Off,
    Reset,
}

#[derive(Args, Clone)]
pub struct EthArgs {
    /// reset ethernet switch
    #[arg(short, long)]
    reset: bool,
}

#[derive(Args)]
pub struct UartArgs {}

#[derive(Args, Clone)]
pub struct UsbArgs {
    #[arg(value_parser = clap::value_parser!(u8).range(1..4))]
    node: u8,
    /// specify which mode to set the given node in.
    mode: UsbConfig,
    #[arg(short, long)]
    /// instead of USB-A, route usb-bus to the BMC chip.
    bmc: bool,
}

#[derive(Args, Clone)]
pub struct FirwmareArgs {}

/// wasdfads
#[derive(Args)]
pub struct PowerArgs {
    // specify command
    pub cmd: OnOff,
    /// turn on/off a specific node. (1-4)
    #[arg(value_parser = clap::value_parser!(u8).range(1..4))]
    pub node: Option<u8>,
}
