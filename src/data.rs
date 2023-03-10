use std::collections::HashSet;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;

use cidr::errors::NetworkParseError;
use cidr::{IpCidr, IpInet, Ipv4Inet, Ipv6Inet};
use lazy_static::lazy_static;
use log::warn;
use regex::Regex;

use crate::error;

pub trait Loadable<T> {
    fn from_str(s: &str) -> error::Result<T>;

    fn load<P: AsRef<Path>>(path: P) -> error::Result<T> {
        Self::from_str(
            fs::read_to_string(&path)
                .map_err(|err| error::ErrorKind::fs(err, &path))?
                .as_str(),
        )
    }
}

pub trait Savable {
    fn to_string(&self) -> error::Result<String>;
    fn save<P: AsRef<Path>>(&self, path: P) -> error::Result<()> {
        fs::write(&path, self.to_string()?).map_err(|err| error::ErrorKind::fs(err, &path).into())
    }
}

// TODO add ipv6 support
impl Loadable<Self> for HashSet<Subnet> {
    fn from_str(s: &str) -> error::Result<Self> {
        lazy_static! {
            static ref RE_V4_SUBNET_MATCH: Regex =
                Regex::new(r"(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}/\d{1,3})").unwrap();
            static ref RE_V4_MATCH: Regex =
                Regex::new(r"(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})").unwrap();
        }
        let mut ret = HashSet::new();

        for cap in RE_V4_SUBNET_MATCH.captures_iter(s) {
            match Subnet::from_str(&cap[0]) {
                Ok(subnet) => {
                    ret.insert(subnet);
                }
                Err(err) => {
                    warn!("parse {:?} to subnet failed: {err:?} , skip.", &cap[0]);
                }
            }
        }

        // TODO: 修好它
        // for cap in RE_V4_MATCH.captures_iter(s) {
        //     match IpAddr::from_str(&cap[0]) {
        //         Ok(ip_addr) => {
        //             ret.push(Subnet::new(ip_addr, Family::Ipv4.len()).unwrap());
        //         }
        //         Err(err) => {
        //             warn!("parse {:?} to subnet failed: {err:?} , skip.", &cap[0]);
        //         }
        //     }
        // }
        Ok(ret)
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Subnet {
    pub cidr: IpCidr,
    pub enable: bool,
}

impl FromStr for Subnet {
    type Err = NetworkParseError;
    fn from_str(s: &str) -> Result<Self, NetworkParseError> {
        let ip_inet = IpInet::from_str(s)?;
        Ok(Self {
            cidr: IpCidr::new(ip_inet.first_address(), ip_inet.network_length())?,
            enable: false,
        })
    }
}

impl Subnet {
    pub fn len(&self) -> usize {
        1 << (self.cidr.family().len() - self.cidr.network_length())
    }

    pub fn get_ip(&self, idx: usize) -> Option<IpInet> {
        if idx >= self.len() {
            return None;
        }

        match self.cidr {
            IpCidr::V4(cidr_v4) => {
                let ipv4 = u32::from(cidr_v4.first_address()) + idx as u32;
                Some(
                    Ipv4Inet::new(Ipv4Addr::from(ipv4), cidr_v4.network_length())
                        .unwrap()
                        .into(),
                )
            }
            IpCidr::V6(cidr_v6) => {
                let ipv6 = u128::from(cidr_v6.first_address()) + idx as u128;
                Some(
                    Ipv6Inet::new(Ipv6Addr::from(ipv6), cidr_v6.network_length())
                        .unwrap()
                        .into(),
                )
            }
        }
    }
}
