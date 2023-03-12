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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port_base: 31000,
            max_connection_count: 50,
            server_url: "http://127.0.0.1/".into(),
            cdn_url: "http://archlinux.cloudflaremirrors.com".into(),
            listen_ip: "127.0.0.2".into(),
            max_rtt: 1000,
            server_res_body: "".into(),
            cdn_res_body: "archlinux".into(),
        }
    }
}
