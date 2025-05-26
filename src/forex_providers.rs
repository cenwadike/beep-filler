//! # Forex Providers Module
//!
//! This module provides a `ForexProvider` trait and its implementations for fetching exchange rates
//! between currency pairs, designed for financial applications requiring real-time currency conversion.
//! The providers fetch exchange rates for various currency pairs (e.g., USD/EUR) from external APIs,
//! enabling accurate valuation for financial transactions or intent processing.
//!
//! ## Overview
//!
//! The module includes:
//! - **`ForexProvider` Trait**: Defines an interface for fetching exchange rates, checking currency pair
//!   support, retrieving supported currencies, and identifying the provider.
//! - **`ExchangeRateApiProvider`**: Fetches exchange rates using the ExchangeRate-API, supporting a wide
//!   range of global currencies.
//! - **`CurrencyLayerProvider`**: Fetches exchange rates using the CurrencyLayer API, requiring an API
//!   key and supporting additional assets like precious metals (XAU, XAG).
//! - **`CurrencyRate` Struct**: Represents an exchange rate with metadata like timestamp and currency
//!   pair, including methods for expiration checks and formatting.
//!
//! These providers are designed for integration into financial systems, such as intent execution
//! workflows requiring currency conversion rates.
//!
//! ## Key Features
//!
//! - **Asynchronous Operation**: Uses `async`/`await` with `reqwest` for non-blocking HTTP requests.
//! - **Precision**: Employs `rust_decimal::Decimal` for accurate financial calculations.
//! - **Extensibility**: The trait-based design allows adding new providers for additional APIs.
//! - **Error Handling**: Uses `anyhow::Result` for robust error propagation with contextual messages.
//! - **Currency Validation**: Checks currency pair support to prevent invalid requests.
//! - **Caching Support**: The `CurrencyRate` struct tracks rate age and expiration for caching logic.
//! - **Logging**: Includes debug and info logs via the `log` crate for monitoring rate fetches.
//!
//! ## Usage
//!
//! Create an instance of a forex provider and call `fetch_exchange_rate` with a currency pair and an
//! HTTP client. Below is an example:
//!
//! ```rust
//! use anyhow::Result;
//! use reqwest::Client;
//! use forex_providers::{ForexProvider, ExchangeRateApiProvider, CurrencyLayerProvider};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let client = Client::new();
//!     let exchange_rate_provider = ExchangeRateApiProvider;
//!     let currency_layer_provider = CurrencyLayerProvider::new("YOUR_API_KEY".to_string());
//!
//!     // Fetch USD/EUR rate from ExchangeRate-API
//!     if exchange_rate_provider.supports_currency_pair("USD", "EUR") {
//!         let rate = exchange_rate_provider
//!             .fetch_exchange_rate("USD", "EUR", &client)
//!             .await?;
//!         println!("USD/EUR rate: {}", rate);
//!     }
//!
//!     // Fetch USD/NGN rate from CurrencyLayer
//!     if currency_layer_provider.supports_currency_pair("USD", "NGN") {
//!         let rate = currency_layer_provider
//!             .fetch_exchange_rate("USD", "NGN", &client)
//!             .await?;
//!         println!("USD/NGN rate: {}", rate);
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
//! - `rust_decimal`: For precise decimal arithmetic in exchange rate calculations.
//! - `serde_json`: For parsing JSON API responses.
//! - `log`: For debug and info logging of rate fetching operations.
//! - `std`: For async futures, pinning, and synchronization utilities.
//!
//! ## Notes
//!
//! - **API Dependencies**: Relies on external APIs (`exchangerate-api.com` and `currencylayer.com`),
//!   which may have rate limits or require API keys. Consider caching or fallback providers in
//!   production.
//! - **API Key for CurrencyLayer**: The `CurrencyLayerProvider` requires a valid API key, obtainable
//!   from [CurrencyLayer](https://currencylayer.com). The free plan has a 1,000-request monthly limit.
//! - **Thread Safety**: Providers implement `Send` and `Sync` for safe concurrent use.
//! - **Caching**: Use `CurrencyRate::is_expired` to implement caching logic based on rate age.
//! - **HTTPS**: All API requests use HTTPS for secure communication.

use anyhow::{Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Defines an interface for fetching exchange rates between currency pairs.
///
/// Implementors must provide methods to fetch rates, check currency pair support, list supported
/// currencies, and return a provider name. The trait ensures thread safety and asynchronous operation.
pub trait ForexProvider: Send + Sync {
    /// Fetches the exchange rate for a given base and target currency pair.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code (e.g., "USD").
    /// * `target_currency` - The target currency code (e.g., "EUR").
    /// * `client` - The HTTP client for API requests.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to a `Result` with the exchange rate as a `Decimal` or an error.
    fn fetch_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>>;

    /// Checks if the provider supports a given currency pair.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    ///
    /// # Returns
    ///
    /// `true` if the currency pair is supported, `false` otherwise.
    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool;

    /// Returns the name of the forex provider.
    ///
    /// # Returns
    ///
    /// A static string identifying the provider.
    fn provider_name(&self) -> &'static str;

    /// Returns a list of supported currency codes.
    ///
    /// # Returns
    ///
    /// A vector of static strings representing supported currency codes.
    fn get_supported_currencies(&self) -> Vec<&'static str>;
}

/// Represents a currency exchange rate with metadata.
#[derive(Debug)]
pub struct CurrencyRate {
    /// The exchange rate as a `Decimal`.
    pub rate: Decimal,
    /// The timestamp when the rate was fetched.
    pub timestamp: std::time::Instant,
    /// The base currency code (e.g., "USD").
    pub base_currency: String,
    /// The target currency code (e.g., "EUR").
    pub target_currency: String,
}

impl CurrencyRate {
    /// Creates a new `CurrencyRate` instance.
    ///
    /// # Arguments
    ///
    /// * `rate` - The exchange rate.
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    ///
    /// # Returns
    ///
    /// A new `CurrencyRate` instance.
    pub fn new(rate: Decimal, base_currency: String, target_currency: String) -> Self {
        Self {
            rate,
            timestamp: std::time::Instant::now(),
            base_currency,
            target_currency,
        }
    }

    /// Checks if the exchange rate is expired based on cache duration.
    ///
    /// # Arguments
    ///
    /// * `cache_duration_secs` - The cache duration in seconds.
    ///
    /// # Returns
    ///
    /// `true` if the rate is expired, `false` otherwise.
    pub fn is_expired(&self, cache_duration_secs: u64) -> bool {
        self.timestamp.elapsed().as_secs() >= cache_duration_secs
    }

    /// Returns the age of the exchange rate in minutes.
    ///
    /// # Returns
    ///
    /// The age of the rate in minutes.
    pub fn age_minutes(&self) -> u64 {
        self.timestamp.elapsed().as_secs() / 60
    }

    /// Returns the currency pair as a formatted string (e.g., "USD/EUR").
    ///
    /// # Returns
    ///
    /// A string representing the currency pair.
    pub fn currency_pair(&self) -> String {
        format!("{}/{}", self.base_currency, self.target_currency)
    }
}

/// Fetches exchange rates using the ExchangeRate-API.
///
/// Supports a wide range of global currencies, fetching rates from `exchangerate-api.com`.
#[derive(Debug)]
pub struct ExchangeRateApiProvider;

impl ForexProvider for ExchangeRateApiProvider {
    /// Fetches the exchange rate for a currency pair from ExchangeRate-API.
    ///
    /// Sends an HTTP request to `exchangerate-api.com` to retrieve the rate for the specified pair.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    /// * `client` - The HTTP client for API requests.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to the exchange rate or an error if the pair is unsupported or
    /// fetching fails.
    fn fetch_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let base_currency = base_currency.to_string();
        let target_currency = target_currency.to_string();
        let client = client.clone();

        Box::pin(async move {
            if !self.supports_currency_pair(&base_currency, &target_currency) {
                return Err(anyhow::anyhow!(
                    "Currency pair {}/{} not supported by {}",
                    base_currency,
                    target_currency,
                    self.provider_name()
                ));
            }

            log::debug!(
                "Fetching {}/{} rate from {}",
                base_currency,
                target_currency,
                self.provider_name()
            );

            let url = format!(
                "https://api.exchangerate-api.com/v4/latest/{}",
                base_currency.to_uppercase()
            );

            let response: Value = client
                .get(&url)
                .header("User-Agent", "intent-filler/1.0")
                .send()
                .await
                .context(format!(
                    "Failed to fetch {}/{} rate from {}",
                    base_currency,
                    target_currency,
                    self.provider_name()
                ))?
                .json()
                .await
                .context("Failed to parse ExchangeRate-API response")?;

            let rate = response["rates"][&target_currency.to_uppercase()]
                .as_f64()
                .context(format!("Invalid {} exchange rate format", target_currency))?;

            let rate_decimal = Decimal::from_f64(rate).context(format!(
                "Failed to convert {} rate to Decimal",
                target_currency
            ))?;

            log::debug!(
                "{}/{} rate: {:.6} from {}",
                base_currency,
                target_currency,
                rate_decimal,
                self.provider_name()
            );
            Ok(rate_decimal)
        })
    }

    /// Checks if the provider supports a given currency pair.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    ///
    /// # Returns
    ///
    /// `true` if both currencies are supported, `false` otherwise.
    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        let supported_currencies = self.get_supported_currencies();
        let base_upper = base_currency.to_uppercase();
        let target_upper = target_currency.to_uppercase();

        supported_currencies.contains(&base_upper.as_str())
            && supported_currencies.contains(&target_upper.as_str())
    }

    /// Returns the provider's name.
    ///
    /// # Returns
    ///
    /// The static string "ExchangeRate-API".
    fn provider_name(&self) -> &'static str {
        "ExchangeRate-API"
    }

    /// Returns a list of supported currency codes.
    ///
    /// # Returns
    ///
    /// A vector of supported currency codes as static strings.
    fn get_supported_currencies(&self) -> Vec<&'static str> {
        vec![
            "USD", "EUR", "GBP", "JPY", "AUD", "CAD", "CHF", "CNY", "SEK", "NZD", "NGN", "ZAR",
            "KES", "GHS", "EGP", "MAD", "TND", "BWP", "MUR", "UGX", "INR", "PKR", "BDT", "LKR",
            "NPR", "BTN", "MVR", "IDR", "MYR", "SGD", "THB", "PHP", "VND", "KRW", "TWD", "HKD",
            "MOP", "BRN", "KHR", "LAK", "MMK", "BRL", "ARS", "CLP", "COP", "PEN", "UYU", "BOB",
            "PYG", "VES", "MXN", "GTQ", "HNL", "NIO", "CRC", "PAB", "DOP", "HTG", "JMD", "TTD",
            "BBD", "XCD", "BZD", "SRD", "GYD", "FKP", "RUB", "UAH", "BYN", "PLN", "CZK", "HUF",
            "RON", "BGN", "HRK", "RSD", "BAM", "MKD", "ALL", "TRY", "GEL", "AMD", "AZN", "KZT",
            "KGS", "UZS", "TJS", "TMT", "AFN", "IRR",
        ]
    }
}

/// Fetches exchange rates using the CurrencyLayer API.
///
/// Requires a valid API key and supports a range of currencies, including precious metals (XAU, XAG).
#[derive(Debug)]
pub struct CurrencyLayerProvider {
    /// The API key for accessing the CurrencyLayer API.
    api_key: String,
}

impl CurrencyLayerProvider {
    /// Creates a new `CurrencyLayerProvider` instance.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The API key for CurrencyLayer authentication.
    ///
    /// # Returns
    ///
    /// A new `CurrencyLayerProvider` instance.
    ///
    /// # Panics
    ///
    /// Panics if the API key is empty.
    pub fn new(api_key: String) -> Self {
        if api_key.is_empty() {
            panic!("CurrencyLayer API key cannot be empty");
        }
        Self { api_key }
    }
}

impl ForexProvider for CurrencyLayerProvider {
    /// Fetches the exchange rate for a currency pair from CurrencyLayer.
    ///
    /// Sends an HTTP request to `currencylayer.com` with the provided API key to retrieve the rate.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    /// * `client` - The HTTP client for API requests.
    ///
    /// # Returns
    ///
    /// A pinned, boxed future resolving to the exchange rate or an error if the pair is unsupported,
    /// the API key is invalid, or fetching fails.
    fn fetch_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let base_currency = base_currency.to_string();
        let target_currency = target_currency.to_string();
        let api_key = self.api_key.clone();
        let client = client.clone();

        Box::pin(async move {
            if !self.supports_currency_pair(&base_currency, &target_currency) {
                return Err(anyhow::anyhow!(
                    "Currency pair {}/{} not supported by {}",
                    base_currency,
                    target_currency,
                    self.provider_name()
                ));
            }

            log::debug!(
                "Fetching {}/{} rate from {}",
                base_currency,
                target_currency,
                self.provider_name()
            );

            let url = format!(
                "https://api.currencylayer.com/live?access_key={}&source={}",
                api_key,
                base_currency.to_uppercase()
            );

            let response: Value = client
                .get(&url)
                .header("User-Agent", "intent-filler/1.0")
                .send()
                .await
                .context(format!(
                    "Failed to fetch {}/{} rate from {}",
                    base_currency,
                    target_currency,
                    self.provider_name()
                ))?
                .json()
                .await
                .context("Failed to parse CurrencyLayer response")?;

            // Check for API errors
            if !response["success"].as_bool().unwrap_or(false) {
                let error_code = response["error"]["code"]
                    .as_i64()
                    .context("Missing error code in CurrencyLayer response")?;
                let error_info = response["error"]["info"]
                    .as_str()
                    .context("Missing error info in CurrencyLayer response")?;
                return Err(anyhow::anyhow!(
                    "CurrencyLayer API error (code {}): {}",
                    error_code,
                    error_info
                ));
            }

            let rate_key = format!(
                "{}{}",
                base_currency.to_uppercase(),
                target_currency.to_uppercase()
            );
            let rate = response["quotes"][&rate_key]
                .as_f64()
                .context(format!("Invalid {} exchange rate format", target_currency))?;

            let rate_decimal = Decimal::from_f64(rate).context(format!(
                "Failed to convert {} rate to Decimal",
                target_currency
            ))?;

            let currency_rate =
                CurrencyRate::new(rate_decimal, base_currency.clone(), target_currency.clone());
            log::info!(
                "Fetched rate for {}: {} from {}",
                currency_rate.currency_pair(),
                rate_decimal,
                self.provider_name()
            );

            Ok(rate_decimal)
        })
    }

    /// Checks if the provider supports a given currency pair.
    ///
    /// # Arguments
    ///
    /// * `base_currency` - The base currency code.
    /// * `target_currency` - The target currency code.
    ///
    /// # Returns
    ///
    /// `true` if both currencies are supported, `false` otherwise.
    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        let supported_currencies = self.get_supported_currencies();
        let base_upper = base_currency.to_uppercase();
        let target_upper = target_currency.to_uppercase();

        supported_currencies.contains(&base_upper.as_str())
            && supported_currencies.contains(&target_upper.as_str())
    }

    /// Returns the provider's name.
    ///
    /// # Returns
    ///
    /// The static string "CurrencyLayer".
    fn provider_name(&self) -> &'static str {
        "CurrencyLayer"
    }

    /// Returns a list of supported currency codes.
    ///
    /// # Returns
    ///
    /// A vector of supported currency codes as static strings, including precious metals.
    fn get_supported_currencies(&self) -> Vec<&'static str> {
        vec![
            "USD", "EUR", "GBP", "JPY", "AUD", "CAD", "CHF", "CNY", "SEK", "NZD", "NGN", "ZAR",
            "KES", "GHS", "EGP", "MAD", "TND", "BWP", "MUR", "UGX", "INR", "PKR", "BDT", "LKR",
            "NPR", "BTN", "MVR", "IDR", "MYR", "SGD", "THB", "PHP", "VND", "KRW", "TWD", "HKD",
            "MOP", "BRN", "KHR", "LAK", "MMK", "BRL", "ARS", "CLP", "COP", "PEN", "UYU", "BOB",
            "PYG", "VES", "XAU", "XAG",
        ]
    }
}
