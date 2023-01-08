use std::os::fd::RawFd;
use std::process::{Command, Stdio};

use cyphernet::crypto::ed25519::PublicKey;

use crate::server::{Action, Delegate};

#[derive(Debug, Default)]
pub struct RemoteShell {}

impl Delegate for RemoteShell {
    fn new_client(&mut self, _id: RawFd, key: PublicKey) -> Vec<Action> {
        log::debug!(target: "nsh", "Remote {key} is connected");
        vec![]
    }

    fn input(&mut self, fd: RawFd, data: Vec<u8>) -> Vec<Action> {
        let mut action_queue = vec![];

        let cmd = match String::from_utf8(data) {
            Ok(cmd) => {
                log::info!(target: "nsh", "Executing `{cmd}` for {fd}");
                cmd
            }
            Err(err) => {
                log::warn!(target: "nsh", "Non-UTF8 command from {fd}: {err}");
                action_queue.push(Action::Send(fd, b"NON_UTF8_COMMAND".to_vec()));
                action_queue.push(Action::UnregisterTransport(fd));
                return action_queue;
            }
        };
        match Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .output()
        {
            Ok(output) => {
                log::debug!(target: "nsh", "Command executed successfully; {} bytes of output collected", output.stdout.len());
                action_queue.push(Action::Send(fd, output.stdout));
                action_queue.push(Action::UnregisterTransport(fd));
            }
            Err(err) => {
                log::error!(target: "nsh", "Error executing command: {err}");
                action_queue.push(Action::Send(fd, err.to_string().as_bytes().to_vec()));
                action_queue.push(Action::UnregisterTransport(fd));
            }
        }

        action_queue
    }
}
