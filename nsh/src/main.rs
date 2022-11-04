#[macro_use]
extern crate amplify;

use std::any::Any;
use std::path::PathBuf;
use std::{fs, io, net};

use clap::Parser;
use cyphernet::addr::{LocalNode, PeerAddr, ProxyError, SocketAddr, UniversalAddr};
use cyphernet::crypto::ed25519::Curve25519;
use ioreactor::popol::PollManager;
use ioreactor::{InternalError, Reactor, ReactorApi};

use crate::nxk_tcp::{NxkAddr, NxkContext, NxkMethod, NxkResource};

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
        remote_host: NxkAddr,
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

fn main() -> Result<(), AppError> {
    let args = Args::parse();

    let config = Config::try_from(args)?;

    let manager = PollManager::<NxkResource<()>>::new();
    let mut reactor = Reactor::with(manager, |err| {
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
            let nsh_socket = NxkContext {
                method: NxkMethod::Connect(remote_host),
                local_node: config.node_keys,
            };
            reactor.connect(nsh_socket)?;
        }
    }
    reactor.join()?;

    Ok(())
}

/// Noise_XK streams (can be any)
// TODO: Wrap actual noise_xk functionality
mod noise_xk {
    use std::io::{self, Read, Write};
    use std::os::unix::io::{AsRawFd, RawFd};

    use cyphernet::addr::LocalNode;
    use cyphernet::crypto::Ec;
    use streampipes::NetStream;

    pub struct Stream<EC: Ec, S: streampipes::Stream> {
        stream: S,
        local_node: LocalNode<EC>,
        remote_key: Option<EC::PubKey>,
    }

    impl<EC: Ec, S: streampipes::Stream> Stream<EC, S> {
        pub fn connect(
            stream: S,
            remote_key: EC::PubKey,
            local_node: LocalNode<EC>,
        ) -> io::Result<Self> {
            Ok(Self {
                stream,
                local_node,
                remote_key: Some(remote_key),
            })
        }

        pub fn upgrade(stream: S, local_node: LocalNode<EC>) -> Self {
            Self {
                stream,
                local_node,
                remote_key: None,
            }
        }
    }

    impl<EC: Ec, S: streampipes::Stream> Read for Stream<EC, S> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.stream.read(buf)
        }
    }

    impl<EC: Ec, S: streampipes::Stream> Write for Stream<EC, S> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.stream.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream.flush()
        }
    }

    impl<EC, S> AsRawFd for Stream<EC, S>
    where
        EC: Ec,
        S: NetStream,
    {
        fn as_raw_fd(&self) -> RawFd {
            self.stream.as_raw_fd()
        }
    }
}

/// Noise_XK streams, connections and sessions based on TCP stream
// TODO: Move to a separate crate
pub mod nxk_tcp {
    use std::io::{self, Read, Write};
    use std::marker::PhantomData;
    use std::net;
    use std::os::fd::{AsRawFd, RawFd};

    use cyphernet::addr::{LocalNode, PeerAddr, UniversalAddr};
    use cyphernet::crypto::ed25519::Curve25519;
    use ioreactor::{Controller, IoEv, ReactorApi, Resource};

    use crate::noise_xk;

    pub type NxkAddr = UniversalAddr<PeerAddr<Curve25519, net::SocketAddr>>;

    pub enum TcpSocket {
        Listen(net::SocketAddr),
        Stream(net::TcpStream),
    }

    pub enum NxkMethod {
        Listen(net::SocketAddr),
        Accept(net::TcpStream, net::SocketAddr),
        Connect(NxkAddr),
    }

    pub struct NxkContext {
        pub method: NxkMethod,
        pub local_node: LocalNode<Curve25519>,
    }

    pub type NxkStream = noise_xk::Stream<Curve25519, net::TcpStream>;

    pub struct NxkSession {
        stream: NxkStream,
        socket_addr: net::SocketAddr,
        peer_addr: Option<NxkAddr>,
        inbound: bool,
    }

    impl NxkSession {
        pub fn accept(
            tcp_stream: net::TcpStream,
            remote_socket_addr: net::SocketAddr,
            local_node: LocalNode<Curve25519>,
        ) -> Self {
            Self {
                stream: NxkStream::upgrade(tcp_stream, local_node),
                socket_addr: remote_socket_addr,
                peer_addr: None,
                inbound: true,
            }
        }

        pub fn connect(nsh_addr: NxkAddr, local_node: LocalNode<Curve25519>) -> io::Result<Self> {
            // TODO: Use socks5
            let tcp_stream = net::TcpStream::connect(&nsh_addr)?;
            let remote_key = nsh_addr.as_remote_addr().to_pubkey();
            Ok(Self {
                stream: NxkStream::connect(tcp_stream, remote_key, local_node)?,
                socket_addr: nsh_addr.to_socket_addr(),
                peer_addr: Some(nsh_addr),
                inbound: false,
            })
        }
    }

    impl AsRawFd for NxkSession {
        fn as_raw_fd(&self) -> RawFd {
            self.stream.as_raw_fd()
        }
    }

    impl Read for NxkSession {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.stream.read(buf)
        }
    }

    impl Write for NxkSession {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.stream.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream.flush()
        }
    }

    pub enum NxkInner {
        Listener(net::TcpListener),
        Session(NxkSession),
    }

    pub struct NxkResource<C: Send> {
        inner: NxkInner,
        local_node: LocalNode<Curve25519>,
        controller: Controller<Self>,
        _phantom: PhantomData<C>,
    }

    impl<C: Send> Resource for NxkResource<C> {
        type Id = RawFd;
        type Context = NxkContext;
        type Cmd = C;
        type Error = io::Error;

        fn with(context: Self::Context, controller: Controller<Self>) -> Result<Self, Self::Error>
        where
            Self: Sized,
        {
            let inner = match context.method {
                NxkMethod::Listen(socket_addr) => {
                    NxkInner::Listener(net::TcpListener::bind(socket_addr)?)
                }
                NxkMethod::Accept(tcp_stream, remote_socket_addr) => NxkInner::Session(
                    NxkSession::accept(tcp_stream, remote_socket_addr, context.local_node.clone()),
                ),
                NxkMethod::Connect(nsh_addr) => {
                    NxkInner::Session(NxkSession::connect(nsh_addr, context.local_node.clone())?)
                }
            };
            Ok(Self {
                inner,
                controller,
                local_node: context.local_node,
                _phantom: none!(),
            })
        }

        fn id(&self) -> Self::Id {
            match &self.inner {
                NxkInner::Listener(listener) => listener.as_raw_fd(),
                NxkInner::Session(session) => session.as_raw_fd(),
            }
        }

        fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
            match self.inner {
                NxkInner::Listener(ref listener) => {
                    let (stream, peer_socket_addr) = listener.accept()?;
                    let nsh_info = NxkContext {
                        method: NxkMethod::Accept(stream, peer_socket_addr),
                        local_node: self.local_node.clone(),
                    };
                    self.controller
                        .connect(nsh_info)
                        .map_err(|_| io::ErrorKind::NotConnected)?;
                }
                NxkInner::Session(ref mut session) => {
                    if io.is_readable { /* TODO: Do read */ }
                    if io.is_writable {
                        session.flush()?;
                    }
                }
            }
            Ok(())
        }

        fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
            match self.inner {
                NxkInner::Listener(_) => panic!("data sent to TCP listener"),
                NxkInner::Session(ref mut stream) => {
                    // TODO: Do writes, like
                    // stream.write_all(&data)?
                }
            };
            Ok(())
        }
    }
}
