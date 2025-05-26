//! # Configuration Module
//!
//! This module defines the `Config` struct for configuring the Beep Intent Filler, a service that processes
//! blockchain-based intents (e.g., token swaps) on the Neutron blockchain within the Cosmos ecosystem.
//! The `Config` struct holds settings for blockchain endpoints, smart contract interactions, transaction
//! parameters, and external API keys, enabling flexible and secure operation of the intent execution system.
//!
//! ## Overview
//!
//! The `Config` struct:
//! - **Stores** critical settings like RPC endpoints, contract addresses, and mnemonic file paths.
//! - **Supports** JSON serialization/deserialization for loading configurations from files.
//! - **Provides** default values for optional fields (e.g., timeouts, chain ID).
//! - **Validates** configuration fields to ensure correctness before use.
//! - **Loads** mnemonic phrases from files for secure transaction signing.
//!
//! This module is used by the `IntentExecutor` (from `crate::executor`) to initialize the intent processing
//! service, ensuring robust integration with the Neutron blockchain and external services like CurrencyLayer.
//!
//! ## Key Features
//!
//! - **Serialization**: Uses `serde` for JSON-based configuration loading with default values.
//! - **Validation**: Enforces non-empty and positive values for critical fields.
//! - **File Handling**: Reads configuration and mnemonic files with detailed error reporting.
//! - **Error Handling**: Uses `anyhow::Result` with `Context` for meaningful error messages.
//! - **Flexibility**: Supports optional mnemonic files and customizable timeouts.
//! - **Cosmos Integration**: Configures Neutron-specific parameters (e.g., chain ID, fee denomination).
//!
//! ## Usage
//!
//! Create a `Config` instance by loading from a JSON file or using defaults, then validate and use it to
//! initialize the `IntentExecutor`. Below is an example:
//!
//! ```rust
//! use anyhow::Result;
//! use intent_system::config::Config;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Load config from file
//!     let config = Config::from_file("config.json")?; // {
//!     //     "rpc_http_endpoint": "https://rpc-palvus.pion-1.ntrn.tech",
//!     //     "rpc_websocket_endpoint": "wss://rpc-palvus.pion-1.ntrn.tech/websocket",
//!     //     "contract_address": "neutron13r9m3cn8zu6rnmkepajnm04zrry4g24exy9tunslseet0s9wrkkstcmkhr",
//!     //     "currency_layer_api_key": "your_api_key"
//!     // }
//!
//!     // Validate config
//!     config.validate()?;
//!
//!     // Load mnemonic (if configured)
//!     if let Some(mnemonic) = config.load_mnemonic()? {
//!         println!("Loaded mnemonic: {}", mnemonic);
//!     }
//!
//!     // Use config with IntentExecutor (example)
//!     // let executor = IntentExecutor::new(config, mnemonic, None).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Dependencies
//!
//! - `anyhow`: For robust error handling with contextual messages.
//! - `serde`: For JSON serialization and deserialization of the `Config` struct.
//! - `std::fs`: For reading configuration and mnemonic files.
//! - `std::path`: For handling file paths in a platform-independent way.
//!
//! ## Integration with Intent System
//!
//! The `Config` struct is a core component of the Beep Intent Filler, used to initialize the `IntentExecutor`
//! for processing intents (e.g., tNGN/tAtom swaps). It provides blockchain connection details, transaction
//! parameters, and API keys for price/forex providers, ensuring seamless operation within the Cosmos
//! ecosystem.
//!
//! ## Notes
//!
//! - **Security**: Mnemonic files must be stored securely to prevent key exposure. Avoid hardcoding
//!   mnemonics in configuration files.
//! - **File Paths**: The `mnemonic_file` path is optional; if unset, no mnemonic is loaded.
//! - **Validation**: Always call `validate()` after loading a config to ensure correctness.
//! - **Neutron-Specific**: Defaults are tailored for the Neutron chain (`neutron-1`, `untrn` fee).
//!   Adjust for other Cosmos chains.
//! - **API Keys**: The `currency_layer_api_key` is required for forex providers like `CurrencyLayerProvider`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{env, fs};

/// Configuration for the Beep Intent Filler service.
///
/// Holds settings for blockchain connections, smart contract interactions, transaction parameters,
/// and external API keys. Supports JSON serialization and validation for robust configuration management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// HTTP endpoint for the Neutron RPC node (e.g., "https://rpc-palvus.pion-1.ntrn.tech").
    pub rpc_http_endpoint: String,

    /// WebSocket endpoint for subscribing to blockchain events (e.g., "wss://rpc-palvus.pion-1.ntrn.tech/websocket").
    pub rpc_websocket_endpoint: String,

    /// Address of the CosmWasm smart contract for intent processing (e.g., "neutron13r...").
    pub contract_address: String,

    /// Delay between WebSocket reconnection attempts in seconds (default: 5).
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_seconds: u64,

    /// Timeout for WebSocket subscriptions in seconds (default: 30).
    #[serde(default = "default_subscription_timeout")]
    pub subscription_timeout_seconds: u64,

    /// Retention period for intent events in hours (default: 24).
    #[serde(default = "default_event_retention_hours")]
    pub event_retention_hours: u64,

    /// Default transaction timeout height in blocks (default: 1200, ~2 minutes at 6s/block).
    #[serde(default = "default_timeout_height")]
    pub default_timeout_height: u64,

    /// Chain ID of the blockchain (default: "neutron-1").
    #[serde(default = "default_chain_id")]
    pub chain_id: String,

    /// Denomination for transaction fees (default: "untrn").
    #[serde(default = "default_fee_denom")]
    pub fee_denom: String,

    /// Gas limit for transactions (default: 200,000).
    #[serde(default = "default_gas_limit")]
    pub gas_limit: u64,

    /// Optional path to a file containing the BIP-39 mnemonic for transaction signing.
    #[serde(default)]
    pub mnemonic_file: Option<String>,

    /// API key for the CurrencyLayer forex provider.
    pub currency_layer_api_key: String,
}

/// Returns the default reconnect delay in seconds.
///
/// # Returns
///
/// `5` seconds, used as the default for `reconnect_delay_seconds`.
fn default_reconnect_delay() -> u64 {
    5
}

/// Returns the default subscription timeout in seconds.
///
/// # Returns
///
/// `30` seconds, used as the default for `subscription_timeout_seconds`.
fn default_subscription_timeout() -> u64 {
    30
}

/// Returns the default event retention period in hours.
///
/// # Returns
///
/// `24` hours, used as the default for `event_retention_hours`.
fn default_event_retention_hours() -> u64 {
    24
}

/// Returns the default transaction timeout height in blocks.
///
/// # Returns
///
/// `1200` blocks, approximately 2 minutes at 6 seconds per block on Neutron, used as the default
/// for `default_timeout_height`.
fn default_timeout_height() -> u64 {
    1200 // ~2 minutes at 6 seconds/block on Neutron
}

/// Returns the default chain ID.
///
/// # Returns
///
/// `"neutron-1"`, the chain ID for the Neutron blockchain.
fn default_chain_id() -> String {
    "neutron-1".to_string()
}

/// Returns the default fee denomination.
///
/// # Returns
///
/// `"untrn"`, the native token denomination for Neutron transaction fees.
fn default_fee_denom() -> String {
    "untrn".to_string()
}

/// Returns the default gas limit for transactions.
///
/// # Returns
///
/// `200,000`, a reasonable default for most CosmWasm contract interactions.
fn default_gas_limit() -> u64 {
    200_000
}

impl Default for Config {
    /// Creates a default `Config` instance with predefined values.
    ///
    /// Initializes fields with sensible defaults tailored for the Neutron blockchain and the Beep Intent
    /// Filler. The `mnemonic_file` is `None`, and `currency_layer_api_key` is empty, requiring explicit
    /// configuration.
    ///
    /// # Returns
    ///
    /// A `Config` instance with default values.
    fn default() -> Self {
        let mut currency_layer_api_key = env::var("CURRENCY_LAYER_API_KEY").ok();
        let currency_layer_api_key = currency_layer_api_key.get_or_insert_default();
        Self {
            // Default Neutron testnet RPC endpoints
            rpc_http_endpoint: "https://rpc-palvus.pion-1.ntrn.tech".to_string(),
            rpc_websocket_endpoint: "wss://rpc-palvus.pion-1.ntrn.tech/websocket".to_string(),
            // Sample contract address for Beep intent processing
            contract_address: "neutron13r9m3cn8zu6rnmkepajnm04zrry4g24exy9tunslseet0s9wrkkstcmkhr"
                .to_string(),
            // Default timing and retention settings
            reconnect_delay_seconds: default_reconnect_delay(),
            subscription_timeout_seconds: default_subscription_timeout(),
            event_retention_hours: default_event_retention_hours(),
            default_timeout_height: default_timeout_height(),
            // Neutron-specific blockchain parameters
            chain_id: default_chain_id(),
            fee_denom: default_fee_denom(),
            gas_limit: default_gas_limit(),
            // Optional mnemonic file (unset by default)
            mnemonic_file: None,
            // Empty API key (must be set for CurrencyLayer)
            currency_layer_api_key: currency_layer_api_key.to_string(),
        }
    }
}

impl Config {
    /// Loads a `Config` instance from a JSON file.
    ///
    /// Reads and parses a JSON file at the specified path, deserializes it into a `Config` struct,
    /// and validates the configuration. The file should contain fields matching the `Config` struct,
    /// with optional fields using defaults if omitted.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path to the JSON configuration file.
    ///
    /// # Returns
    ///
    /// A `Result` containing the parsed and validated `Config` or an error if the file cannot be read,
    /// parsed, or validated.
    ///
    /// # Example
    ///
    /// ```rust
    /// use intent_system::config::Config;
    /// let config = Config::from_file("config.json")?;
    /// ```
    pub fn from_file(path: &str) -> Result<Self> {
        // Resolve the file path
        let path = Path::new(path);
        // Read the file contents
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        // Parse JSON into Config struct
        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        // Validate the configuration
        config.validate()?;
        Ok(config)
    }

    /// Validates the configuration fields.
    ///
    /// Ensures that required fields are non-empty, numeric fields are positive, and the mnemonic file
    /// (if specified) exists. This method should be called after loading or modifying a `Config` instance.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success (`Ok(())`) or an error if any validation check fails.
    ///
    /// # Example
    ///
    /// ```rust
    /// let config = Config::default();
    /// config.validate()?;
    /// ```
    pub fn validate(&self) -> Result<()> {
        // Check for non-empty string fields
        if self.rpc_http_endpoint.is_empty() {
            return Err(anyhow::anyhow!("rpc_http_endpoint must not be empty"));
        }
        if self.rpc_websocket_endpoint.is_empty() {
            return Err(anyhow::anyhow!("rpc_websocket_endpoint must not be empty"));
        }
        if self.contract_address.is_empty() {
            return Err(anyhow::anyhow!("contract_address must not be empty"));
        }
        // Check for positive numeric fields
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
        // Check for non-empty blockchain parameters
        if self.chain_id.is_empty() {
            return Err(anyhow::anyhow!("chain_id must not be empty"));
        }
        if self.fee_denom.is_empty() {
            return Err(anyhow::anyhow!("fee_denom must not be empty"));
        }
        if self.gas_limit == 0 {
            return Err(anyhow::anyhow!("gas_limit must be positive"));
        }
        // Validate mnemonic file existence if specified
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

    /// Loads a mnemonic phrase from the configured file, if specified.
    ///
    /// Reads the mnemonic from the file path in `mnemonic_file`, trims whitespace, and returns it as
    /// a `String`. If no mnemonic file is configured, returns `None`.
    ///
    /// # Returns
    ///
    /// A `Result` containing an `Option<String>` with the mnemonic (if present) or `None` if no file
    /// is configured, or an error if the file cannot be read.
    ///
    /// # Example
    ///
    /// ```rust
    /// let config = Config {
    ///     mnemonic_file: Some("mnemonic.txt".to_string()),
    ///     ..Default::default()
    /// };
    /// if let Some(mnemonic) = config.load_mnemonic()? {
    ///     println!("Mnemonic: {}", mnemonic);
    /// }
    /// ```
    pub fn load_mnemonic(&self) -> Result<Option<String>> {
        if let Some(mnemonic_file) = &self.mnemonic_file {
            // Read the mnemonic file contents
            let content = fs::read_to_string(mnemonic_file)
                .with_context(|| format!("Failed to read mnemonic file: {}", mnemonic_file))?;
            // Trim whitespace and return the mnemonic
            Ok(Some(content.trim().to_string()))
        } else {
            // No mnemonic file configured
            Ok(None)
        }
    }
}
