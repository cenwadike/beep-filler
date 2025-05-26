//! # Price Providers Module
//!
//! This module provides a `PriceProvider` trait and its implementations for fetching token prices in USD,
//! supporting a blockchain-based intent system, likely for a Cosmos-based smart contract platform.
//! The providers fetch prices for specific tokens (e.g., Nigerian Naira - NGN, and Cosmos ATOM) from
//! external APIs or an `IntentExecutor`, enabling accurate valuation of tokens for intent processing,
//! such as profitability analysis for token swaps.
//!
//! ## Overview
//!
//! The module includes:
//! - **`PriceProvider` Trait**: Defines an interface for fetching token prices, checking token support,
//!   and identifying the provider.
//! - **`NGNPriceProvider`**: Fetches the USD price of NGN using an exchange rate API or an
//!   `IntentExecutor` if provided, supporting tokens "ungn", "NGN", and "ngn".
//! - **`AtomCoinGeckoProvider`**: Fetches the USD price of ATOM using the CoinGecko API, supporting
//!   tokens "uatom", "ATOM", and "atom".
//!
//! These providers integrate with the `IntentExecutor` (from `crate::executor`) to support financial
//! calculations in intent execution workflows.
//!
//! ## Key Features
//!
//! - **Asynchronous Operation**: Uses `async`/`await` with `reqwest` for non-blocking HTTP requests.
//! - **Precision**: Employs `rust_decimal::Decimal` for accurate financial calculations.
//! - **Extensibility**: The trait-based design allows adding new providers for additional tokens.
//! - **Error Handling**: Uses `anyhow::Result` for robust error propagation with contextual messages.
//! - **Token Validation**: Checks token support to prevent invalid price requests.
//! - **Logging**: Includes debug logs via the `log` crate for monitoring price fetches.
//!
//! ## Usage
//!
//! Create an instance of a price provider and call `fetch_price` with a token identifier, an HTTP client,
//! and an optional `IntentExecutor`. Below is an example:
//!
//! ```rust
//! use anyhow::Result;
//! use reqwest::Client;
//! use intent_system::price_providers::{PriceProvider, NGNPriceProvider, AtomCoinGeckoProvider};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let client = Client::new();
//!     let ngn_provider = NGNPriceProvider;
//!     let atom_provider = AtomCoinGeckoProvider;
//!
//!     // Fetch NGN price
//!     if ngn_provider.supports_token("ngn") {
//!         let price = ngn_provider.fetch_price("ngn", &client, None).await?;
//!         println!("NGN price: ${}", price);
//!     }
//!
//!     // Fetch ATOM price
//!     if atom_provider.supports_token("uatom") {
//!         let price = atom_provider.fetch_price("uatom", &client, None).await?;
//!         println!("ATOM price: ${}", price);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Dependencies
//!
//! - `anyhow`: For error handling with contextual information.
//! - `reqwest`: For HTTP requests to external APIs.
//! - `rust_decimal`: For precise decimal arithmetic in price calculations.
//! - `serde_json`: For parsing JSON API responses.
//! - `log`: For debug logging of price fetching operations.
//! - `std`: For async futures, pinning, and synchronization utilities.
//!
//! ## Integration with Intent System
//!
//! The providers are used by the `IntentExecutor` to value tokens in `ProfitAnalysis` (from the `types`
//! module) during intent execution. The `NGNPriceProvider` can use an `IntentExecutor` to fetch cached
//! or alternative NGN rates, while the `AtomCoinGeckoProvider` relies solely on CoinGecko.
//!
//! ## Notes
//!
//! - **API Dependencies**: Relies on external APIs (`exchangerate-api.com` for NGN, `coingecko.com`
//!   for ATOM), which may have rate limits or availability issues. Consider caching or fallbacks in
//!   production.
//! - **Executor Coupling**: The `NGNPriceProvider` optionally uses an `IntentExecutor`, introducing
//!   dependency but enabling flexible rate sourcing.
//! - **Thread Safety**: Providers implement `Send` and `Sync` for safe concurrent use.

use anyhow::{Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use crate::executor::IntentExecutor;

/// Defines an interface for fetching token prices in USD.
///
/// Implementors must provide methods to fetch prices, check token support, and return a provider name.
/// The trait ensures thread safety and asynchronous operation.
pub trait PriceProvider: Send + Sync {
    /// Fetches the USD price of a specified token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier (e.g., "ngn", "uatom").
    /// * `client` - The HTTP client for API requests.
    /// * `executor` - Optional `IntentExecutor` for alternative data sources.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to a `Result` with the price as a `Decimal` or an error.
    fn fetch_price(
        &self,
        token: &str,
        client: &Client,
        executor: Option<&IntentExecutor>,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>>;

    /// Checks if the provider supports a given token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier.
    ///
    /// # Returns
    ///
    /// `true` if the token is supported, `false` otherwise.
    fn supports_token(&self, token: &str) -> bool;

    /// Returns the name of the price provider.
    ///
    /// # Returns
    ///
    /// A static string identifying the provider.
    fn provider_name(&self) -> &'static str;
}

/// Fetches USD prices for the Nigerian Naira (NGN).
///
/// Retrieves prices either via an `IntentExecutor`'s `get_usd_ngn_rate` method or directly from an
/// exchange rate API. Supports tokens "ungn", "NGN", and "ngn".
pub struct NGNPriceProvider;

impl PriceProvider for NGNPriceProvider {
    /// Fetches the USD price of NGN.
    ///
    /// Uses the `IntentExecutor` if provided to get the USD/NGN rate; otherwise, fetches directly from
    /// an API. Returns the inverse of the USD/NGN rate as the NGN price in USD.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier (must be "ungn", "NGN", or "ngn").
    /// * `client` - The HTTP client for API requests.
    /// * `executor` - Optional `IntentExecutor` for rate fetching.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to the NGN price in USD or an error if the token is unsupported
    /// or fetching fails.
    fn fetch_price(
        &self,
        token: &str,
        client: &Client,
        executor: Option<&IntentExecutor>,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let token = token.to_string();
        let client = client.clone();
        let executor = executor.map(|e| std::sync::Arc::new(e.clone())); // Clone executor if present

        Box::pin(async move {
            if !self.supports_token(&token) {
                return Err(anyhow::anyhow!(
                    "Token {} not supported by NGN price provider",
                    token
                ));
            }

            log::debug!("Fetching NGN price from NGN price provider");

            let usd_ngn_rate = if let Some(exec) = executor {
                exec.get_usd_ngn_rate().await?
            } else {
                self.fetch_usd_ngn_rate_direct(&client).await?
            };

            let ngn_price_usd = Decimal::ONE / usd_ngn_rate;

            log::debug!(
                "NGN price: ${:.6} (USD/NGN rate: {:.4})",
                ngn_price_usd,
                usd_ngn_rate
            );
            Ok(ngn_price_usd)
        })
    }

    /// Checks if the provider supports a given token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier.
    ///
    /// # Returns
    ///
    /// `true` for "ungn", "NGN", or "ngn"; `false` otherwise.
    fn supports_token(&self, token: &str) -> bool {
        matches!(token, "ungn" | "NGN" | "ngn")
    }

    /// Returns the provider's name.
    ///
    /// # Returns
    ///
    /// The static string "NGN price".
    fn provider_name(&self) -> &'static str {
        "NGN price"
    }
}

impl NGNPriceProvider {
    /// Fetches the USD/NGN exchange rate directly from an external API.
    ///
    /// Sends an HTTP request to `exchangerate-api.com` to retrieve the USD/NGN rate.
    ///
    /// # Arguments
    ///
    /// * `client` - The HTTP client for the API request.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to the USD/NGN rate as a `Decimal` or an error if the request
    /// or parsing fails.
    fn fetch_usd_ngn_rate_direct(
        &self,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let client = client.clone();

        Box::pin(async move {
            let url = "https://api.exchangerate-api.com/v4/latest/USD";

            let response: Value = client
                .get(url)
                .header("User-Agent", "intent-filler/1.0")
                .send()
                .await
                .context("Failed to fetch USD/NGN rate")?
                .json()
                .await
                .context("Failed to parse exchange rate response")?;

            let ngn_rate = response["rates"]["NGN"]
                .as_f64()
                .context("Invalid NGN exchange rate format")?;

            Decimal::from_f64(ngn_rate).context("Failed to convert NGN rate to decimal")
        })
    }
}

/// Fetches USD prices for the Cosmos ATOM token using the CoinGecko API.
///
/// Supports tokens identified as "uatom", "ATOM", or "atom".
pub struct AtomCoinGeckoProvider;

impl PriceProvider for AtomCoinGeckoProvider {
    /// Fetches the USD price of ATOM from CoinGecko.
    ///
    /// Sends an HTTP request to the CoinGecko API to retrieve the current ATOM price in USD.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier (must be "uatom", "ATOM", or "atom").
    /// * `client` - The HTTP client for API requests.
    /// * `_executor` - Ignored, as this provider does not use an executor.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to the ATOM price in USD or an error if the token is
    /// unsupported or fetching fails.
    fn fetch_price(
        &self,
        token: &str,
        client: &Client,
        _executor: Option<&IntentExecutor>,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let token = token.to_string();
        let client = client.clone();

        Box::pin(async move {
            if !self.supports_token(&token) {
                return Err(anyhow::anyhow!(
                    "Token {} not supported by ATOM CoinGecko provider",
                    token
                ));
            }

            log::debug!("Fetching ATOM price from CoinGecko");

            let url = "https://api.coingecko.com/api/v3/simple/price?ids=cosmos&vs_currencies=usd";

            let response: Value = client
                .get(url)
                .header("User-Agent", "intent-filler/1.0")
                .send()
                .await
                .context("Failed to fetch ATOM price from CoinGecko")?
                .json()
                .await
                .context("Failed to parse CoinGecko response")?;

            let price_f64 = response["cosmos"]["usd"]
                .as_f64()
                .context("Invalid ATOM price format from CoinGecko")?;

            let price =
                Decimal::from_f64(price_f64).context("Failed to convert ATOM price to decimal")?;

            log::debug!("ATOM price: ${:.6}", price);
            Ok(price)
        })
    }

    /// Checks if the provider supports a given token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identifier.
    ///
    /// # Returns
    ///
    /// `true` for "uatom", "ATOM", or "atom"; `false` otherwise.
    fn supports_token(&self, token: &str) -> bool {
        matches!(token, "uatom" | "ATOM" | "atom")
    }

    /// Returns the provider's name.
    ///
    /// # Returns
    ///
    /// The static string "CoinGecko-ATOM".
    fn provider_name(&self) -> &'static str {
        "CoinGecko-ATOM"
    }
}
