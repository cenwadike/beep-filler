use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub rpc_http_endpoint: String,
    pub rpc_websocket_endpoint: String,
    pub contract_address: String,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_seconds: u64,
    #[serde(default = "default_subscription_timeout")]
    pub subscription_timeout_seconds: u64,
    #[serde(default = "default_event_retention_hours")]
    pub event_retention_hours: u64,
    #[serde(default = "default_timeout_height")]
    pub default_timeout_height: u64,
    #[serde(default = "default_chain_id")]
    pub chain_id: String,
    #[serde(default = "default_fee_denom")]
    pub fee_denom: String,
    #[serde(default = "default_gas_limit")]
    pub gas_limit: u64,
    #[serde(default)]
    pub mnemonic_file: Option<String>,
}

fn default_reconnect_delay() -> u64 {
    5
}

fn default_subscription_timeout() -> u64 {
    30
}

fn default_event_retention_hours() -> u64 {
    24
}

fn default_timeout_height() -> u64 {
    1200 // ~2 minutes at 6 seconds/block on Neutron
}

fn default_chain_id() -> String {
    "neutron-1".to_string()
}

fn default_fee_denom() -> String {
    "untrn".to_string()
}

fn default_gas_limit() -> u64 {
    200_000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rpc_http_endpoint: "https://rpc-palvus.pion-1.ntrn.tech".to_string(),
            rpc_websocket_endpoint: "wss://rpc-palvus.pion-1.ntrn.tech/websocket".to_string(),
            contract_address: "neutron13r9m3cn8zu6rnmkepajnm04zrry4g24exy9tunslseet0s9wrkkstcmkhr"
                .to_string(),
            reconnect_delay_seconds: default_reconnect_delay(),
            subscription_timeout_seconds: default_subscription_timeout(),
            event_retention_hours: default_event_retention_hours(),
            default_timeout_height: default_timeout_height(),
            chain_id: default_chain_id(),
            fee_denom: default_fee_denom(),
            gas_limit: default_gas_limit(),
            mnemonic_file: None,
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self> {
        let path = Path::new(path);
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.rpc_http_endpoint.is_empty() {
            return Err(anyhow::anyhow!("rpc_http_endpoint cannot be empty"));
        }
        if self.rpc_websocket_endpoint.is_empty() {
            return Err(anyhow::anyhow!("rpc_websocket_endpoint cannot be empty"));
        }
        if self.contract_address.is_empty() {
            return Err(anyhow::anyhow!("contract_address cannot be empty"));
        }
        if self.reconnect_delay_seconds == 0 {
            return Err(anyhow::anyhow!("reconnect_delay_seconds must be positive"));
        }
        if self.subscription_timeout_seconds == 0 {
            return Err(anyhow::anyhow!(
                "subscription_timeout_seconds must be positive"
            ));
        }
        if self.event_retention_hours == 0 {
            return Err(anyhow::anyhow!("event_retention_hours must be positive"));
        }
        if self.default_timeout_height == 0 {
            return Err(anyhow::anyhow!("default_timeout_height must be positive"));
        }
        if self.chain_id.is_empty() {
            return Err(anyhow::anyhow!("chain_id cannot be empty"));
        }
        if self.fee_denom.is_empty() {
            return Err(anyhow::anyhow!("fee_denom cannot be empty"));
        }
        if self.gas_limit == 0 {
            return Err(anyhow::anyhow!("gas_limit must be positive"));
        }
        if let Some(mnemonic_file) = &self.mnemonic_file {
            if !Path::new(mnemonic_file).exists() {
                return Err(anyhow::anyhow!(
                    "mnemonic_file does not exist: {}",
                    mnemonic_file
                ));
            }
        }
        Ok(())
    }

    pub fn load_mnemonic(&self) -> Result<Option<String>> {
        if let Some(mnemonic_file) = &self.mnemonic_file {
            let content = fs::read_to_string(mnemonic_file)
                .with_context(|| format!("Failed to read mnemonic file: {}", mnemonic_file))?;
            Ok(Some(content.trim().to_string()))
        } else {
            Ok(None)
        }
    }
}
