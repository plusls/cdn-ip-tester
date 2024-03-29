#![feature(error_generic_member_access)]

use std::collections::HashSet;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cidr::IpInet;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, warn, LevelFilter};
use reqwest::{Client, Url};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::runtime::Handle;

use crate::cache::{RttResult, RttResultCache, RttResults};
use crate::config::Config;
use crate::data::{Loadable, Savable, Subnet};
use crate::error::{DeserializedError, ErrorKind, ReqwestError, Result, TokioError};
use crate::template::{Outbound, SingBoxConfig};

mod cache;
mod config;
mod data;
mod error;
mod template;

const CONFIG_FILE_NAME: &str = "ip-tester.toml";
const OUTBOUND_TEMPLATE_FILE_NAME: &str = "outbound-template.json";
const SING_BOX_TEMPLATE_FILE_NAME: &str = "sing-box-template.json";
const SING_BOX_CONFIG_FILE_NAME: &str = "sing-box-test-config.json";
const RTT_RESULT_FILE_NAME: &str = "result.txt";
const RTT_RESULT_CACHE_FILE_NAME: &str = "result_cache.toml";

async fn do_test_rtt(
    client: Client,
    url: Url,
    expected_body: String,
) -> core::result::Result<u64, ReqwestError> {
    let start = SystemTime::now();
    let res = client
        .get(url)
        .send()
        .await
        .map_err(ReqwestError::network)?;

    let body = res.text().await.map_err(ReqwestError::network)?;
    if !body.contains(expected_body.as_str()) {
        Err(ReqwestError::body_no_match(body, expected_body))?
    }
    Ok(SystemTime::now().duration_since(start).unwrap().as_millis() as u64)
}

async fn test_rtt(config: Arc<Config>, cdn_ip: IpAddr, idx: usize) -> Result<RttResult> {
    let server_client = Client::builder()
        .proxy(
            reqwest::Proxy::all(format!(
                "socks5://{}:{}",
                config.listen_ip,
                config.port_base + idx as u16
            ))
            .map_err(ReqwestError::build)?,
        )
        .timeout(Duration::from_millis(config.max_rtt))
        .build()
        .map_err(ReqwestError::build)?;

    let server_url = Url::parse(config.server_url.as_str()).map_err(DeserializedError::from)?;
    let cdn_ip_string = cdn_ip.to_string();

    let cdn_url = if config.cdn_url.is_empty() {
        Url::parse(format!("http://{}", cdn_ip_string).as_str()).map_err(DeserializedError::from)?
    } else {
        Url::parse(config.cdn_url.as_str()).map_err(DeserializedError::from)?
    };
    let cdn_domain = if let Some(cdn_domain) = cdn_url.domain() {
        cdn_domain
    } else if config.cdn_url.is_empty() {
        cdn_ip_string.as_str()
    } else {
        Err(DeserializedError::custom("Url must have domain, not IP"))?
    };
    if cdn_url.scheme() != "http" && cdn_url.scheme() != "https" {
        Err(DeserializedError::custom(
            "Url scheme must be http or https",
        ))?
    }
    let cdn_url_port = if let Some(cdn_url_port) = cdn_url.port_or_known_default() {
        cdn_url_port
    } else {
        unreachable!()
    };

    let cdn_client = Client::builder()
        .resolve_to_addrs(cdn_domain, &[SocketAddr::new(cdn_ip, cdn_url_port)])
        .timeout(Duration::from_millis(config.max_rtt))
        .build()
        .map_err(ReqwestError::build)?;

    let cdn_expected_body = config.cdn_res_body.clone();
    let cdn_rtt_task = tokio::task::spawn(do_test_rtt(cdn_client, cdn_url, cdn_expected_body));
    let server_expected_body = config.server_res_body.clone();
    let server_rtt_task =
        tokio::task::spawn(do_test_rtt(server_client, server_url, server_expected_body));

    let cdn_rtt_result = cdn_rtt_task.await.map_err(TokioError::from)?;
    let server_rtt_result = server_rtt_task.await.map_err(TokioError::from)?;

    Ok(RttResult::new(server_rtt_result?, cdn_rtt_result?))
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
            .spawn()
            .map_err(ErrorKind::process)?;
        let mut tmp_buf = [0_u8];
        if let Err(read_stdout_err) = child
            .stderr
            .as_mut()
            .unwrap()
            .read_exact(&mut tmp_buf)
            .await
        {
            child.wait().await.map_err(ErrorKind::process)?;
            let mut stderr_output = Vec::new();
            child
                .stderr
                .as_mut()
                .unwrap()
                .read_to_end(&mut stderr_output)
                .await
                .map_err(ErrorKind::process)?;
            let stderr_output_str = String::from_utf8(stderr_output.clone()).unwrap();
            error!("{read_stdout_err}\noutput: \n{stderr_output_str}");
            Err(ErrorKind::process(read_stdout_err))?
        }
        Ok(Self { child })
    }
}

impl Drop for SingBox {
    fn drop(&mut self) {
        tokio::task::block_in_place(move || {
            Handle::current().block_on(async {
                if let Err(err) = self.child.kill().await {
                    error!("self.child.kill failed: {err}");
                } else {
                    debug!("child kill!");
                }
            });
        });
    }
}

async fn test_rtts(
    config: &Arc<Config>,
    sing_box_template: &SingBoxConfig,
    outbound_template: &Outbound,
    data_dir: &str,
    ignore_body_warning: bool,
    progress_bar: &ProgressBar,
    ips: &[IpInet],
) -> Result<Vec<Option<RttResult>>> {
    let sing_box_config = sing_box_template.generate(
        outbound_template,
        &ips.iter()
            .map(|ip_inet| ip_inet.address().to_string())
            .collect::<Vec<String>>(),
        config.listen_ip.clone(),
        config.port_base,
    );

    let sing_box_config_path = format!("{data_dir}/{SING_BOX_CONFIG_FILE_NAME}");
    sing_box_config.save(&sing_box_config_path)?;

    let sing_box = match SingBox::new(&sing_box_config_path).await {
        Ok(sing_box) => sing_box,
        Err(err) => {
            error!("Can not start sing box process: {err}");
            Err(err)?
        }
    };

    let mut tasks = Vec::new();
    let mut ret = Vec::new();
    for (i, &cdn_ip) in ips.iter().enumerate() {
        let config = config.clone();
        tasks.push(tokio::task::spawn(test_rtt(config, cdn_ip.address(), i)));
    }

    for (i, task) in tasks.iter_mut().enumerate() {
        let res = task.await.map_err(TokioError::from)?;

        match res {
            Ok(rtt) => {
                let log_str = format!("ip: {}, rtt: {:?}", ips[i], rtt);
                progress_bar.println(log_str.as_str());
                debug!("{log_str}");
                ret.push(Some(rtt));
            }
            Err(err) => {
                if !ignore_body_warning {
                    if let Some(ReqwestError::BodyNoMatch { .. }) =
                        err.source().unwrap().downcast_ref()
                    {
                        warn!("ip: {} body unmatched: \n{}", ips[i], err);
                    }
                }

                // warn!("ip:{}, err:{:?}", ips[i], err);

                ret.push(None);
            }
        }
    }
    drop(sing_box);
    Ok(ret)
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    ignore_body_warning: bool,
    #[arg(long)]
    ip_file: String,
    #[arg(long, default_value_t = 0)]
    subnet_count: usize,
    #[arg(long)]
    no_cache: bool,
    #[arg(long, default_value = "data")]
    data_dir: String,
    #[arg(long)]
    auto_skip: bool,
    #[arg(long, default_value_t = 10)]
    enable_threshold: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    pretty_env_logger::formatted_builder()
        .filter_level(LevelFilter::Info)
        .init();

    let config_path = format!("{}/{CONFIG_FILE_NAME}", args.data_dir);
    let config: Arc<Config> = match Config::load(&config_path) {
        Ok(config) => Arc::new(config),
        Err(err) => {
            info!("Unable to load config from {config_path}\n{err}");
            return Err(err);
        }
    };

    let outbound_template_path = format!("{}/{OUTBOUND_TEMPLATE_FILE_NAME}", args.data_dir);
    let outbound_template = match Outbound::load(&outbound_template_path) {
        Ok(outbound) => outbound,
        Err(err) => {
            info!("Unable to load outbound template from {outbound_template_path}\n{err}");
            return Err(err);
        }
    };

    let mut subnets: Vec<Subnet> = match HashSet::load(&args.ip_file) {
        Ok(subnets) => subnets,
        Err(err) => {
            info!("Unable to load subnets from {}\n{err}", &args.ip_file);
            return Err(err);
        }
    }
    .iter()
    .map(Subnet::clone)
    .collect();

    let subnets = if args.subnet_count != 0 {
        &mut subnets[..args.subnet_count]
    } else {
        &mut subnets
    };

    let max_subnet_len = subnets
        .iter()
        .fold(0_usize, |max_subnet_len, subnet| {
            max_subnet_len.max(subnet.len())
        })
        .min(config.max_subnet_len);

    info!(
        "Load {} subnets from {:?} success. max_subnet_len: {}",
        subnets.len(),
        args.ip_file,
        max_subnet_len
    );

    let sing_box_template_path = format!("{}/{SING_BOX_TEMPLATE_FILE_NAME}", args.data_dir);
    let sing_box_template = match SingBoxConfig::load(&sing_box_template_path) {
        Ok(sing_box_template) => sing_box_template,
        Err(err) => {
            info!(
                "Unable to load sing box template from {}\n{err}",
                &sing_box_template_path
            );
            return Err(err);
        }
    };

    let mut rtt_results;
    let mut rtt_result_cache;
    let rtt_result_file_name = format!("{}/{RTT_RESULT_FILE_NAME}", args.data_dir);
    let rtt_result_cache_file_name = format!("{}/{RTT_RESULT_CACHE_FILE_NAME}", args.data_dir);

    if args.no_cache {
        info!("no_cache = true, use default rtt result cache and default rtt result");
        rtt_results = RttResults::default();
        rtt_result_cache = RttResultCache::default()
    } else {
        rtt_results = match RttResults::load(&rtt_result_file_name) {
            Ok(rtt_results) => {
                info!(
                    "Load {} rtt results from {rtt_result_file_name} success",
                    rtt_results.len()
                );
                rtt_results
            }
            Err(err) => {
                if let ErrorKind::Fs { .. } = *err.0 {
                    info!("Can not load rtt result. Create new rtt result.");
                    RttResults::default()
                } else {
                    error!("Can not load rtt result: {err}");
                    return Err(err);
                }
            }
        };

        rtt_result_cache = match RttResultCache::load(&rtt_result_cache_file_name) {
            Ok(rtt_result_cache) => {
                if rtt_result_cache.current_subnet >= subnets.len() {
                    Err(DeserializedError::custom(format!( "Can not load rtt result cache. current_subnet: {}, but subnets.len(): {}", rtt_result_cache.current_subnet, subnets.len()).as_str()))?;
                }

                if rtt_result_cache.current_subnet_start >= max_subnet_len {
                    Err(DeserializedError::custom(format!( "Can not load rtt result cache. current_subnet_start: {}, but max_subnet_len: {}", rtt_result_cache.current_subnet_start, max_subnet_len).as_str()))?;
                }
                info!("Load rtt result cache success: {rtt_result_cache:?}");
                rtt_result_cache
            }

            Err(err) => {
                if let ErrorKind::Fs { .. } = *err.0 {
                    info!("Can not load rtt result cache. Create new rtt result cache.");
                    RttResultCache::default()
                } else {
                    error!("Can not load rtt result cache cache: {err}");
                    return Err(err);
                }
            }
        }
    }
    rtt_results.save(&rtt_result_file_name)?;
    rtt_result_cache.save(&rtt_result_cache_file_name)?;

    rtt_results.enable_subnets(subnets);

    fn calc_subnet_len(
        subnet: &Subnet,
        rtt_result_cache: &RttResultCache,
        args: &Args,
        max_subnet_len: usize,
    ) -> usize {
        if !args.auto_skip
            || rtt_result_cache.current_subnet_start < args.enable_threshold
            || subnet.enable
        {
            subnet.len().min(max_subnet_len)
        } else {
            0
        }
    }

    let mut all_ip_count = subnets.iter().fold(0, |acc, subnet| {
        acc + calc_subnet_len(subnet, &rtt_result_cache, &args, max_subnet_len)
    });

    fn calc_start_ip_count(
        subnets: &[Subnet],
        rtt_result_cache: &RttResultCache,
        args: &Args,
        max_subnet_len: usize,
    ) -> usize {
        subnets.iter().enumerate().fold(0, |acc, (i, subnet)| {
            let subnet_len = calc_subnet_len(subnet, rtt_result_cache, args, max_subnet_len);
            acc + subnet_len.min(rtt_result_cache.current_subnet_start)
                + if i < rtt_result_cache.current_subnet && subnet_len != 0 {
                    1
                } else {
                    0
                }
        })
    }

    let mut start_ip_count = calc_start_ip_count(subnets, &rtt_result_cache, &args, max_subnet_len);

    info!("current progress: {start_ip_count}/{all_ip_count}");

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

    while rtt_result_cache.current_subnet_start < max_subnet_len {
        let mut ips: Vec<IpInet> = Vec::new();
        let mut subnet_idxs: Vec<usize> = Vec::new();
        while ips.len() < config.max_connection_count {
            let subnet = &subnets[rtt_result_cache.current_subnet];
            if !args.auto_skip
                || rtt_result_cache.current_subnet_start < args.enable_threshold
                || subnet.enable
            {
                if let Some(ip_inet) = subnet.get_ip(rtt_result_cache.current_subnet_start) {
                    ips.push(ip_inet);
                    subnet_idxs.push(rtt_result_cache.current_subnet);
                }
            }

            rtt_result_cache.current_subnet += 1;
            if rtt_result_cache.current_subnet == subnets.len() {
                rtt_result_cache.current_subnet = 0;
                rtt_result_cache.current_subnet_start += 1;

                if args.auto_skip && rtt_result_cache.current_subnet_start == args.enable_threshold
                {
                    all_ip_count = subnets.iter().fold(0, |acc, subnet| {
                        acc + calc_subnet_len(subnet, &rtt_result_cache, &args, max_subnet_len)
                    });


                    // TODO:  可能会溢出，有空看看
                    // start_ip_count =
                    //     calc_start_ip_count(subnets, &rtt_result_cache, &args, max_subnet_len)
                    //         - ips.len();
                    start_ip_count =
                        if calc_start_ip_count(subnets, &rtt_result_cache, &args, max_subnet_len) >  ips.len() {
                            calc_start_ip_count(subnets, &rtt_result_cache, &args, max_subnet_len) - ips.len()
                } else {
                            0
                        };

                    progress_bar.println(format!("update: {start_ip_count}/{all_ip_count}"));
                    progress_bar.set_length(all_ip_count as u64);
                    progress_bar.set_position(start_ip_count as u64);
                    progress_bar.reset_eta();
                }

                if rtt_result_cache.current_subnet_start == max_subnet_len {
                    break;
                }
            }
        }

        let test_res = test_rtts(
            &config,
            &sing_box_template,
            &outbound_template,
            args.data_dir.as_str(),
            args.ignore_body_warning,
            &progress_bar,
            &ips,
        )
        .await?;
        let mut success_count = 0;
        for (i, ip) in ips.iter().enumerate() {
            if let Some(rtt) = &test_res[i] {
                success_count += 1;
                rtt_results.add_result(*ip, rtt.clone());
                if rtt_result_cache.current_subnet_start < args.enable_threshold {
                    subnets[subnet_idxs[i]].enable = true;
                }
            }
        }

        if success_count != 0 {
            rtt_results.commit();
            rtt_results.save(&rtt_result_file_name)?;
        }

        let log_str = format!(
            "Test success count: {success_count}/{} subnet: {}/{} current_subnet_start: {}/{}",
            ips.len(),
            rtt_result_cache.current_subnet,
            subnets.len(),
            rtt_result_cache.current_subnet_start,
            max_subnet_len
        );
        progress_bar.inc(ips.len() as u64);
        progress_bar.println(log_str.as_str());
        debug!("{log_str}");
        rtt_result_cache.save(&rtt_result_cache_file_name)?
    }

    progress_bar.finish_with_message("finish!");
    Ok(())
}
