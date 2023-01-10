#[macro_use]
extern crate amplify;

use std::any::Any;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use std::{fs, io};

use clap::Parser;
use cyphernet::addr::{HostName, InetHost, Localhost, NetAddr, PartialAddr, PeerAddr};
use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::socks5::{Socks5, Socks5Error};
use netservices::tunnel::Tunnel;
use netservices::NetSession;
use nsh::client::Client;
use nsh::command::Command;
use nsh::processor::Processor;
use nsh::server::{Accept, NodeKeys, Server};
use nsh::shell::LogLevel;
use nsh::{RemoteAddr, Transport};
use reactor::poller::popol;
use reactor::Reactor;

pub const DEFAULT_PORT: u16 = 3232;
pub const DEFAULT_SOCKS5_PORT: u16 = 9050; // We default to Tor proxy

pub const DEFAULT_DIR: &'static str = "~/.nsh";
pub const DEFAULT_ID_FILE: &'static str = "ssi_ed25519";

type AddrArg = PartialAddr<HostName, DEFAULT_PORT>;

#[derive(Clone, Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Verbosity level
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Start as a daemon listening on a specific socket.
    ///
    /// If the socket address is not given, defaults to 127.0.0.1:3232
    #[arg(short, long)]
    pub listen: Option<Option<PartialAddr<InetHost, DEFAULT_PORT>>>,

    /// Path to an identity (key) file.
    #[arg(short, long)]
    pub id: Option<PathBuf>,

    /// SOCKS5 proxy, as IPv4 or IPv6 socket. If port is not given, it defaults
    /// to 9050.
    #[arg(short = 'p', long, conflicts_with = "listen")]
    pub proxy: Option<PartialAddr<InetHost, DEFAULT_SOCKS5_PORT>>,

    /// Tunneling mode: listens on a provided address and tunnels all incoming
    /// connections to the `REMOTE_HOST`.
    ///
    /// If the socket address is not given, defaults to 127.0.0.1:3232
    #[arg(short, long, conflicts_with = "listen")]
    pub tunnel: Option<Option<PartialAddr<InetHost, DEFAULT_SOCKS5_PORT>>>,

    /// Address of the remote host to connect.
    ///
    /// Remote address, if no proxy is used, should be either IPv4 or IPv6
    /// socket address with optional port information. If SOCKS5 proxy is used,
    /// (see `--socks5` argument) remote address can be a string representing
    /// address supported by the specific proxy, for instance Tor, I2P or
    /// Nym address.
    ///
    /// If the address is provided without a port, a default port 3232 is used.
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub remote_host: Option<PeerAddr<PublicKey, AddrArg>>,

    /// Command to execute on the remote host
    #[arg(conflicts_with_all = ["listen", "tunnel"], required_unless_present_any = ["listen", "tunnel"])]
    pub command: Option<Command>,
}

enum Mode {
    Listen(NetAddr<InetHost>),
    Tunnel {
        local: NetAddr<InetHost>,
        remote: RemoteAddr,
    },
    Connect {
        remote_host: RemoteAddr,
        remote_command: Command,
    },
}

struct Config {
    pub node_keys: NodeKeys,
    pub mode: Mode,
    pub proxy_addr: NetAddr<InetHost>,
}

#[derive(Debug, Display, Error, From)]
#[display(inner)]
pub enum AppError {
    #[from]
    Io(io::Error),

    #[from]
    Curve25519(ed25519_compact::Error),

    #[from]
    Reactor(reactor::Error<Accept, Transport>),

    #[from]
    Socks5(Socks5Error),

    #[from]
    #[display("error creating thread")]
    Thread(Box<dyn Any + Send>),

    #[display("unable to construct tunnel with {0}: {1}")]
    Tunnel(RemoteAddr, io::Error),
}

impl TryFrom<Args> for Config {
    type Error = AppError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let command = if let Some(listen) = args.listen {
            let local_socket = listen.unwrap_or_else(Localhost::localhost).into();
            Mode::Listen(local_socket)
        } else if let Some(tunnel) = args.tunnel {
            let local = tunnel
                .unwrap_or_else(|| PartialAddr::localhost(None))
                .into();
            let remote = args.remote_host.expect("clap library broken");
            Mode::Tunnel {
                local,
                remote: remote.into(),
            }
        } else {
            let remote_host = args.remote_host.expect("clap library broken");
            Mode::Connect {
                remote_host: remote_host.into(),
                remote_command: args.command.unwrap_or(Command::ECHO),
            }
        };

        let id_path = args.id.unwrap_or_else(|| {
            let mut dir = PathBuf::from(DEFAULT_DIR);
            dir.push(DEFAULT_ID_FILE);
            dir
        });
        let id_path = shellexpand::tilde(&id_path.to_string_lossy()).to_string();
        let id_pem = fs::read_to_string(&id_path).or_else(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                println!(
                    "Identity file not found; creating new identity in '{}'",
                    id_path
                );
                let pair = ::ed25519_compact::KeyPair::generate();
                let pem = pair.sk.to_pem();
                let mut dir = PathBuf::from(&id_path);
                dir.pop();
                fs::create_dir_all(dir)?;
                fs::write(id_path, &pem)?;
                Ok(pem)
            } else {
                Err(err)
            }
        })?;
        let id = PrivateKey::from_pem(&id_pem)?;
        let node_keys = NodeKeys::from(id);
        println!("Using identity {}", node_keys.pk());

        let proxy_addr = args.proxy.unwrap_or(Localhost::localhost()).into();

        Ok(Config {
            node_keys,
            mode: command,
            proxy_addr,
        })
    }
}

fn run() -> Result<(), AppError> {
    let args = Args::parse();

    LogLevel::from_verbosity_flag_count(args.verbose).apply();

    let config = Config::try_from(args)?;
    let proxy = Socks5::new(config.proxy_addr)?;

    match config.mode {
        Mode::Listen(socket_addr) => {
            println!("Listening on {socket_addr} ...");

            let processor = Processor::new(proxy);
            let service = Server::with(config.node_keys.ecdh().clone(), &socket_addr, processor)?;
            let reactor = Reactor::new(service, popol::Poller::new())?;

            reactor.join()?;
        }
        Mode::Tunnel { remote, local } => {
            eprintln!("Tunneling to {remote} from {local}...");

            let session =
                Transport::connect_blocking(remote.clone(), config.node_keys.ecdh(), &proxy)?;
            let mut tunnel = match Tunnel::with(session, local) {
                Ok(tunnel) => tunnel,
                Err((session, err)) => {
                    session.disconnect()?;
                    return Err(AppError::Tunnel(remote, err));
                }
            };
            let _ = tunnel.tunnel_once(popol::Poller::new(), Duration::from_secs(10))?;
            tunnel.into_session().disconnect()?;
        }
        Mode::Connect {
            remote_host,
            remote_command,
        } => {
            eprintln!("Connecting to {remote_host} ...");

            let mut stdout = io::stdout();
            let mut client = Client::connect(config.node_keys.ecdh(), remote_host, &proxy)?;
            let mut printout = client.exec(remote_command)?;
            eprintln!("Remote output >>>");
            for batch in &mut printout {
                stdout.write_all(&batch)?;
            }
            stdout.flush()?;
            client = printout.complete();
            client.disconnect()?;

            eprintln!("<<< done");
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {}", err);
            ExitCode::FAILURE
        }
    }
}
