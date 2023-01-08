#[macro_use]
extern crate amplify;

use std::any::Any;
use std::io::Write;
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::process::ExitCode;
use std::{fs, io, net};

use clap::Parser;
use cyphernet::addr::{PeerAddr, ProxyError, SocketAddr};
use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::socks5::Socks5Error;
use nsh::client::Client;
use nsh::server::{NodeKeys, Server};
use nsh::shell::LogLevel;
use nsh::RemoteAddr;
use reactor::poller::popol;
use reactor::Reactor;

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
    /// (see `--socks5` argument) remote address can be a string representing
    /// address supported by the specific proxy, for instance Tor, I2P or
    /// Nym address.
    ///
    /// If SOCKS5 proxy is used, either it has to be provided as `--socks5`
    /// argument, or as a prefix to the remote host address here, in form of
    /// `socks5h://<proxy_address>/<remote_host>`.
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub remote_host: Option<PeerAddr<PublicKey, SocketAddr<DEFAULT_PORT>>>,

    /// Command to execute on the remote host
    #[arg(conflicts_with = "listen", required_unless_present = "listen")]
    pub command: Option<String>,
}

enum Command {
    Listen(net::SocketAddr),
    Connect {
        remote_host: RemoteAddr,
        remote_command: String,
    },
}

struct Config {
    pub node_keys: NodeKeys,
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
    Reactor(reactor::Error<net::SocketAddr, RawFd>),

    #[from]
    Socks5(Socks5Error),

    #[from]
    #[display("error creating thread")]
    Thread(Box<dyn Any + Send>),
}

impl TryFrom<Args> for Config {
    type Error = AppError;

    fn try_from(args: Args) -> Result<Self, Self::Error> {
        let command = if let Some(listen) = args.listen {
            let local_socket = listen.unwrap_or_else(SocketAddr::localhost).into();
            Command::Listen(local_socket)
        } else {
            let remote_host = args.remote_host.expect("clap library broken");
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

        Ok(Config { node_keys, command })
    }
}

fn run() -> Result<(), AppError> {
    let args = Args::parse();

    LogLevel::from_verbosity_flag_count(args.verbose).apply();

    let config = Config::try_from(args)?;

    match config.command {
        Command::Listen(socket_addr) => {
            println!("Listening on {} ...", socket_addr);

            // TODO: Listen on an address
            let service = Server::bind(config.node_keys.ecdh().clone(), socket_addr)?;
            let reactor = Reactor::new(service, popol::Poller::new())?;

            reactor.join()?;
        }
        Command::Connect {
            remote_host,
            remote_command,
        } => {
            eprintln!("Connecting to {} ...", remote_host);

            let mut stdout = io::stdout();
            let mut client = Client::connect(config.node_keys.ecdh(), remote_host)?;
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
