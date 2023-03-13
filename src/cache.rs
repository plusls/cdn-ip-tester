use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use cidr::{IpCidr, IpInet};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};

use cdn_ip_tester_derive::{TomlLoadable, TomlSavable};

use crate::data::{Loadable, Savable, Subnet};
use crate::error::{DeserializedError, Result};

#[derive(Debug, Clone)]
pub struct RttResult {
    cdn_rtt: u64,
    server_rtt: u64,
}

impl Eq for RttResult {}

impl PartialEq<Self> for RttResult {
    fn eq(&self, other: &Self) -> bool {
        self.server_rtt == other.server_rtt && self.cdn_rtt == other.cdn_rtt
    }
}

impl PartialOrd<Self> for RttResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RttResult {
    fn cmp(&self, other: &Self) -> Ordering {
        self.server_rtt
            .cmp(&other.server_rtt)
            .then(self.cdn_rtt.cmp(&other.cdn_rtt))
    }
}

impl RttResult {
    pub(crate) fn new(server_rtt: u64, cdn_rtt: u64) -> Self {
        Self {
            cdn_rtt,
            server_rtt,
        }
    }
}

#[derive(Debug, Default)]
pub struct RttResults {
    res: HashMap<IpInet, RttResult>,
    sorted_res_keys: Vec<IpInet>,
    tmp_key_set: HashSet<IpInet>,
}

impl RttResults {
    pub fn len(&self) -> usize {
        self.res.len()
    }

    pub fn add_result(&mut self, ip_inet: IpInet, rtt_result: RttResult) {
        self.tmp_key_set.insert(ip_inet);
        // 永远用最新的结果进行覆盖
        self.res.insert(ip_inet, rtt_result);
    }

    fn from_string_list(s: &Vec<String>) -> Result<Self> {
        lazy_static! {
            static ref RE_RTT_RESULT_MATCH: Regex =
                Regex::new(r"^ip: (.{2,45}/\d+), server_rtt: (\d+), cdn_rtt: (\d+)$").unwrap();
        }
        let mut ret = Self::default();

        for line in s {
            let res = RE_RTT_RESULT_MATCH.captures(line);
            if let Some(res) = res {
                let ip_inet = IpInet::from_str(&res[1]).map_err(DeserializedError::from)?;
                ret.res.insert(
                    ip_inet,
                    RttResult::new(
                        u64::from_str(&res[2]).map_err(DeserializedError::from)?,
                        u64::from_str(&res[3]).map_err(DeserializedError::from)?,
                    ),
                );
                ret.sorted_res_keys.push(ip_inet);
            } else {
                return Err(DeserializedError::regex(line.clone(), &RE_RTT_RESULT_MATCH))?;
            }
        }
        ret.sorted_res_keys
            .sort_by_key(|ip_inet| ret.res.get(ip_inet).unwrap());
        Ok(ret)
    }

    pub fn commit(&mut self) {
        let mut new_res = Vec::new();

        if self.tmp_key_set.is_empty() {
            return;
        }
        let mut buf: Vec<IpInet> = self.tmp_key_set.iter().copied().collect();
        buf.sort_by_key(|ip_inet| self.res.get(ip_inet).unwrap());

        let mut i = 0_usize;
        let mut j = 0_usize;
        let mut res_data = self.sorted_res_keys.get(i).cloned();
        let mut buf_data = buf.get(j).cloned();
        while i < self.sorted_res_keys.len() || j < buf.len() {
            if buf_data.is_none() {
                let tmp_res_data = res_data.unwrap();
                i += 1;
                res_data = self.sorted_res_keys.get(i).cloned();
                if !self.tmp_key_set.contains(&tmp_res_data) {
                    new_res.push(tmp_res_data);
                }
                continue;
            }
            if res_data.is_none() {
                new_res.push(buf_data.unwrap());
                j += 1;
                buf_data = buf.get(j).cloned();
                continue;
            }
            let tmp_res_data = res_data.unwrap();
            let tmp_buf_data = buf_data.unwrap();

            if self.res.get(&tmp_res_data).unwrap() < self.res.get(&tmp_buf_data).unwrap() {
                i += 1;
                res_data = self.sorted_res_keys.get(i).cloned();
                if !self.tmp_key_set.contains(&tmp_res_data) {
                    new_res.push(tmp_res_data);
                }
            } else {
                j += 1;
                buf_data = buf.get(j).cloned();
                new_res.push(tmp_buf_data);
            }
        }
        self.sorted_res_keys = new_res;
        self.tmp_key_set.clear();
    }

    pub fn enable_subnets(&self, subnets: &mut [Subnet]) {
        let mut cidr_set: HashSet<IpCidr> = HashSet::new();
        for key in &self.sorted_res_keys {
            cidr_set.insert(IpCidr::new(key.first_address(), key.network_length()).unwrap());
        }

        for subnet in subnets {
            if cidr_set.contains(&subnet.cidr) {
                subnet.enable = true;
            }
        }
    }
}

impl Loadable<Self> for RttResults {
    fn from_str(s: &str) -> Result<Self> {
        let string_list = s
            .split('\n')
            .map(|s| s.replace('\r', ""))
            .filter_map(|s| if s.is_empty() { None } else { Some(s) })
            .collect();
        Self::from_string_list(&string_list)
    }
}

impl Savable for RttResults {
    fn to_string(&self) -> Result<String> {
        let mut ret = String::new();

        for ip_inet in &self.sorted_res_keys {
            let rtt_result = self.res.get(ip_inet).unwrap();
            ret.push_str(
                format!(
                    "ip: {ip_inet}, server_rtt: {}, cdn_rtt: {}\n",
                    rtt_result.server_rtt, rtt_result.cdn_rtt
                )
                .as_str(),
            );
        }
        Ok(ret)
    }
}

#[derive(Serialize, Deserialize, Debug, Default, TomlLoadable, TomlSavable)]
pub struct RttResultCache {
    pub current_subnet: usize,
    pub current_subnet_start: usize,
}
