pub mod fd;
pub mod tcp;

pub use fd::FdResource;
pub use tcp::{TcpLocator, TcpSocket};
