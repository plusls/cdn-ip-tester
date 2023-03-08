use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub trait Template<T: DeserializeOwned> {
    fn from_file<P: AsRef<Path>>(path: P) -> Result<T> {
        let template_str = fs::read_to_string(&path).context(format!("unable to load template from {:?}", path.as_ref()))?;
        serde_json::from_str(template_str.as_str()).context(format!("unable to parse template from {:?}", path.as_ref()))
    }
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SingBoxConfig {
    inbounds: Vec<Inbound>,
    outbounds: Vec<Outbound>,
    route: Route,
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

impl SingBoxConfig {
    pub fn generate(&self, outbound_template: &Outbound, ips: &[String], listen_ip: String, port_base: u16) -> Self {
        let mut ret = self.clone();
        for (i, ip) in ips.iter().enumerate() {
            let inbound_tag = format!("inbound-{i}");
            let outbound_tag = format!("outbound-{i}");
            ret.inbounds.push(Inbound::new(inbound_tag.clone(), listen_ip.clone(), port_base + i as u16));
            ret.outbounds.push(outbound_template.generate(outbound_tag.clone(), ip.clone()));
            ret.route.rules.push(Rule::new(inbound_tag, outbound_tag));
        }
        ret
    }
}

impl Template<SingBoxConfig> for SingBoxConfig {}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Inbound {
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

impl Inbound {
    pub fn new(tag: String, listen: String, listen_port: u16) -> Self {
        let mut ret = Inbound {
            other: HashMap::new()
        };
        ret.other.insert("type".into(), "socks".into());
        ret.other.insert("tag".into(), tag.into());
        ret.other.insert("listen".into(), listen.into());
        ret.other.insert("listen_port".into(), listen_port.into());
        ret.other.insert("tcp_fast_open".into(), true.into());
        ret.other.insert("users".into(), Vec::<Value>::new().into());
        ret
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outbound {
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

impl Outbound {
    pub fn generate(&self, tag: String, server: String) -> Self {
        let mut ret = self.clone();
        ret.other.insert("tag".into(), tag.into());
        ret.other.insert("server".into(), server.into());
        ret
    }
}

impl Template<Outbound> for Outbound {}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Route {
    rules: Vec<Rule>,
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rule {
    outbound: String,
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

impl Rule {
    pub fn new(inbound: String, outbound: String) -> Self {
        let mut ret = Self {
            outbound,
            other: HashMap::new(),
        };
        ret.other.insert("inbound".into(), vec![inbound].into());
        ret
    }
}

