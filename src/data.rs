use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;

use cidr::errors::NetworkParseError;
use cidr::IpCidr;
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
impl Loadable<Self> for Vec<Subnet> {
    fn from_str(s: &str) -> error::Result<Self> {
        lazy_static! {
            static ref RE_V4_SUBNET_MATCH: Regex =
                Regex::new(r"(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}/\d{1,3})").unwrap();
            static ref RE_V4_MATCH: Regex =
                Regex::new(r"(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})").unwrap();
        }
        let mut ret = Vec::new();

        for cap in RE_V4_SUBNET_MATCH.captures_iter(s) {
            match Subnet::from_str(&cap[0]) {
                Ok(subnet) => ret.push(subnet),
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

#[derive(Debug)]
pub struct Subnet(IpCidr);

impl FromStr for Subnet {
    type Err = NetworkParseError;
    fn from_str(s: &str) -> Result<Self, NetworkParseError> {
        IpCidr::from_str(s).map(Self)
    }
}

impl Subnet {
    pub fn len(&self) -> usize {
        (1 << (self.0.family().len() - self.0.network_length())).min(256)
    }

    pub fn get_ip(&self, idx: usize) -> Option<IpAddr> {
        match self.0 {
            IpCidr::V4(cidr_v4) => {
                let ipv4 = u32::from(cidr_v4.first_address()) + idx as u32;
                Some(Ipv4Addr::from(ipv4).into())
            }
            IpCidr::V6(cidr_v6) => {
                let ipv6 = u128::from(cidr_v6.first_address()) + idx as u128;
                Some(Ipv6Addr::from(ipv6).into())
            }
        }
    }
}

// impl Subnet {
//     fn from_str(s: &str) -> Result<Self> {
//
//     }
//
//     fn get_ip_str(ip: u32) -> String {
//         format!(
//             "{}.{}.{}.{}",
//             ip >> 24,
//             (ip >> 16) & 0xff,
//             (ip >> 8) & 0xff,
//             ip & 0xff
//         )
//     }
//
//     fn get_ip_list(&self, start: usize, count: usize) -> Vec<String> {
//         let mut ret = Vec::new();
//         for i in start..start + count {
//             if i >= self.count {
//                 break;
//             }
//             ret.push(Self::get_ip_str(self.start + (i as u32)));
//         }
//         ret
//     }
//     fn from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Self>> {
//         let ip_segments_str = fs::read_to_string(&path).context(format!(
//             "unable to load ip segments from {:?}",
//             path.as_ref()
//         ))?;
//         ip_segments_str
//             .split('\n')
//             .map(|s| s.replace('\r', "").replace(' ', ""))
//             .filter_map(|s| {
//                 if s.is_empty() {
//                     None
//                 } else {
//                     Some(Self::from_str(s.as_str()))
//                 }
//             })
//             .collect()
//     }
// }
//
