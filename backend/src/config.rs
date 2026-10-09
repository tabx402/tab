use crate::{
    bnb::{address, CHAIN_ID, USDT},
    error::{ApiError, Result},
};
use std::{env, path::PathBuf};
#[derive(Clone)]
pub struct Config {
    pub network: String,
    pub chain_id: u64,
    pub rpc: String,
    pub logs_rpc: Option<String>,
    pub usdt: String,
    pub program: String,
    pub official_tab: Option<String>,
    pub app_id: Option<String>,
    pub database: PathBuf,
    pub manifest: PathBuf,
    pub merchants: PathBuf,
    pub x402_merchants: PathBuf,
    pub bind: String,
    pub openrouter_key: Option<String>,
    pub tavily_key: Option<String>,
    pub inference_daily_micros: u64,
    pub confirmations: u64,
    pub sponsor_key: Option<String>,
    pub sponsor_address: Option<String>,
    pub sponsor_enabled: bool,
}
fn optional(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.trim().is_empty())
}
impl Config {
    pub fn environment() -> Result<Self> {
        let root = optional("TAB_PROJECT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/ubuntu/apps/tabagents"));
        let chain_id = optional("TAB_BNB_CHAIN_ID")
            .map(|v| v.parse::<u64>())
            .transpose()
            .map_err(|_| ApiError::validation("Invalid BNB chain identifier."))?
            .unwrap_or(CHAIN_ID);
        if chain_id != CHAIN_ID {
            return Err(ApiError::validation(
                "Tab uses BNB Smart Chain mainnet, chain 56.",
            ));
        }
        let program = optional("TAB_BNB_PROTOCOL")
            .map(|v| address(&v))
            .transpose()?
            .unwrap_or_default();
        let usdt = optional("TAB_USDT_ADDRESS")
            .map(|v| address(&v))
            .transpose()?
            .unwrap_or_else(|| USDT.into());
        if usdt != USDT {
            return Err(ApiError::validation(
                "Unexpected BNB mainnet USDT contract.",
            ));
        }
        let database = optional("TAB_DATABASE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("backend/data/tab-bnb56.sqlite"));
        if database.to_string_lossy().contains("solana") {
            return Err(ApiError::validation(
                "Use a separate BNB database. Existing Solana records must be preserved.",
            ));
        }
        Ok(Self {
            network: "mainnet".into(),
            chain_id,
            rpc: optional("TAB_BNB_RPC")
                .unwrap_or_else(|| "https://bsc-dataseed.bnbchain.org".into()),
            logs_rpc: Some(
                optional("TAB_BNB_LOGS_RPC").unwrap_or_else(|| "https://rpc-bsc.48.club".into()),
            ),
            usdt,
            program,
            official_tab: optional("TAB_OFFICIAL_TOKEN")
                .map(|v| address(&v))
                .transpose()?,
            app_id: optional("PRIVY_APP_ID"),
            database,
            manifest: optional("TAB_BNB_MANIFEST")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("contracts/deployments/bnb-56.json")),
            merchants: optional("TAB_JOB_MERCHANTS")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("backend/config/job-merchants-bnb.json")),
            x402_merchants: optional("TAB_X402_MERCHANTS")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("backend/config/x402-merchants-bnb.json")),
            bind: format!(
                "{}:{}",
                optional("TAB_HOST").unwrap_or_else(|| "127.0.0.1".into()),
                optional("TAB_PORT").unwrap_or_else(|| "4297".into())
            ),
            openrouter_key: optional("OPENROUTER_API_KEY"),
            tavily_key: optional("TAVILY_API_KEY"),
            inference_daily_micros: optional("TAB_INFERENCE_DAILY_MICROS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(100000)
                .min(1_000_000),
            confirmations: optional("TAB_BNB_CONFIRMATIONS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3)
                .max(3),
            sponsor_key: optional("TAB_BNB_SPONSOR_PRIVATE_KEY"),
            sponsor_address: optional("TAB_BNB_SPONSOR_ADDRESS")
                .map(|value| address(&value))
                .transpose()?,
            sponsor_enabled: optional("TAB_BNB_SPONSOR_ENABLED")
                .map(|value| value.parse::<bool>())
                .transpose()
                .map_err(|_| {
                    ApiError::validation("TAB_BNB_SPONSOR_ENABLED must be true or false.")
                })?
                .unwrap_or(false),
        })
    }
}
