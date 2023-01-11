use std::io;

use cyphernet::crypto::ed25519::{PublicKey, Signature};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Authenticator {
    pubkey: PublicKey,
    signature: Signature,
    remote_id: Option<PublicKey>,
}

impl Authenticator {
    pub fn new(pubkey: PublicKey, signature: Signature) -> Self {
        Self {
            pubkey,
            signature,
            remote_id: None,
        }
    }

    pub fn certify(&self, writer: &mut impl io::Write) -> io::Result<()> {
        log::debug!(target: "authentication", "Sending auth credentials: {}, {}", self.pubkey, self.signature);
        writer.write_all(self.pubkey.as_slice())?;
        writer.write_all(self.signature.as_slice())
    }

    // TODO: Do the real authentication with challenge
    pub fn verify(&mut self, reader: &mut impl io::Read) -> io::Result<Option<PublicKey>> {
        let mut buf = [0u8; 64];
        reader.read_exact(&mut buf[..32])?;

        debug_assert!(self.remote_id.is_none());
        let pubkey = PublicKey::try_from(&buf[..32]).expect("fixed-length key");

        reader.read_exact(&mut buf)?;
        let sig = Signature::from(buf);

        if pubkey.verify(pubkey.as_slice(), &sig).is_err() {
            log::error!(target: "authentication", "Authentication of {pubkey} failed with sig {sig}");
            return Ok(None);
        }
        log::info!(target: "authentication", "Peer {pubkey} signature {sig} authenticated");

        self.remote_id = Some(pubkey);
        Ok(self.remote_id)
    }

    pub fn remote_id(&self) -> Option<PublicKey> {
        self.remote_id
    }

    pub fn is_auth_complete(&self) -> bool {
        self.remote_id.is_some()
    }
}
