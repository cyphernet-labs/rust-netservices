mod noise;

use crate::SplitIo;

pub trait Encrypt {
    fn encrypt(&mut self, data: &[u8]) -> Vec<u8>;
}

pub trait Decrypt {
    type Error: std::error::Error;

    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

pub trait Transcode: SplitIo + Encrypt + Decrypt {
    type Encryptor: Encrypt;
    type Decryptor: Decrypt;
}
