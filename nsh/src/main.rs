#[macro_use]
extern crate amplify;

use std::any::Any;
use std::io::Write;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::{fs, io, net};

use clap::Parser;
use cyphernet::addr::{LocalNode, PeerAddr, ProxyError, SocketAddr, UniversalAddr};
use cyphernet::crypto::ed25519::Curve25519;
use ioreactor::popol::PollManager;
use ioreactor::{Controller, InternalError, Reactor, ReactorApi, Resource};
use streampipes::transcode::dumb::DumbTranscodedStream;

pub const DEFAULT_PORT: u16 = 3232;
pub const DEFAULT_SOCKS5_PORT: u16 = 9050; // We default to Tor proxy

pub const DEFAULT_DIR: &'static str = "~/.nsh";
pub const DEFAULT_ID_FILE: &'static str = "id_ed25519";

#[derive(Clone, Debug, Parser)]
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
    pub remote_host: Option<UniversalAddr<PeerAddr<Curve25519, SocketAddr<DEFAULT_PORT>>>>,

    /// Command to execute on the remote host
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub command: Option<String>,
}

enum Command {
    Listen(net::SocketAddr),
    Connect {
        remote_host: UniversalAddr<PeerAddr<Curve25519, net::SocketAddr>>,
        remote_command: String,
    },
}

struct Config {
    pub node_keys: LocalNode<Curve25519>,
    pub command: Command,
}

#[derive(Debug, Display, Error, From)]
#[display(inner)]
pub enum AppError {
    #[from]
    Proxy(ProxyError),

    #[from]
    Io(io::Error),

    #[from]
    Curve25519(ed25519_compact::Error),

    #[from]
    Reactor(InternalError),

    #[from]
    #[display("other error")]
    Other(Box<dyn Any + Send>),
}

impl TryFrom<Args> for Config {
    type Error = AppError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let command = if let Some(listen) = args.listen {
            let local_socket = listen.unwrap_or_else(SocketAddr::localhost).into();
            Command::Listen(local_socket)
        } else {
            let mut remote_host = args.remote_host.expect("clap library broken");
            if let Some(proxy) = args.socks5 {
                remote_host = remote_host.try_proxy(proxy.into())?;
            }
            Command::Connect {
                remote_host: remote_host.into(),
                remote_command: args.command.unwrap_or_else(|| s!("bash")),
            }
        };

        let id_path = args.id.unwrap_or_else(|| {
            let mut dir = PathBuf::from(DEFAULT_DIR);
            dir.push(DEFAULT_ID_FILE);
            dir
        });
        let id_path = shellexpand::tilde(&id_path.to_string_lossy()).to_string();
        let id_pem = fs::read_to_string(id_path)?;
        let id = ed25519_compact::SecretKey::from_pem(&id_pem)?;

        Ok(Config {
            node_keys: LocalNode::from(id.into()),
            command,
        })
    }
}

type Stream = DumbTranscodedStream<io::Cursor<Vec<u8>>, Vec<u8>>;

fn main() -> Result<(), AppError> {
    let args = Args::parse();

    let config = Config::try_from(args)?;

    let manager = PollManager::<NshSession>::new();
    let mut reactor = Reactor::with(manager, (), |err| {
        panic!("{}", err);
    })?;

    match config.command {
        Command::Listen(socket_addr) => {
            println!("Listening on {} ...", socket_addr);
            // TODO: Listen on an address
        }
        Command::Connect {
            remote_host,
            remote_command: _,
        } => {
            println!("Connecting to {} ...", remote_host);
            reactor.connect(remote_host)?;
        }
    }
    reactor.join()?;

    Ok(())
}

pub type NshAddr = UniversalAddr<PeerAddr<Curve25519, net::SocketAddr>>;

pub struct NshSession {
    connection: net::TcpStream,
    controller: Controller<Self>,
}

impl Resource for NshSession {
    type Id = RawFd;
    type Addr = NshAddr;
    type Error = io::Error;
    type Data = Vec<u8>;
    type OutputChannels = ();

    fn with(
        addr: Self::Addr,
        controller: Controller<Self>,
        output_channels: Self::OutputChannels,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let connection = net::TcpStream::connect(addr)?;
        Ok(Self {
            connection,
            controller,
        })
    }

    fn id(&self) -> Self::Id {
        self.connection.as_raw_fd()
    }

    fn update_from_io(&mut self, input: bool, output: bool) -> Result<(), Self::Error> {
        todo!()
    }

    fn send(&mut self, data: Self::Data) -> Result<(), Self::Error> {
        self.connection.write_all(&data)
    }
}
