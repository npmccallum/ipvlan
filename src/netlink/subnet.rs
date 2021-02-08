// SPDX-License-Identifier: Apache-2.0

use std::net::IpAddr;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Subnet {
    address: IpAddr,
    prefix: u8,
}

pub enum Error {
    Address(std::net::AddrParseError),
    Prefix(std::num::ParseIntError),
    Field,
}

impl From<Error> for std::io::Error {
    #[inline]
    fn from(_value: Error) -> Self {
        std::io::ErrorKind::InvalidInput.into()
    }
}

impl From<std::net::AddrParseError> for Error {
    #[inline]
    fn from(value: std::net::AddrParseError) -> Self {
        Self::Address(value)
    }
}

impl From<std::num::ParseIntError> for Error {
    #[inline]
    fn from(value: std::num::ParseIntError) -> Self {
        Self::Prefix(value)
    }
}

impl std::fmt::Display for Subnet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.address, self.prefix)
    }
}

impl FromStr for Subnet {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split('/');
        let addr = split.next().ok_or(Error::Field)?;
        let pfix = split.next().ok_or(Error::Field)?;
        if split.next().is_some() {
            return Err(Error::Field);
        }

        Ok(Self::new(addr.parse()?, pfix.parse()?))
    }
}

impl Subnet {
    fn mask(addr: IpAddr, prefix: u8) -> IpAddr {
        match addr {
            IpAddr::V4(addr) => {
                let shift = 32 - prefix;
                let mask = !0 >> shift << shift;
                let addr = u32::from(addr) & mask;
                addr.to_be_bytes().into()
            }

            IpAddr::V6(addr) => {
                let shift = 128 - prefix;
                let mask = !0 >> shift << shift;
                let addr = u128::from(addr) & mask;
                addr.to_be_bytes().into()
            }
        }
    }

    #[inline]
    pub fn new(address: IpAddr, prefix: u8) -> Self {
        Self {
            address: Self::mask(address, prefix),
            prefix,
        }
    }

    #[inline]
    pub fn address(&self) -> IpAddr {
        self.address
    }

    #[inline]
    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    pub fn random(&self) -> IpAddr {
        let rand = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        match self.address() {
            IpAddr::V4(addr) => {
                let rand = (rand as u32) << self.prefix >> self.prefix;
                (u32::from(addr) | rand).to_be_bytes().into()
            }

            IpAddr::V6(addr) => {
                let rand = rand << self.prefix >> self.prefix;
                (u128::from(addr) | rand).to_be_bytes().into()
            }
        }
    }

    #[inline]
    pub fn contains(&self, addr: IpAddr) -> bool {
        match (self.address, addr) {
            (IpAddr::V4(..), IpAddr::V4(..)) => (),
            (IpAddr::V6(..), IpAddr::V6(..)) => (),
            _ => return false,
        }

        Self::mask(addr, self.prefix) == self.address
    }
}
