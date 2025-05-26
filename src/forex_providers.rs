use anyhow::{Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

// Define the ForexProvider trait with explicit lifetime
pub trait ForexProvider: Send + Sync {
    fn fetch_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>>;
    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool;
    fn provider_name(&self) -> &'static str;
    fn get_supported_currencies(&self) -> Vec<&'static str>;
}

#[derive(Debug)]
pub struct CurrencyRate {
    pub rate: Decimal,
    pub timestamp: std::time::Instant,
    pub base_currency: String,
    pub target_currency: String,
}

impl CurrencyRate {
    pub fn new(rate: Decimal, base_currency: String, target_currency: String) -> Self {
        Self {
            rate,
            timestamp: std::time::Instant::now(),
            base_currency,
            target_currency,
        }
    }

    pub fn is_expired(&self, cache_duration_secs: u64) -> bool {
        self.timestamp.elapsed().as_secs() >= cache_duration_secs
    }

    pub fn age_minutes(&self) -> u64 {
        self.timestamp.elapsed().as_secs() / 60
    }

    pub fn currency_pair(&self) -> String {
        format!("{}/{}", self.base_currency, self.target_currency)
    }
}

#[derive(Debug)]
pub struct ExchangeRateApiProvider;

impl ForexProvider for ExchangeRateApiProvider {
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
                    "Currency pair {}/{} not supported by ExchangeRateAPI provider",
                    base_currency,
                    target_currency
                ));
            }

            let url = format!(
                "https://api.exchangerate-api.com/v4/latest/{}",
                base_currency.to_uppercase()
            );

            log::debug!(
                "Fetching {}/{} rate from ExchangeRateAPI",
                base_currency,
                target_currency
            );

            let response: Value = client
                .get(&url)
                .header("User-Agent", "intent-executor/1.0")
                .send()
                .await
                .context(format!(
                    "Failed to fetch {}/{} rate",
                    base_currency, target_currency
                ))?
                .json()
                .await
                .context("Failed to parse exchange rate response")?;

            let rate = response["rates"][target_currency.to_uppercase()]
                .as_f64()
                .context(format!("Invalid {} exchange rate format", target_currency))?;

            Decimal::from_f64(rate).context(format!(
                "Failed to convert {} rate to decimal",
                target_currency
            ))
        })
    }

    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        let supported_currencies = self.get_supported_currencies();
        let base_upper = base_currency.to_uppercase();
        let target_upper = target_currency.to_uppercase();

        supported_currencies.contains(&base_upper.as_str())
            && supported_currencies.contains(&target_upper.as_str())
    }

    fn provider_name(&self) -> &'static str {
        "ExchangeRateAPI"
    }

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

#[derive(Debug)]
pub struct CurrencyLayerProvider {
    #[allow(dead_code)]
    api_key: Option<String>,
}

impl CurrencyLayerProvider {
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }
}

impl ForexProvider for CurrencyLayerProvider {
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
                    "Currency pair {}/{} not supported by ExchangeRateAPI provider",
                    base_currency,
                    target_currency
                ));
            }

            let url = format!(
                "https://api.exchangerate-api.com/v4/latest/{}",
                base_currency.to_uppercase()
            );

            log::debug!(
                "Fetching {}/{} rate from ExchangeRateAPI",
                base_currency,
                target_currency
            );

            let response: Value = client
                .get(&url)
                .header("User-Agent", "intent-executor/1.0")
                .send()
                .await
                .context(format!(
                    "Failed to fetch {}/{} rate",
                    base_currency, target_currency
                ))?
                .json()
                .await
                .context("Failed to parse exchange rate response")?;

            let rate = response["rates"][target_currency.to_uppercase()]
                .as_f64()
                .context(format!("Invalid {} exchange rate format", target_currency))?;

            let rate_decimal = Decimal::from_f64(rate).context(format!(
                "Failed to convert {} rate to decimal",
                target_currency
            ))?;

            // Create CurrencyRate and use currency_pair for logging
            let currency_rate =
                CurrencyRate::new(rate_decimal, base_currency.clone(), target_currency.clone());
            log::info!(
                "Fetched rate for {}: {}",
                currency_rate.currency_pair(),
                rate_decimal
            );

            Ok(rate_decimal)
        })
    }

    fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        let supported_currencies = self.get_supported_currencies();
        let base_upper = base_currency.to_uppercase();
        let target_upper = target_currency.to_uppercase();

        supported_currencies.contains(&base_upper.as_str())
            && supported_currencies.contains(&target_upper.as_str())
    }

    fn provider_name(&self) -> &'static str {
        "CurrencyLayer"
    }

    fn get_supported_currencies(&self) -> Vec<&'static str> {
        vec![
            "USD", "EUR", "GBP", "JPY", "AUD", "CAD", "CHF", "CNY", "SEK", "NZD", "NGN", "ZAR",
            "KES", "GHS", "EGP", "MAD", "TND", "BWP", "MUR", "UGX", "INR", "PKR", "BDT", "LKR",
            "NPR", "BTN", "MVR", "IDR", "MYR", "SGD", "THB", "PHP", "VND", "KRW", "TWD", "HKD",
            "MOP", "BRN", "KHR", "LAK", "MMK", "BRL", "ARS", "CLP", "COP", "PEN", "UYU", "BOB",
            "PYG", "VES",
        ]
    }
}
