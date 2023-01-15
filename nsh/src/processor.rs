use cyphernet::addr::{InetHost, NetAddr};
use std::net::TcpStream;
use std::os::fd::RawFd;
use std::process;
use std::process::Stdio;
use std::str::FromStr;

use cyphernet::{ed25519, Cert, Digest, Sha256};
use netservices::LinkDirection;
use reactor::Resource;

use crate::command::Command;
use crate::server::{Action, Delegate};
use crate::{Session, Transport};

#[derive(Debug)]
pub struct Processor {
    cert: Cert<ed25519::Signature>,
    signer: ed25519::PrivateKey,
    proxy_addr: NetAddr<InetHost>,
    force_proxy: bool,
}

impl Processor {
    pub fn with(
        cert: Cert<ed25519::Signature>,
        signer: ed25519::PrivateKey,
        proxy_addr: NetAddr<InetHost>,
        force_proxy: bool,
    ) -> Self {
        Self {
            cert,
            signer,
            proxy_addr,
            force_proxy,
        }
    }
}

impl Delegate for Processor {
    fn accept(&self, connection: TcpStream) -> Session {
        Session::accept::<{ Sha256::OUTPUT_LEN }>(
            connection,
            self.cert.clone(),
            self.signer.clone(),
        )
    }

    fn new_client(&mut self, _id: RawFd, key: ed25519::PublicKey) -> Vec<Action> {
        log::debug!(target: "nsh", "Remote {key} is connected");
        vec![]
    }

    fn input(&mut self, fd: RawFd, data: Vec<u8>) -> Vec<Action> {
        let mut action_queue = vec![];

        let cmd = match String::from_utf8(data) {
            Ok(cmd) => cmd,
            Err(err) => {
                log::warn!(target: "nsh", "Non-UTF8 command from {fd}: {err}");
                action_queue.push(Action::Send(fd, b"NON_UTF8_COMMAND".to_vec()));
                action_queue.push(Action::UnregisterTransport(fd));
                return action_queue;
            }
        };

        let Ok(cmd) = Command::from_str(&cmd) else {
            action_queue.push(Action::Send(fd, format!("INVALID_COMMAND").as_bytes().to_vec()));
            action_queue.push(Action::UnregisterTransport(fd));
            return action_queue;
        };

        log::info!(target: "nsh", "Executing '{cmd}' for {fd}");
        match cmd {
            Command::ECHO => {
                match process::Command::new("sh")
                    .arg("-c")
                    .arg("echo")
                    .stdout(Stdio::piped())
                    .output()
                {
                    Ok(output) => {
                        log::debug!(target: "nsh", "Command executed successfully; {} bytes of output collected", output.stdout.len());
                        action_queue.push(Action::Send(fd, output.stdout));
                    }
                    Err(err) => {
                        log::error!(target: "nsh", "Error executing command: {err}");
                        action_queue.push(Action::Send(fd, err.to_string().as_bytes().to_vec()));
                        action_queue.push(Action::UnregisterTransport(fd));
                    }
                }
            }
            Command::Forward { hop, command } => {
                // TODO: Ensure that the host key equals the key provided during authentication
                let session = match Session::connect_nonblocking::<{ Sha256::OUTPUT_LEN }>(
                    hop.addr,
                    self.cert,
                    self.signer.clone(),
                    self.proxy_addr.clone(),
                    self.force_proxy,
                ) {
                    Ok(session) => session,
                    Err(err) => {
                        action_queue.push(Action::Send(
                            fd,
                            format!("Failure: {err}").as_bytes().to_vec(),
                        ));
                        return action_queue;
                    }
                };
                match Transport::with_session(session, LinkDirection::Outbound) {
                    Ok(transport) => {
                        let id = transport.id();
                        action_queue.push(Action::RegisterTransport(transport));
                        action_queue
                            .push(Action::Send(id, command.to_string().as_bytes().to_vec()));
                    }
                    Err(err) => {
                        action_queue.push(Action::Send(
                            fd,
                            format!("Failure: {err}").as_bytes().to_vec(),
                        ));
                        action_queue.push(Action::UnregisterTransport(fd));
                    }
                }
            }
        };

        action_queue
    }
}
