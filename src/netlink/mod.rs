// SPDX-License-Identifier: Apache-2.0

mod address;
mod connection;
mod interface;
mod subnet;

pub use address::Address;
use connection::Connection;
pub use interface::Interface;
pub use subnet::Subnet;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Decode(netlink_packet_route::DecodeError),
}

impl From<Error> for std::io::Error {
    #[inline]
    fn from(value: Error) -> Self {
        match value {
            Error::Decode(..) => std::io::ErrorKind::InvalidInput.into(),
            Error::Io(e) => e,
        }
    }
}

impl From<std::io::ErrorKind> for Error {
    #[inline]
    fn from(value: std::io::ErrorKind) -> Self {
        Error::Io(value.into())
    }
}

impl From<std::io::Error> for Error {
    #[inline]
    fn from(value: std::io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<netlink_packet_route::DecodeError> for Error {
    #[inline]
    fn from(value: netlink_packet_route::DecodeError) -> Self {
        Error::Decode(value)
    }
}
