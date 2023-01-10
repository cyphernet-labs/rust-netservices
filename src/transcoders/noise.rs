use cyphernet::noise::framing::{NoiseDecryptor, NoiseEncryptor, NoiseState, NoiseTranscoder};
use cyphernet::noise::HandshakeError;

use crate::transcoders::{Decrypt, Encrypt};

impl Encrypt for NoiseEncryptor {
    fn encrypt(&mut self, data: &[u8]) -> Vec<u8> {
        match self.encrypt_buf(data) {
            Ok(values) => values,
            Err(_) => Vec::new(),
        }
    }
}

impl Decrypt for NoiseDecryptor {
    type Error = HandshakeError;
    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        match self.decrypt_single_message(Some(data)) {
            Ok(Some(data)) => Ok(data),
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(HandshakeError::Encryption(e)),
        }
    }
}

impl<S: NoiseState> Encrypt for NoiseTranscoder<S> {
    fn encrypt(&mut self, data: &[u8]) -> Vec<u8> {
        self.expect_encryptor().encrypt(data)
    }
}

impl<S: NoiseState> Decrypt for NoiseTranscoder<S> {
    type Error = HandshakeError;

    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.expect_decryptor().decrypt(data)
    }
}
