use serde::{Deserialize, Serialize};

use cdn_ip_tester_derive::{TomlLoadable, TomlSavable};

#[derive(Serialize, Deserialize, Clone, TomlLoadable, TomlSavable)]
pub struct Config {
    pub port_base: u16,
    pub max_connection_count: usize,
    pub server_url: String,
    pub cdn_url: String,
    pub listen_ip: String,
    pub max_rtt: u64,
    pub server_res_body: String,
    pub cdn_res_body: String,
    pub max_subnet_len: usize,
}
