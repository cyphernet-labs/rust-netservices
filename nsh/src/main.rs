#[macro_use]
extern crate amplify;
#[macro_use]
extern crate clap;

use std::net;
use std::path::{PathBuf};
use cyphernet::addr::{UniversalAddr, SocketAddr, ProxyError, PeerAddr};
use clap::{Parser};
use ed25519_compact::{KeyPair, SecretKey};

pub const DEFAULT_PORT: u16 = 3232;
pub const DEFAULT_SOCKS5_PORT: u16 = 9050; // We default to Tor proxy

pub const DEFAULT_DIR: &'static str = "~/.nsh";
pub const DEFAULT_ID_FILE: &'static str = "id_ed25519";

#[derive(Clone, Debug)]
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Verbosity level
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Start as a daemon listening on a specific socket.
    ///
    /// If the socket address is not given, defaults to 127.0.0.1.
    #[arg(short, long)]
    pub listen: Option<Option<SocketAddr<DEFAULT_PORT>>>,

    /// Path to an identity (key) file.
    #[arg(short, long)]
    pub id: Option<PathBuf>,

    /// SOCKS5 proxy, as IPv4 or IPv6 socket. If port is not given, it defaults
    /// to 9050.
    #[arg(short = '5', long, conflicts_with = "listen")]
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
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub remote_host: Option<UniversalAddr<PeerAddr<SecretKey, SocketAddr<DEFAULT_PORT>>>>,

    /// Command to execute on the remote host
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub command: Option<String>
}

enum Command {
    Listen(net::SocketAddr),
    Connect {
        remote_host: UniversalAddr<PeerAddr<SecretKey, net::SocketAddr>>,
        remote_command: String
    },
}

struct Config {
    pub id: KeyPair,
    pub command: Command
}

#[derive(Debug, Display, Error, From)]
#[display(inner)]
pub enum AppError {
    #[from]
    Proxy(ProxyError)
}

impl TryFrom<Args> for Config {
    type Error = AppError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let mut remote_host = args.remote_host;
        if let Some(proxy) = args.socks5 {
            remote_host = remote_host.try_proxy(proxy.into())?;
        }

        let command = if let Some(listen) = args.listen {
            let local_socket = listen.unwrap_or_else(SocketAddr::localhost).into();
            Command::Listen(local_socket)
        } else {
            let remote_host = args.remote_host.expect("clap library broken");
            // Merge SOCKS5 information
            // Resolve to default port
        };

        Ok(Config {
            command: args.command.unwrap_or_else(|| s!("nsh"))
        })
    }
}

type Reactor = nakamoto_net_poll::Reactor<net::TcpStream>;

fn main() -> Result<(), AppError> {
    let args = Args::parse();

    let config = Config::try_from(args)?;

    match config.command {
        Command::Listen() => {

        }
        Command::Connect { .. } => {}
    }

    println!("Connecting to {} ...", config.remote_host);

    Ok(())
}
