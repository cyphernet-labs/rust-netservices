#[macro_use]
extern crate amplify;
#[macro_use]
extern crate log;

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
use std::{fs, io, net};

use clap::Parser;
use crossbeam_channel as chan;
use cyphernet::addr::{LocalNode, PeerAddr, ProxyError, SocketAddr, UniversalAddr};
use cyphernet::crypto::ed25519::Curve25519;
use nakamoto_net::{Io, Link, LocalTime, Reactor as _};
use streampipes::frame::dumb::DumbFramed64KbStream;
use streampipes::transcode::dumb::{DumbDecoder, DumbEncoder, DumbTranscodedStream};

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
    Nakamoto(nakamoto_net::error::Error),
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

type Reactor = nakamoto_net_poll::Reactor<TcpStream>;
type Stream = DumbTranscodedStream<io::Cursor<Vec<u8>>, Vec<u8>>;

fn main() -> Result<(), AppError> {
    let args = Args::parse();

    let config = Config::try_from(args)?;

    let (handle, commands) = chan::unbounded::<ProtocolCmd>();
    let (shutdown, shutdown_recv) = chan::bounded(1);
    let (listening_send, listening) = chan::bounded(1);
    let mut reactor = Reactor::new(shutdown_recv, listening_send)?;
    let mut protocol = Protocol::default();
    let events = Events {};

    let listen_sockets = match config.command {
        Command::Listen(socket_addr) => {
            println!("Listening on {} ...", socket_addr);
            vec![socket_addr]
        }
        Command::Connect {
            remote_host,
            remote_command: _,
        } => {
            println!("Connecting to {} ...", remote_host);
            protocol.connect(remote_host.into());
            vec![]
        }
    };
    reactor.run(&listen_sockets, protocol, events, commands)?;

    Ok(())
}

#[derive(Debug, Clone)]
pub enum ProtocolEvent {}
#[derive(Debug, Clone)]
pub enum ProtocolCmd {}
#[derive(Debug, Display)]
#[display(doc_comments)]
pub enum DisconnectReason {}
impl From<DisconnectReason> for nakamoto_net::DisconnectReason<DisconnectReason> {
    fn from(reason: DisconnectReason) -> Self {
        nakamoto_net::DisconnectReason::Protocol(reason)
    }
}

#[derive(Default)]
pub struct Protocol {
    connect_queue: VecDeque<net::SocketAddr>,
    streams: HashMap<net::SocketAddr, Stream>,
}

impl Protocol {
    pub fn connect(&mut self, socket_addr: net::SocketAddr) {
        self.connect_queue.push_back(socket_addr);
    }
}

impl nakamoto_net::Protocol for Protocol {
    type Event = ProtocolEvent;
    type Command = ProtocolCmd;
    type DisconnectReason = DisconnectReason;

    fn received_bytes(&mut self, addr: &net::SocketAddr, bytes: &[u8]) {
        let stream = self.streams.get_mut(addr).expect("unknown remote peer");
        stream.push_bytes(bytes).expect("buffer overflow");

        // Printout the received data
        let mut buf = vec![0u8; bytes.len()];
        loop {
            let len = stream.read(&mut buf).expect("in-memory reading");
            if len == 0 {
                break;
            }
            let data = String::from_utf8_lossy(&buf[..len]);
            println!("{}", data);
        }
    }

    fn attempted(&mut self, addr: &net::SocketAddr) {}

    fn connected(&mut self, addr: net::SocketAddr, _local_addr: &net::SocketAddr, link: Link) {
        debug_assert_eq!(
            link,
            Link::Inbound,
            "Reactor implementation can only accept incoming connections"
        );

        let reader = io::Cursor::new(Vec::<u8>::new());
        let writer = Vec::<u8>::new();
        let stream = Stream::with(reader, writer, DumbDecoder, DumbEncoder);
        if self.streams.insert(addr, stream).is_some() {
            warn!(
                "Repeated incoming connection from peer {}, resetting tunnel",
                addr
            );
        }

        // TODO: Perform handshake
    }

    fn disconnected(
        &mut self,
        addr: &net::SocketAddr,
        _reason: nakamoto_net::DisconnectReason<Self::DisconnectReason>,
    ) {
        self.streams.remove(addr).expect("unknown remote peer");
    }

    fn command(&mut self, cmd: Self::Command) {
        panic!("Commands are not supported")
    }

    fn tick(&mut self, local_time: LocalTime) {}

    fn wake(&mut self) {}
}

impl Iterator for Protocol {
    type Item = Io<ProtocolEvent, DisconnectReason>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(socket_addr) = self.connect_queue.pop_front() {
            return Some(Io::Connect(socket_addr));
        }
        None
    }
}

pub struct Events {}

impl nakamoto_net::Publisher<ProtocolEvent> for Events {
    fn publish(&mut self, e: ProtocolEvent) {
        info!("Received event {:?}", e);
    }
}
