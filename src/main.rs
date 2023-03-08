use std::{fs, io};
use std::mem::replace;
use std::path::Path;
use std::process::Stdio;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use log::{debug, error, info, LevelFilter};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::runtime::Handle;

use crate::template::Template as _;

mod template;

const CONFIG_FILE_NAME: &str = "ip-tester.toml";
const IP_FILE_NAME: &str = "ip-v4.txt";
const OUTBOUND_TEMPLATE_FILE_NAME: &str = "outbound-template.json";
const SING_BOX_TEMPLATE_FILE_NAME: &str = "sing-box-template.json";
const SING_BOX_CONFIG_FILE_NAME: &str = "sing-box-test-config.json";
const RTT_RESULT_FILE_NAME: &str = "rtt_result.txt";
const RTT_RESULT_CACHE_FILE_NAME: &str = "rtt_result_cache.toml";


#[derive(Serialize, Deserialize, Clone)]
struct Config {
    port_base: u16,
    max_connection_count: usize,
    domain: String,
    path: String,
    listen_ip: String,
    max_rtt: u64,
    body: String,
}

impl Config {
    fn from_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_str = fs::read_to_string(&path).context(format!("unable to load config from {:?}", path.as_ref()))?;
        Self::from_str(config_str.as_str()).context(format!("unable to parse config from {:?}", path.as_ref()))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port_base: 31000,
            max_connection_count: 200,
            domain: "127.0.0.1".into(),
            path: "/".into(),
            listen_ip: "127.0.0.2".into(),
            max_rtt: 1000,
            body: "rtt tester".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct RttResultCache {
    current_subnet: usize,
    current_subnet_start: usize,
}


impl RttResultCache {
    fn from_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let rtt_result_cache_str = fs::read_to_string(&path).context(format!("unable to load rtt result cache from {:?}", path.as_ref()))?;
        Self::from_str(rtt_result_cache_str.as_str()).context(format!("unable to parse rtt result cache from {:?}", path.as_ref()))
    }
}


#[derive(Debug)]
struct Subnet {
    start: u32,
    count: usize,
}

impl Subnet {
    fn from_str(s: &str) -> Result<Self> {
        lazy_static! {
            static ref RE_IP_CHECK: Regex = Regex::new(r"^((25[0-5]|(2[0-4]|1\d|[1-9]|)\d)\.?\b){4}/(3[0-2]|[1-2]\d|\d)$").unwrap();
            static ref RE_IP_MATCH: Regex = Regex::new(r"(\d+).(\d+).(\d+).(\d+)/(\d+)$").unwrap();
        }
        if !RE_IP_CHECK.is_match(s) {
            return Err(anyhow!("invalid sub_net: {s}"));
        }
        let cap = RE_IP_MATCH.captures(s).unwrap();
        let mut ip_array = [0; 4];
        for (i, it) in ip_array.iter_mut().rev().enumerate() {
            *it = u8::from_str(&cap[i + 1])?
        }
        let subnet_length = 32 - usize::from_str(&cap[5])?;
        let mut ret = Self {
            start: ip_array.iter().enumerate()
                .fold(0_u32, |acc, (i, &x)| acc + ((x as u32) << (8 * i))),
            count: 1 << subnet_length,
        };

        let mask = (1_u32 << subnet_length) - 1;
        if ret.start & mask == 0 {
            if mask != 0 {
                ret.start += 1;
                ret.count -= 2;
            }
        } else {
            // info!("{} {}", (ret.start & mask) as usize, ret.count);
            ret.count -= (ret.start & mask) as usize;
            if ret.count > 0 {
                ret.count -= 1;
            }
        }
        Ok(ret)
    }

    fn get_ip_str(ip: u32) -> String {
        format!("{}.{}.{}.{}", ip >> 24, (ip >> 16) & 0xff, (ip >> 8) & 0xff, ip & 0xff)
    }

    fn get_ip_list(&self, start: usize, count: usize) -> Vec<String> {
        let mut ret = Vec::new();
        for i in start..start + count {
            if i >= self.count {
                break;
            }
            ret.push(Self::get_ip_str(self.start + (i as u32)));
        }
        ret
    }
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Self>> {
        let ip_segments_str = fs::read_to_string(&path).context(format!("unable to load ip segments from {:?}", path.as_ref()))?;
        ip_segments_str.split('\n').map(|s| s.replace('\r', "").replace(' ', ""))
            .filter_map(|s| if s.is_empty() { None } else { Some(Self::from_str(s.as_str())) }).collect()
    }
}

async fn test_rtt(config: &Config, idx: usize) -> Result<u64> {
    let url = format!("http://{}{}", config.domain, config.path);
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("socks5://{}:{}", config.listen_ip, config.port_base + idx as u16))?)
        .timeout(Duration::from_millis(config.max_rtt))
        .build()?;
    let start = SystemTime::now();
    let body = client.get(url).send()
        .await?
        .text()
        .await?;
    let rtt = SystemTime::now().duration_since(start)?.as_millis() as u64;
    if body == config.body {
        Ok(rtt)
    } else {
        Err(anyhow!("expected body: {:?}, current body: {body:?}", config.body))
    }
}

struct SingBox {
    child: Child,
}

impl SingBox {
    async fn new(config_file_name: &str) -> Result<Self> {
        let mut child = Command::new("./sing-box")
            .args(["run", "-c", config_file_name])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn().context("Failed to start sing box process")?;
        let mut tmp_buf = [0_u8];
        if let Err(read_stdout_err) = child.stdout.as_mut().unwrap().read_exact(&mut tmp_buf).await.context("Sing box process read_exact stdout failed.") {
            let status = child.wait().await.context("Sing box process wait failed.")?;
            let mut stderr_output = Vec::new();
            child.stderr.as_mut().unwrap().read_to_end(&mut stderr_output).await.context("Sing box process read_to_end stderr failed.")?;
            let stderr_output_str = String::from_utf8(stderr_output.clone())?;
            error!("{read_stdout_err:?}\noutput: \n{stderr_output_str}");
            return Err(read_stdout_err.context(format!("Sing box process exited. status: {status:?}")));
        }
        Ok(Self {
            child
        })
    }
}

impl Drop for SingBox {
    fn drop(&mut self) {
        tokio::task::block_in_place(move || {
            Handle::current().block_on(async {
                if let Err(err) = self.child.kill().await {
                    error!("self.child.kill failed: {err:?}");
                } else {
                    debug!("child kill!");
                }
            });
        });
    }
}

#[derive(Debug)]
struct RttResult {
    res: Vec<(u64, String)>,
    buf: Vec<(u64, String)>,
}

impl RttResult {
    fn new() -> Self {
        Self {
            res: Vec::new(),
            buf: Vec::new(),
        }
    }

    fn add_result(&mut self, ip: String, rtt: u64) {
        self.buf.push((rtt, ip));
    }

    fn from_string_list(s: &Vec<String>) -> Result<Self> {
        lazy_static! {
            static ref RE_RTT_RESULT_MATCH: Regex = Regex::new(r"^ip: (((25[0-5]|(2[0-4]|1\d|[1-9]|)\d)\.?\b){4}),rtt: (\d+)$").unwrap();
        }
        let mut ret = Self::new();

        for line in s {
            let res = RE_RTT_RESULT_MATCH.captures(line);
            if let Some(res) = res {
                ret.res.push((u64::from_str(&res[5]).unwrap(), res[1].into()));
            } else {
                return Err(anyhow!("match failed: {line:?}"));
            }
        }
        Ok(ret)
    }
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let rtt_results_str = fs::read_to_string(&path).context(format!("unable to load rtt result from {:?}", path.as_ref()))?;
        let rtt_results_str_array = rtt_results_str.split('\n')
            .map(|s| s.replace('\r', ""))
            .filter_map(|s| if s.is_empty() { None } else { Some(s) })
            .collect();
        Self::from_string_list(&rtt_results_str_array)
    }

    fn dump(&mut self) -> String {
        let mut new_res = Vec::new();
        let mut i = 0_usize;
        let mut j = 0_usize;
        self.buf.sort();

        let mut res_data = self.res.get_mut(i).map(|x| replace(x, Default::default()));
        let mut buf_data = self.buf.get_mut(j).map(|x| replace(x, Default::default()));
        while i < self.res.len() || j < self.buf.len() {
            if buf_data.is_none() {
                new_res.push(res_data.unwrap());
                i += 1;
                res_data = self.res.get_mut(i).map(std::mem::take);
                continue;
            }

            if res_data.is_none() {
                new_res.push(buf_data.unwrap());
                j += 1;
                buf_data = self.buf.get_mut(j).map(std::mem::take);
                continue;
            }

            if res_data.as_ref().unwrap().0 < buf_data.as_ref().unwrap().0 {
                new_res.push(res_data.unwrap());
                i += 1;
                res_data = self.res.get_mut(i).map(std::mem::take);
            } else {
                new_res.push(buf_data.unwrap());
                j += 1;
                buf_data = self.buf.get_mut(j).map(std::mem::take);
            }
        }


        self.res = new_res;
        self.buf.clear();
        let mut ret = String::new();

        for (rtt, ip) in &self.res {
            ret.push_str(format!("ip: {ip},rtt: {rtt}\n").as_str());
        }
        ret
    }

    fn dump_to_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let output = self.dump();
        fs::write(path, &output)?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::formatted_builder().filter_level(LevelFilter::Info).init();

    let config: Config = match Config::from_file(CONFIG_FILE_NAME) {
        Ok(config) => config,
        Err(err) => {
            if let Some(io::Error { .. }) = err.downcast_ref() {
                info!("Unable to load config from {CONFIG_FILE_NAME}, create new config.");
                let config = Config::default();
                fs::write(CONFIG_FILE_NAME, toml::to_string(&config)?).context(format!("unable to save config to {CONFIG_FILE_NAME:?}"))?;
                config
            } else {
                return Err(err);
            }
        }
    };

    let outbound_template = template::Outbound::from_file(OUTBOUND_TEMPLATE_FILE_NAME)?;
    let subnets = Subnet::from_file(IP_FILE_NAME)?;
    let sing_box_template = template::SingBoxConfig::from_file(SING_BOX_TEMPLATE_FILE_NAME)?;

    let mut rtt_result = match RttResult::from_file(RTT_RESULT_FILE_NAME) {
        Ok(rtt_result) => {
            info!("Load rtt result {} success", rtt_result.res.len());
            rtt_result
        }
        Err(err) => {
            if let Some(io::Error { .. }) = err.downcast_ref() {
                info!("Can not load rtt result. Create new rtt result.");
                RttResult::new()
            } else {
                info!("Can not load rtt result: {err:?}");
                return Err(err);
            }
        }
    };

    let mut rtt_result_cache = match RttResultCache::from_file(RTT_RESULT_CACHE_FILE_NAME) {
        Ok(rtt_result_cache) => {
            let subnet_count = if let Some(subnet) = subnets.get(rtt_result_cache.current_subnet) {
                subnet.count
            } else {
                return Err(anyhow!("Can not load rtt result cache. current_subnet: {}, but subnets.len(): {}", rtt_result_cache.current_subnet, subnets.len()));
            };

            if rtt_result_cache.current_subnet_start < subnet_count {
                info!("Load rtt result cache success: {rtt_result_cache:?}");
                rtt_result_cache
            } else {
                return Err(anyhow!("Can not load rtt result cache. current_subnet_start: {}, but subnet.count: {subnet_count}", rtt_result_cache.current_subnet_start));
            }
        }

        Err(err) => {
            if let Some(io::Error { .. }) = err.downcast_ref() {
                info!("Can not load rtt result cache. Create new rtt result cache.");
                RttResultCache::default()
            } else {
                info!("Can not load rtt result cache: {err:?}");
                return Err(err);
            }
        }
    };
    let all_ip_count = subnets.iter()
        .fold(0, |acc, subnet| acc + subnet.count);
    let start_ip_count = subnets[..rtt_result_cache.current_subnet].iter()
        .fold(0, |acc, subnet| acc + subnet.count)
        + rtt_result_cache.current_subnet_start;

    let progress_bar = ProgressBar::new(all_ip_count as u64);
    progress_bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{wide_bar:.cyan/blue}] [{pos:>7}/{len:7}] {percent}% ({elapsed_precise}/{duration_precise})",
        )
            .unwrap()
            .progress_chars("#>-"),
    );
    progress_bar.set_position(start_ip_count as u64);
    progress_bar.reset_eta();

    while rtt_result_cache.current_subnet != subnets.len() {
        fs::write(RTT_RESULT_CACHE_FILE_NAME, toml::to_string(&rtt_result_cache)?).context(format!("unable to save rtt result cache to {RTT_RESULT_CACHE_FILE_NAME:?}"))?;


        let mut ips: Vec<String> = Vec::new();
        while rtt_result_cache.current_subnet < subnets.len() && ips.len() < config.max_connection_count {
            let subnet = &subnets[rtt_result_cache.current_subnet];
            let tmp_ips = subnet.get_ip_list(rtt_result_cache.current_subnet_start, config.max_connection_count - ips.len());
            rtt_result_cache.current_subnet_start += tmp_ips.len();
            if rtt_result_cache.current_subnet_start == subnet.count {
                rtt_result_cache.current_subnet_start = 0;
                rtt_result_cache.current_subnet += 1;
            }
            ips.extend_from_slice(&tmp_ips);
        }

        let sing_box_config = sing_box_template.generate(&outbound_template, &ips, config.listen_ip.clone(), config.port_base);
        let sing_box = SingBox::new(SING_BOX_CONFIG_FILE_NAME).await?;

        fs::write(SING_BOX_CONFIG_FILE_NAME, serde_json::to_string_pretty(&sing_box_config)?)?;
        let mut tasks = Vec::new();

        for i in 0..ips.len() {
            let config = config.clone();
            // TODO 重写
            tasks.push(tokio::task::spawn(async move {
                let config = config;
                let i = i.clone();
                let res = test_rtt(&config, i).await;
                res
            }));
        }


        let mut count = 0;
        for (i, task) in tasks.iter_mut().enumerate() {
            let res = task.await?;
            if res.is_ok() {
                let rtt = res.unwrap();
                let log_str = format!("ip:{},rtt:{}", ips[i], rtt);
                progress_bar.println(log_str.as_str());
                debug!("{log_str}");
                rtt_result.add_result(ips[i].clone(), rtt);
                count += 1;
            }
        }
        progress_bar.inc(tasks.len() as u64);
        if count != 0 {
            rtt_result.dump_to_file(RTT_RESULT_FILE_NAME)?;
        }
        let log_str = format!("Test success count: {count}/{} {}->{} ", ips.len(), ips[0], ips[ips.len() - 1]);
        progress_bar.println(log_str.as_str());
        debug!("{log_str}");

        drop(sing_box);
    }
    progress_bar.finish_with_message("finish!");
    Ok(())
}
