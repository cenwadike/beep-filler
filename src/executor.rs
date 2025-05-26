//! # Intent Executor Module
//!
//! This module defines the `IntentExecutor` struct, which manages the execution of blockchain-based
//! intents (e.g., token swaps) on a Cosmos-based network (e.g., Neutron). It integrates with price and
//! forex providers to evaluate intent profitability, queries intent details, and executes transactions
//! using the CosmWasm and Tendermint ecosystems. The executor handles intent events, performs profit
//! analysis, and interacts with smart contracts to fulfill intents.
//!
//! ## Overview
//!
//! The `IntentExecutor`:
//! - **Initializes** with a configuration, mnemonic for signing, and minimum profit percentage.
//! - **Processes** `IntentCreatedEvent`s by validating intents, analyzing profitability, and executing
//!   profitable swaps.
//! - **Fetches** token prices and forex rates using providers (e.g., `NGNPriceProvider`,
//!   `ExchangeRateApiProvider`).
//! - **Caches** prices and rates to optimize performance.
//! - **Executes** transactions on the blockchain, including increasing token allowances and filling
//!   intents.
//! - **Manages** account information and transaction signing using BIP-39 mnemonics and secp256k1 keys.
//!
//! ## Key Features
//!
//! - **Asynchronous Operation**: Uses `async`/`await` for non-blocking HTTP requests and blockchain
//!   interactions.
//! - **Profit Analysis**: Evaluates intent profitability using `ProfitAnalysis` to ensure minimum
//!   profit margins.
//! - **Caching**: Implements price and forex rate caching with configurable durations to reduce API calls.
//! - **Error Handling**: Uses `anyhow::Result` for robust error propagation with context.
//! - **Extensibility**: Supports adding/removing price and forex providers dynamically.
//! - **Logging**: Includes detailed logging (`log` crate) for debugging and monitoring.
//! - **Blockchain Integration**: Leverages `cosmrs` for transaction signing and broadcasting, and
//!   `tendermint_rpc` for blockchain communication.
//! - **Fault Tolerance**: Includes retry logic for external APIs and blockchain interactions, with
//!   timeouts and input validation.
//!
//! ## Usage
//!
//! To use the `IntentExecutor`, initialize it with a configuration and mnemonic, then call
//! `handle_intent_event` to process intent events. Below is an example:
//!
//! ```rust
//! use anyhow::Result;
//! use intent_system::{config::Config, executor::IntentExecutor, types::IntentCreatedEvent};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize configuration
//!     let config = Config {
//!         rpc_http_endpoint: "http://localhost:1317".to_string(),
//!         contract_address: "neutron1...".to_string(),
//!         ..Default::default()
//!     };
//!
//!     // Create executor with mnemonic
//!     let mnemonic = "your 24-word mnemonic here";
//!     let executor = IntentExecutor::new(config, mnemonic.to_string(), Some(0.005)).await?;
//!
//!     // Process an intent event
//!     let event = IntentCreatedEvent {
//!         intent_id: "intent_1".to_string(),
//!         status: "Active".to_string(),
//!         sender: "neutron1...".to_string(),
//!         block_height: 12345,
//!     };
//!     executor.handle_intent_event(event).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Dependencies
//!
//! - `anyhow`: For flexible error handling with context.
//! - `bip32`, `bip39`: For mnemonic-based key derivation.
//! - `cosmrs`: For Cosmos transaction construction and signing.
//! - `cosmwasm_std`: For blockchain-specific types (`Uint128`).
//! - `reqwest`: For HTTP requests to APIs and blockchain nodes.
//! - `rust_decimal`: For precise decimal arithmetic in profit calculations.
//! - `serde_json`: For JSON serialization/deserialization.
//! - `tendermint_rpc`: For interacting with Tendermint nodes.
//! - `tokio-retry`: For retrying failed API and blockchain operations.
//! - `log`: For structured logging.
//! - `std`: For collections, async utilities, and synchronization primitives.
//!
//! ## Integration with Intent System
//!
//! The `IntentExecutor` works with:
//! - **Types Module**: Uses `IntentCreatedEvent`, `IntentResponse`, `BeepCoin`, `Token`, etc., for intent
//!   processing.
//! - **Price Providers**: Fetches token prices via `PriceProvider` implementations (e.g., `NGNPriceProvider`).
//! - **Forex Providers**: Fetches exchange rates via `ForexProvider` implementations (e.g.,
//!   `ExchangeRateApiProvider`).
//! - **Event System**: Processes events from the `intent_event` module, integrating with the
//!   `IntentEventSourcer` and `IntentEventConsumer`.
//!
//! ## Notes
//!
//! - **Security**: Mnemonic handling must be secure to prevent key exposure.
//! - **API Reliability**: External APIs (e.g., CoinGecko, CurrencyLayer) may have rate limits or downtime;
//!   caching and retry logic mitigate this.
//! - **Blockchain Assumptions**: Configured for the Neutron chain (`neutron-1`); adjust for other chains.
//! - **Performance**: Caching reduces API calls but requires monitoring for stale data.
//! - **Fault Tolerance**: Retries are implemented for API and blockchain calls with exponential backoff
//!   (max 3 attempts, initial delay 1s, max delay 5s). Timeouts are set for HTTP requests (10s) to prevent
//!   hanging. Input validation ensures robustness against invalid data.

use anyhow::{Context, Result};
use bip32::{ChildNumber, Seed, XPrv};
use bip39::{Language, Mnemonic};
use cosmrs::{
    AccountId, Coin,
    crypto::secp256k1::SigningKey,
    tx::{self, Fee, Msg, SignDoc, SignerInfo},
};
use cosmwasm_std::Uint128;
use log::{debug, error, info, warn};
use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde_json::{Value, json};
use std::{collections::HashMap, env};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tendermint_rpc::{Client as TmClient, HttpClient, Url};
use tokio_retry::{Retry, strategy::ExponentialBackoff};

use crate::forex_providers::{
    CurrencyLayerProvider, CurrencyRate, ExchangeRateApiProvider, ForexProvider,
};
use crate::price_providers::{AtomCoinGeckoProvider, NGNPriceProvider, PriceProvider};
use crate::types::{
    BeepCoin, ExecuteMsg, ExecuteMsgEnum, IntentCreatedEvent, IntentResponse, IntentStatus,
    QueryMsg, QueryMsgEnum, Token, TxResult,
};
use crate::{config::Config, types::ProfitAnalysis};

/// Manages execution of blockchain-based intents, such as token swaps.
///
/// Integrates with price and forex providers to evaluate profitability, queries intent details, and
/// executes transactions on a Cosmos-based blockchain (e.g., Neutron). Handles intent events, performs
/// profit analysis, and interacts with smart contracts.
#[derive(Clone)]
pub struct IntentExecutor {
    /// Configuration for blockchain endpoints and contract details.
    config: Config,
    /// HTTP client for API and blockchain queries with timeout.
    http_client: Client,
    /// Tendermint HTTP client for transaction broadcasting.
    tm_client: HttpClient,
    /// Signing key for transaction authorization, wrapped in `Arc` for thread safety.
    signing_key: Arc<SigningKey>,
    /// Account ID derived from the signing key.
    account_id: AccountId,
    /// Minimum profit percentage required for intent execution.
    min_profit_percentage: Decimal,
    /// Cache for token prices, with timestamps for expiration.
    price_cache: Arc<tokio::sync::RwLock<HashMap<String, (Decimal, std::time::Instant)>>>,
    /// Cache for forex rates, with rate details.
    forex_cache: Arc<tokio::sync::RwLock<HashMap<String, CurrencyRate>>>,
    /// Duration (seconds) for forex cache validity.
    forex_cache_duration_secs: u64,
    /// List of forex providers for exchange rates.
    forex_providers: Vec<Arc<Box<dyn ForexProvider>>>,
    /// List of price providers for token prices.
    price_providers: Vec<Arc<Box<dyn PriceProvider>>>,
}

/// Stores account information for transaction signing.
#[derive(Debug)]
struct AccountInfo {
    /// Account number on the blockchain.
    account_number: u64,
    /// Sequence number for transaction ordering.
    sequence: u64,
}

impl IntentExecutor {
    /// Creates a new `IntentExecutor` instance.
    ///
    /// Initializes the executor with a configuration, mnemonic for signing, and optional minimum profit
    /// percentage. Sets up HTTP and Tendermint clients, derives the signing key, and initializes price
    /// and forex providers.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for blockchain and contract settings.
    /// * `mnemonic` - BIP-39 mnemonic phrase for key derivation.
    /// * `min_profit_percentage` - Optional minimum profit percentage (default: 0.3%).
    ///
    /// # Returns
    ///
    /// A `Result` containing the initialized `IntentExecutor` or an error if setup fails.
    pub async fn new(
        config: Config,
        mnemonic: String,
        min_profit_percentage: Option<f64>,
    ) -> Result<Self> {
        // Configure HTTP client with timeout
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let url = Url::from_str(&config.rpc_http_endpoint).context("Invalid RPC endpoint URL")?;
        let tm_client = HttpClient::new(url).context("Failed to create Tendermint client")?;

        let signing_key = Self::signing_key_from_mnemonic(&mnemonic)?;
        let public_key = signing_key.public_key();
        let account_id = public_key
            .account_id("neutron")
            .map_err(|e| anyhow::anyhow!("Failed to get account ID: {}", e))?;

        // Safely convert min_profit_percentage with default
        let min_profit_percentage = min_profit_percentage
            .map(|p| {
                Decimal::from_f64(p)
                    .ok_or_else(|| anyhow::anyhow!("Invalid profit percentage: {}", p))
            })
            .transpose()?
            .unwrap_or_else(|| {
                Decimal::from_str("0.003").expect("Static decimal parsing should not fail")
            });

        let mut currency_layer_api_key = env::var("CURRENCY_LAYER_API_KEY").ok();
        let currency_layer_api_key = currency_layer_api_key.get_or_insert_default();

        let price_providers: Vec<Arc<Box<dyn PriceProvider>>> = vec![
            Arc::new(Box::new(NGNPriceProvider)),
            Arc::new(Box::new(AtomCoinGeckoProvider)),
        ];

        let forex_providers: Vec<Arc<Box<dyn ForexProvider>>> = vec![
            Arc::new(Box::new(ExchangeRateApiProvider)),
            Arc::new(Box::new(CurrencyLayerProvider::new(currency_layer_api_key.to_string()))),
        ];

        let forex_cache_duration_secs = 8 * 60 * 60; // Default 8 hours

        info!(
            "Initialized smart intent executor for account: {}",
            account_id
        );
        info!(
            "Minimum profit percentage: {}%",
            min_profit_percentage * Decimal::from(100)
        );
        info!(
            "Forex cache duration: {} hours",
            forex_cache_duration_secs / 3600
        );
        info!("Loaded {} price providers:", price_providers.len());
        for provider in &price_providers {
            info!("  - {}", provider.provider_name());
        }
        info!("Loaded {} forex providers:", forex_providers.len());
        for provider in &forex_providers {
            info!(
                "  - {} (supports {} currencies)",
                provider.provider_name(),
                provider.get_supported_currencies().len()
            );
        }

        Ok(Self {
            config,
            http_client,
            tm_client,
            signing_key: Arc::new(signing_key),
            account_id,
            min_profit_percentage,
            price_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            forex_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            forex_cache_duration_secs,
            forex_providers,
            price_providers,
        })
    }

    /// Derives a signing key from a BIP-39 mnemonic phrase.
    ///
    /// Uses BIP-32 derivation to generate a secp256k1 private key for transaction signing.
    ///
    /// # Arguments
    ///
    /// * `mnemonic` - The BIP-39 mnemonic phrase.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `SigningKey` or an error if derivation fails.
    fn signing_key_from_mnemonic(mnemonic: &str) -> Result<SigningKey> {
        let mnemonic =
            Mnemonic::parse_in(Language::English, mnemonic).context("Invalid mnemonic phrase")?;
        let seed = Seed::new(mnemonic.to_seed(""));

        let master_key = XPrv::new(seed.as_bytes()).context("Failed to derive master key")?;
        let child_number = ChildNumber::default();
        let derived_key = master_key
            .derive_child(child_number)
            .context("Failed to derive child key")?;
        let private_key_bytes = derived_key.private_key().to_bytes();
        let signing_key = SigningKey::from_slice(&private_key_bytes).map_err(|_| {
            anyhow::anyhow!("Failed to create signing key from derived private key")
        })?;

        Ok(signing_key)
    }

    /// Handles an intent creation event.
    ///
    /// Validates the intent, analyzes profitability for swap intents, increases allowances for non-native
    /// tokens, and executes the intent if profitable. Skips processing on validation or profitability
    /// failures, logging the reason.
    ///
    /// # Arguments
    ///
    /// * `event` - The `IntentCreatedEvent` to process.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error if execution fails.
    pub async fn handle_intent_event(&self, event: IntentCreatedEvent) -> Result<()> {
        info!("New Intent Created:");
        info!("Intent ID: {}", event.intent_id);
        info!("Status: {}", event.status);
        info!("Creator: {}", event.sender);
        info!("Block Height: {}", event.block_height);

        // Validate intent ID
        if event.intent_id.is_empty() {
            warn!("Skipping Intent. Reason: Empty intent ID");
            return Ok(());
        }

        info!("Validating Intent...");
        let intent_response = match self.get_intent(&event.intent_id).await {
            Ok(response) => response,
            Err(e) => {
                warn!("Skipping Intent. Reason: Failed to fetch intent - {}", e);
                return Ok(());
            }
        };

        if intent_response.intent.status != IntentStatus::Active {
            warn!("Skipping Intent. Reason: Intent not active");
            return Ok(());
        }

        match &intent_response.intent.intent_type {
            crate::types::IntentType::Swap(swap_intent) => {
                info!("Analyzing swap intent profitability...");

                let profit_analysis = match self
                    .analyze_swap_profitability(
                        &intent_response.intent.input_tokens,
                        &swap_intent.output_tokens,
                        &intent_response.intent.tip,
                    )
                    .await
                {
                    Ok(analysis) => analysis,
                    Err(e) => {
                        error!(
                            "Failed to analyze profitability for intent {}: {}",
                            event.intent_id, e
                        );
                        warn!(
                            "Skipping Intent. Reason: Unable to determine token prices - {}",
                            e
                        );
                        return Ok(());
                    }
                };

                info!("Profit Analysis Results:");
                info!("Input Value: ${:.4}", profit_analysis.input_value_usd);
                info!("Output Value: ${:.4}", profit_analysis.output_value_usd);
                info!("Tip Value: ${:.4}", profit_analysis.tip_value_usd);
                info!("Total Revenue: ${:.4}", profit_analysis.total_revenue_usd);
                info!(
                    "Expected Profit: ${:.4} ({:.2}%)",
                    profit_analysis.profit_usd,
                    profit_analysis.profit_percentage * Decimal::from(100)
                );
                info!(
                    "Required Minimum: {:.2}%",
                    self.min_profit_percentage * Decimal::from(100)
                );

                if !profit_analysis.is_profitable {
                    warn!(
                        "Skipping Intent. Reason: Insufficient profit margin. Expected: {:.2}%, Required: {:.2}%",
                        profit_analysis.profit_percentage * Decimal::from(100),
                        self.min_profit_percentage * Decimal::from(100)
                    );
                    return Ok(());
                }

                info!("Intent is profitable! Proceeding with execution...");

                for token in &swap_intent.output_tokens {
                    if !token.is_native {
                        if let Err(e) = self
                            .increase_allowance(
                                &token.token,
                                &self.config.contract_address,
                                &token.amount,
                            )
                            .await
                        {
                            error!(
                                "Failed to increase allowance for token {}: {}",
                                token.token, e
                            );
                            warn!("Skipping Intent. Reason: Allowance increase failed");
                            return Ok(());
                        }
                    }
                }

                info!("Filling intent");
                let tx_result = match self
                    .fill_intent(&event.intent_id, &swap_intent.output_tokens)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        error!("Failed to fill intent {}: {}", event.intent_id, e);
                        warn!("Skipping Intent. Reason: Intent execution failed");
                        return Ok(());
                    }
                };

                info!(
                    "Filled intent successfully. Tx Hash: {}",
                    tx_result.transaction_hash
                );
                info!(
                    "Expected profit realized: ${:.4}",
                    profit_analysis.profit_usd
                );
                info!("------------------------");
            }
        }

        Ok(())
    }

    /// Analyzes the profitability of a swap intent.
    ///
    /// Calculates the USD value of input tokens, output tokens, and tip, then determines profit and
    /// whether it meets the minimum profit percentage.
    ///
    /// # Arguments
    ///
    /// * `input_tokens` - Input tokens provided by the intent creator.
    /// * `output_tokens` - Output tokens required to fulfill the intent.
    /// * `tip` - Tip provided to incentivize execution.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `ProfitAnalysis` or an error if price fetching fails.
    async fn analyze_swap_profitability(
        &self,
        input_tokens: &[BeepCoin],
        output_tokens: &[Token],
        tip: &BeepCoin,
    ) -> Result<ProfitAnalysis> {
        let mut input_value_usd = Decimal::ZERO;
        for input_token in input_tokens {
            let price = self
                .get_token_price_usd(&input_token.token)
                .await
                .context(format!(
                    "Failed to get price for input token: {}",
                    input_token.token
                ))?;
            let token_amount = self.uint128_to_decimal(input_token.amount)?;
            input_value_usd += price * token_amount;
        }

        let mut output_value_usd = Decimal::ZERO;
        for output_token in output_tokens {
            let price = self
                .get_token_price_usd(&output_token.token)
                .await
                .context(format!(
                    "Failed to get price for output token: {}",
                    output_token.token
                ))?;
            let token_amount = self.uint128_to_decimal(output_token.amount)?;
            output_value_usd += price * token_amount;
        }

        let tip_price = self
            .get_token_price_usd(&tip.token)
            .await
            .context(format!("Failed to get price for tip token: {}", tip.token))?;
        let tip_amount = self.uint128_to_decimal(tip.amount)?;
        let tip_value_usd = tip_price * tip_amount;

        let total_revenue_usd = input_value_usd + tip_value_usd;

        let profit_usd = total_revenue_usd - output_value_usd;
        let profit_percentage = if output_value_usd > Decimal::ZERO {
            profit_usd / output_value_usd
        } else {
            Decimal::ZERO
        };

        let is_profitable = profit_percentage >= self.min_profit_percentage;

        Ok(ProfitAnalysis {
            input_value_usd,
            output_value_usd,
            tip_value_usd,
            total_revenue_usd,
            profit_usd,
            profit_percentage,
            is_profitable,
        })
    }

    /// Fetches the USD price of a token, using cache if available.
    ///
    /// Checks the price cache (valid for 5 minutes) before fetching from a price provider.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier (e.g., "uatom", "ngn").
    ///
    /// # Returns
    ///
    /// A `Result` containing the token price in USD or an error if fetching fails.
    async fn get_token_price_usd(&self, token: &str) -> Result<Decimal> {
        // Validate token identifier
        if token.is_empty() {
            return Err(anyhow::anyhow!("Invalid token identifier: empty"));
        }

        {
            let cache = self.price_cache.read().await;
            if let Some((price, timestamp)) = cache.get(token) {
                if timestamp.elapsed().as_secs() < 300 {
                    debug!("Using cached price for {}: ${:.6}", token, price);
                    return Ok(*price);
                }
            }
        }

        let price = self.fetch_token_price_from_api(token).await?;

        {
            let mut cache = self.price_cache.write().await;
            cache.insert(token.to_string(), (price, std::time::Instant::now()));
        }

        debug!("Fetched fresh price for {}: ${:.6}", token, price);
        Ok(price)
    }

    /// Fetches a token price from a price provider API with retry logic.
    ///
    /// Retries up to 3 times with exponential backoff (initial delay 1s, max 5s).
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier.
    ///
    /// # Returns
    ///
    /// A `Result` containing the token price in USD or an error if no provider supports the token.
    async fn fetch_token_price_from_api(&self, token: &str) -> Result<Decimal> {
        let price_provider = self.get_price_provider_for_token(token)?;
        let retry_strategy = ExponentialBackoff::from_millis(1000)
            .max_delay(Duration::from_secs(5))
            .take(3);

        Retry::spawn(retry_strategy, || async {
            price_provider
                .fetch_price(token, &self.http_client, Some(self))
                .await
                .map_err(|e| {
                    warn!("Retrying price fetch for {}: {}", token, e);
                    e
                })
        })
        .await
        .context(format!("Failed to fetch price for token: {}", token))
    }

    /// Finds a price provider that supports the given token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier.
    ///
    /// # Returns
    ///
    /// A `Result` containing a reference to the `PriceProvider` or an error if no provider is found.
    fn get_price_provider_for_token(&self, token: &str) -> Result<&dyn PriceProvider> {
        for provider in self.price_providers.iter() {
            if provider.supports_token(token) {
                debug!(
                    "Using {} provider for token: {}",
                    provider.provider_name(),
                    token
                );
                return Ok(provider.as_ref().as_ref());
            }
        }

        Err(anyhow::anyhow!(
            "No price provider found for token: {}. Supported tokens: {}",
            token,
            self.get_supported_tokens().join(", ")
        ))
    }

    /// Returns a list of supported token identifiers.
    ///
    /// # Returns
    ///
    /// A sorted, deduplicated vector of supported token identifiers.
    pub fn get_supported_tokens(&self) -> Vec<String> {
        let mut supported = Vec::new();
        let test_tokens = vec!["ungn", "NGN", "ngn", "uatom", "ATOM", "atom"];

        for token in test_tokens {
            for provider in self.price_providers.iter() {
                if provider.supports_token(token) {
                    supported.push(token.to_string());
                    break;
                }
            }
        }

        supported.sort();
        supported.dedup();
        supported
    }

    /// Adds a new price provider to the executor.
    ///
    /// # Arguments
    ///
    /// * `provider` - The `PriceProvider` to add.
    pub fn add_price_provider(&mut self, provider: Box<dyn PriceProvider>) {
        info!("Adding new price provider: {}", provider.provider_name());
        self.price_providers.push(Arc::new(provider));
    }

    /// Removes a price provider by name.
    ///
    /// # Arguments
    ///
    /// * `provider_name` - The name of the provider to remove.
    ///
    /// # Returns
    ///
    /// `true` if a provider was removed, `false` otherwise.
    pub fn remove_price_provider(&mut self, provider_name: &str) -> bool {
        let initial_len = self.price_providers.len();
        self.price_providers
            .retain(|p| p.provider_name() != provider_name);
        let removed = self.price_providers.len() < initial_len;

        if removed {
            info!("Removed price provider: {}", provider_name);
        } else {
            warn!("Price provider not found: {}", provider_name);
        }

        removed
    }

    /// Lists the names of all price providers.
    ///
    /// # Returns
    ///
    /// A vector of provider names.
    pub fn list_price_providers(&self) -> Vec<&str> {
        self.price_providers
            .iter()
            .map(|p| p.provider_name())
            .collect()
    }

    /// Fetches an exchange rate for a currency pair, using cache if available.
    ///
    /// Checks the forex cache (valid for `forex_cache_duration_secs`) before fetching from a provider.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency (e.g., "USD").
    /// * `target_currency` - The target currency (e.g., "NGN").
    ///
    /// # Returns
    ///
    /// A `Result` containing the exchange rate as a `Decimal` or an error if fetching fails.
    pub async fn get_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
    ) -> Result<Decimal> {
        // Validate currency codes
        if base_currency.is_empty() || target_currency.is_empty() {
            return Err(anyhow::anyhow!(
                "Invalid currency pair: empty base or target"
            ));
        }

        let cache_key = format!(
            "{}_{}",
            base_currency.to_uppercase(),
            target_currency.to_uppercase()
        );

        {
            let cache = self.forex_cache.read().await;
            if let Some(currency_rate) = cache.get(&cache_key) {
                if !currency_rate.is_expired(self.forex_cache_duration_secs) {
                    debug!(
                        "Using cached {}/{} rate: {:.4} (cached {} minutes ago)",
                        base_currency,
                        target_currency,
                        currency_rate.rate,
                        currency_rate.age_minutes()
                    );
                    return Ok(currency_rate.rate);
                }
            }
        }

        debug!(
            "Fetching fresh {}/{} rate from API",
            base_currency, target_currency
        );
        let rate = self
            .fetch_exchange_rate_from_providers(base_currency, target_currency)
            .await?;

        {
            let mut cache = self.forex_cache.write().await;
            cache.insert(
                cache_key,
                CurrencyRate::new(rate, base_currency.to_string(), target_currency.to_string()),
            );
        }

        info!(
            "Updated {}/{} rate: {:.4} (cached for {} hours)",
            base_currency,
            target_currency,
            rate,
            self.forex_cache_duration_secs / 3600
        );
        Ok(rate)
    }

    /// Fetches an exchange rate from forex providers with retry logic.
    ///
    /// Tries each provider that supports the currency pair until one succeeds, with up to 3 retries
    /// per provider (exponential backoff, initial delay 1s, max 5s).
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency.
    /// * `target_currency` - The target currency.
    ///
    /// # Returns
    ///
    /// A `Result` containing the exchange rate or an error if all providers fail.
    async fn fetch_exchange_rate_from_providers(
        &self,
        base_currency: &str,
        target_currency: &str,
    ) -> Result<Decimal> {
        let retry_strategy = ExponentialBackoff::from_millis(1000)
            .max_delay(Duration::from_secs(5))
            .take(3);

        let mut last_error = None;

        for provider in self.forex_providers.iter() {
            if provider.supports_currency_pair(base_currency, target_currency) {
                let result = Retry::spawn(retry_strategy.clone(), || async {
                    provider
                        .fetch_exchange_rate(base_currency, target_currency, &self.http_client)
                        .await
                        .map_err(|e| {
                            warn!(
                                "Retrying {}/{} rate fetch from {}: {}",
                                base_currency,
                                target_currency,
                                provider.provider_name(),
                                e
                            );
                            e
                        })
                })
                .await;

                match result {
                    Ok(rate) => {
                        debug!(
                            "Successfully fetched {}/{} rate from {}: {:.4}",
                            base_currency,
                            target_currency,
                            provider.provider_name(),
                            rate
                        );
                        return Ok(rate);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to fetch {}/{} rate from {} after retries: {}",
                            base_currency,
                            target_currency,
                            provider.provider_name(),
                            e
                        );
                        last_error = Some(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!(
                "No forex provider supports currency pair: {}/{}",
                base_currency,
                target_currency
            )
        }))
    }

    /// Fetches the USD/NGN exchange rate.
    ///
    /// Convenience method for a common currency pair.
    ///
    /// # Returns
    ///
    /// A `Result` containing the USD/NGN rate or an error.
    pub async fn get_usd_ngn_rate(&self) -> Result<Decimal> {
        self.get_exchange_rate("USD", "NGN").await
    }

    /// Adds a new forex provider to the executor.
    ///
    /// # Arguments
    ///
    /// * `provider` - The `ForexProvider` to add.
    pub fn add_forex_provider(&mut self, provider: Box<dyn ForexProvider>) {
        info!(
            "Adding new forex provider: {} (supports {} currencies)",
            provider.provider_name(),
            provider.get_supported_currencies().len()
        );
        self.forex_providers.push(Arc::new(provider));
    }

    /// Removes a forex provider by name.
    ///
    /// # Arguments
    ///
    /// * `provider_name` - The name of the provider to remove.
    ///
    /// # Returns
    ///
    /// `true` if a provider was removed, `false` otherwise.
    pub fn remove_forex_provider(&mut self, provider_name: &str) -> bool {
        let initial_len = self.forex_providers.len();
        self.forex_providers
            .retain(|p| p.provider_name() != provider_name);
        let removed = self.forex_providers.len() < initial_len;

        if removed {
            info!("Removed forex provider: {}", provider_name);
        } else {
            warn!("Forex provider not found: {}", provider_name);
        }

        removed
    }

    /// Lists the names of all forex providers.
    ///
    /// # Returns
    ///
    /// A vector of provider names.
    pub fn list_forex_providers(&self) -> Vec<&str> {
        self.forex_providers
            .iter()
            .map(|p| p.provider_name())
            .collect()
    }

    /// Returns supported currency pairs by provider.
    ///
    /// # Returns
    ///
    /// A vector of tuples containing provider names and their supported currencies.
    pub fn get_supported_currency_pairs(&self) -> Vec<(String, Vec<String>)> {
        let mut result = Vec::new();

        for provider in self.forex_providers.iter() {
            let currencies = provider
                .get_supported_currencies()
                .iter()
                .map(|&s| s.to_string())
                .collect();
            result.push((provider.provider_name().to_string(), currencies));
        }

        result
    }

    /// Checks if a currency pair is supported by any forex provider.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency.
    /// * `target_currency` - The target currency.
    ///
    /// # Returns
    ///
    /// `true` if supported, `false` otherwise.
    pub fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        self.forex_providers
            .iter()
            .any(|provider| provider.supports_currency_pair(base_currency, target_currency))
    }

    /// Fetches exchange rates for multiple currency pairs.
    ///
    /// # Arguments
    ///
    /// * `currency_pairs` - A slice of (base, target) currency pairs.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `HashMap` of pair keys (e.g., "USD_NGN") to rates, or an error.
    pub async fn get_multiple_exchange_rates(
        &self,
        currency_pairs: &[(String, String)],
    ) -> Result<HashMap<String, Decimal>> {
        let mut results = HashMap::new();

        for (base, target) in currency_pairs {
            match self.get_exchange_rate(base, target).await {
                Ok(rate) => {
                    let key = format!("{}_{}", base, target);
                    results.insert(key, rate);
                }
                Err(e) => {
                    error!("Failed to fetch {}/{} rate: {}", base, target, e);
                }
            }
        }

        Ok(results)
    }

    /// Clears the forex cache for a specific pair or all pairs.
    ///
    /// # Arguments
    ///
    /// * `currency_pair` - Optional tuple of (base, target) currencies to clear.
    pub async fn clear_forex_cache(&self, currency_pair: Option<(&str, &str)>) {
        let mut cache = self.forex_cache.write().await;

        if let Some((base, target)) = currency_pair {
            let cache_key = format!("{}_{}", base.to_uppercase(), target.to_uppercase());
            cache.remove(&cache_key);
            info!("Cleared forex cache for {}/{}", base, target);
        } else {
            cache.clear();
            info!("Cleared all forex cache entries");
        }
    }

    /// Returns statistics about the forex cache.
    ///
    /// # Returns
    ///
    /// A `HashMap` containing cache size, duration, and entry details (pair, rate, age, expiration).
    pub async fn get_forex_cache_stats(&self) -> HashMap<String, serde_json::Value> {
        let cache = self.forex_cache.read().await;
        let mut stats = HashMap::new();

        stats.insert("total_entries".to_string(), json!(cache.len()));
        stats.insert(
            "cache_duration_hours".to_string(),
            json!(self.forex_cache_duration_secs / 3600),
        );

        let mut entries = Vec::new();
        for (key, rate) in cache.iter() {
            entries.push(json!({
                "pair": key,
                "rate": rate.rate,
                "age_minutes": rate.age_minutes(),
                "is_expired": rate.is_expired(self.forex_cache_duration_secs)
            }));
        }
        stats.insert("entries".to_string(), json!(entries));

        stats
    }

    /// Sets the minimum profit percentage.
    ///
    /// # Arguments
    ///
    /// * `percentage` - The new minimum profit percentage as a float.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error if the percentage is invalid.
    pub fn set_min_profit_percentage(&mut self, percentage: f64) -> Result<()> {
        self.min_profit_percentage =
            Decimal::from_f64(percentage).context("Invalid profit percentage")?;
        info!(
            "Updated minimum profit percentage to: {}%",
            self.min_profit_percentage * Decimal::from(100)
        );
        Ok(())
    }

    /// Returns the current minimum profit percentage.
    ///
    /// # Returns
    ///
    /// The minimum profit percentage as a float, defaulting to 0.003 if conversion fails.
    pub fn get_min_profit_percentage(&self) -> f64 {
        self.min_profit_percentage.to_f64().unwrap_or(0.003)
    }

    /// Clears the token price cache.
    pub async fn clear_price_cache(&self) {
        let mut cache = self.price_cache.write().await;
        cache.clear();
        info!("Token price cache cleared");
    }

    /// Sets the forex cache duration in hours.
    ///
    /// # Arguments
    ///
    /// * `hours` - The new cache duration in hours.
    pub fn set_forex_cache_duration_hours(&mut self, hours: u64) {
        self.forex_cache_duration_secs = hours * 60 * 60;
        info!("Updated forex cache duration to {} hours", hours);
    }

    /// Returns the forex cache duration in hours.
    ///
    /// # Returns
    ///
    /// The cache duration in hours.
    pub fn get_forex_cache_duration_hours(&self) -> u64 {
        self.forex_cache_duration_secs / 3600
    }

    /// Converts a `Uint128` to a `Decimal`.
    ///
    /// # Arguments
    ///
    /// * `value` - The `Uint128` value to convert.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Decimal` or an error if conversion fails.
    fn uint128_to_decimal(&self, value: Uint128) -> Result<Decimal> {
        let value_str = value.to_string();
        Decimal::from_str(&value_str).context("Failed to convert Uint128 to Decimal")
    }

    /// Queries an intent by ID with retry logic.
    ///
    /// Sends a query to the smart contract to retrieve intent details, retrying up to 3 times on failure.
    ///
    /// # Arguments
    ///
    /// * `intent_id` - The ID of the intent to query.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `IntentResponse` or an error if the query fails after retries.
    async fn get_intent(&self, intent_id: &str) -> Result<IntentResponse> {
        let query_msg = QueryMsg {
            msg: QueryMsgEnum::GetIntent {
                intent_id: intent_id.to_string(),
            },
        };

        let query_data = json!({
            "smart": {
                "contract_addr": self.config.contract_address,
                "msg": query_msg
            }
        });

        let retry_strategy = ExponentialBackoff::from_millis(1000)
            .max_delay(Duration::from_secs(5))
            .take(3);

        let response = Retry::spawn(retry_strategy, || async {
            self.http_client
                .post(&format!(
                    "{}/cosmwasm/wasm/v1/contract/{}/smart",
                    &self.config.rpc_http_endpoint, &self.config.contract_address
                ))
                .json(&query_data)
                .send()
                .await
                .map_err(|e| {
                    warn!("Retrying intent query for {}: {}", intent_id, e);
                    e
                })
        })
        .await
        .context(format!("Failed to query intent {}", intent_id))?;

        let result: Value = response
            .json()
            .await
            .context("Failed to parse query response")?;

        let intent_response: IntentResponse = serde_json::from_value(result["data"].clone())
            .context("Failed to deserialize intent response")?;

        Ok(intent_response)
    }

    /// Increases the allowance for a CW20 token with retry logic.
    ///
    /// Executes a transaction to allow a spender to use a specified amount of tokens, retrying up to
    /// 3 times on failure.
    ///
    /// # Arguments
    ///
    /// * `token_address` - The CW20 token contract address.
    /// * `spender` - The address allowed to spend the tokens.
    /// * `amount` - The amount to allow.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TxResult` or an error if execution fails after retries.
    async fn increase_allowance(
        &self,
        token_address: &str,
        spender: &str,
        amount: &Uint128,
    ) -> Result<TxResult> {
        let execute_msg = ExecuteMsg {
            msg: ExecuteMsgEnum::IncreaseAllowance {
                spender: spender.to_string(),
                amount: *amount,
            },
        };

        self.execute_contract(token_address, &execute_msg, &[])
            .await
    }

    /// Fills an intent by executing a transaction with retry logic.
    ///
    /// Sends a `FillIntent` message to the smart contract with the specified output tokens, retrying
    /// up to 3 times on failure.
    ///
    /// # Arguments
    ///
    /// * `intent_id` - The ID of the intent to fill.
    /// * `output_tokens` - The output tokens to provide.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TxResult` or an error if execution fails after retries.
    async fn fill_intent(&self, intent_id: &str, output_tokens: &[Token]) -> Result<TxResult> {
        let execute_msg = ExecuteMsg {
            msg: ExecuteMsgEnum::FillIntent {
                intent_id: intent_id.to_string(),
                output_tokens: output_tokens.to_vec(),
            },
        };

        self.execute_contract(&self.config.contract_address, &execute_msg, &[])
            .await
    }

    /// Executes a smart contract transaction with retry logic.
    ///
    /// Constructs, signs, and broadcasts a transaction to execute a contract message, retrying up to
    /// 3 times on failure.
    ///
    /// # Arguments
    ///
    /// * `contract_address` - The contract address.
    /// * `msg` - The `ExecuteMsg` to send.
    /// * `funds` - Funds to include in the transaction (e.g., native tokens).
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TxResult` or an error if execution fails after retries.
    async fn execute_contract(
        &self,
        contract_address: &str,
        msg: &ExecuteMsg,
        funds: &[Coin],
    ) -> Result<TxResult> {
        // Validate contract address
        if contract_address.is_empty() {
            return Err(anyhow::anyhow!("Invalid contract address: empty"));
        }

        let account_info = self.get_account_info().await?;

        let execute_contract_msg = cosmrs::cosmwasm::MsgExecuteContract {
            sender: self.account_id.clone(),
            contract: AccountId::from_str(contract_address)
                .map_err(|e| anyhow::anyhow!("Failed to parse contract address: {}", e))?,
            msg: serde_json::to_vec(msg)?,
            funds: funds.to_vec(),
        };

        let tx_body = tx::Body::new(
            vec![
                execute_contract_msg
                    .to_any()
                    .map_err(|e| anyhow::anyhow!("Failed to convert message to Body: {}", e))?,
            ],
            "",
            0u32,
        );

        let signer_info =
            SignerInfo::single_direct(Some(self.signing_key.public_key()), account_info.sequence);

        let fee = Fee::from_amount_and_gas(
            Coin {
                denom: "untrn".parse().unwrap(),
                amount: 1000u64.into(),
            },
            200_000u64,
        );

        let auth_info = tx::AuthInfo {
            signer_infos: vec![signer_info],
            fee,
        };

        let sign_doc = SignDoc::new(
            &tx_body,
            &auth_info,
            &cosmrs::tendermint::chain::Id::try_from("neutron-1".to_string())
                .map_err(|e| anyhow::anyhow!("Failed to parse chain ID: {}", e))?,
            account_info.account_number,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create sign doc: {}", e))?;

        let tx_signed = sign_doc
            .sign(&self.signing_key)
            .map_err(|e| anyhow::anyhow!("Failed to sign transaction: {}", e))?;

        let tx_bytes = tx_signed
            .to_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to serialize transaction: {}", e))?;

        let retry_strategy = ExponentialBackoff::from_millis(1000)
            .max_delay(Duration::from_secs(5))
            .take(3);

        let broadcast_result = Retry::spawn(retry_strategy, || async {
            self.tm_client
                .broadcast_tx_commit(tx_bytes.clone())
                .await
                .map_err(|e| {
                    warn!("Retrying transaction broadcast: {}", e);
                    e
                })
        })
        .await
        .context("Failed to broadcast transaction after retries")?;

        let result = broadcast_result.check_tx.code;
        if !result.is_ok() {
            return Err(anyhow::anyhow!(
                "CheckTx failed: {}",
                broadcast_result.check_tx.log
            ));
        }
        let result = broadcast_result.tx_result.code;
        if !result.is_ok() {
            return Err(anyhow::anyhow!(
                "DeliverTx failed: {}",
                broadcast_result.tx_result.log
            ));
        }

        Ok(TxResult {
            transaction_hash: broadcast_result.hash.to_string(),
            gas_used: broadcast_result.tx_result.gas_used as u64,
        })
    }

    /// Fetches account information (number and sequence) for transaction signing with retry logic.
    ///
    /// Queries the blockchain for the executor's account details, retrying up to 3 times on failure.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `AccountInfo` or an error if the query fails after retries.
    async fn get_account_info(&self) -> Result<AccountInfo> {
        let retry_strategy = ExponentialBackoff::from_millis(1000)
            .max_delay(Duration::from_secs(5))
            .take(3);

        let response = Retry::spawn(retry_strategy, || async {
            self.http_client
                .get(&format!(
                    "{}/cosmos/auth/v1beta1/accounts/{}",
                    self.config.rpc_http_endpoint, self.account_id
                ))
                .send()
                .await
                .map_err(|e| {
                    warn!("Retrying account info query for {}: {}", self.account_id, e);
                    e
                })
        })
        .await
        .context("Failed to get account info after retries")?;

        let result: Value = response
            .json()
            .await
            .context("Failed to parse account response")?;

        let account_number = result
            .get("account")
            .and_then(|acc| acc.get("account_number"))
            .and_then(|num| num.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing account number in response"))?
            .parse::<u64>()
            .context("Invalid account number")?;

        let sequence = result
            .get("account")
            .and_then(|acc| acc.get("sequence"))
            .and_then(|seq| seq.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing sequence in response"))?
            .parse::<u64>()
            .context("Invalid sequence")?;

        Ok(AccountInfo {
            account_number,
            sequence,
        })
    }
}
