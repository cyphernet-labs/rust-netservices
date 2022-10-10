#[macro_use]
extern crate amplify;
#[macro_use]
extern crate clap;

use cyphernet::addr::{UniversalAddr, SocketAddr, ProxyError};
use clap::{Parser};

pub const DEFAULT_PORT: u16 = 3232;
pub const DEFAULT_SOCKS5_PORT: u16 = 9050; // We default to Tor proxy

#[derive(Clone, Debug)]
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Verbosity level
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// SOCKS5 proxy, as IPv4 or IPv6 socket. If port is not given, it defaults
    /// to 9050.
    #[arg(short = '5', long)]
    pub socks5: Option<SocketAddr<DEFAULT_SOCKS5_PORT>>,

    /// Address of the remote host to connect.
    ///
    /// Remote address, if no proxy is used, should be either IPv4 or IPv6
    /// socket address with optional port information. If SOCKS5 proxy is used,
    /// (see `--socks5` argument) remote address can be a string representating
    /// address supported by the specific proxy, for instance Tor, I2P or
    /// Nym address.
    ///
    /// If SOCKS5 proxy is used, either it has to be provided as `--socks5`
    /// argument, or as a prefix to the remote host address here, in form of
    /// `socks5h://<proxy_address>/<remote_host>`.
    #[arg()]
    pub remote_host: UniversalAddr<SocketAddr<DEFAULT_PORT>>,

    /// Command to execute on the remote host
    #[arg()]
    pub command: Option<String>
}

struct Config {
    pub remote_host: UniversalAddr<SocketAddr<DEFAULT_PORT>>,
    pub command: String
}

impl TryFrom<Args> for Config {
    type Error = ProxyError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let mut remote_host = args.remote_host;
        if let Some(proxy) = args.socks5 {
            remote_host = remote_host.try_proxy(proxy.into())?;
        }
        Ok(Config {
            remote_host,
            command: args.command.unwrap_or_else(|| s!("nsh"))
        })
    }
}

fn main() -> Result<(), ProxyError> {
    let args = Args::parse();

    let config = Config::try_from(args)?;

    println!("Connecting to {} ...", config.remote_host);

    Ok(())
}
