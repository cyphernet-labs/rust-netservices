use std::io;

#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[from]
    #[display("inner")]
    Io(io::Error),

    #[display("resource error {0}")]
    Resource(Box<dyn std::error::Error + Send>),

    #[display("service error {0}")]
    Service(Box<dyn std::error::Error + Send>),
}
