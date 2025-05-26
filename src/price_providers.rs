use anyhow::{Context, Result};
use reqwest::Client;
use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use crate::executor::IntentExecutor;

// Define the PriceProvider trait without async_trait
pub trait PriceProvider: Send + Sync {
    fn fetch_price(
        &self,
        token: &str,
        client: &Client,
        executor: Option<&IntentExecutor>,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>>;
    fn supports_token(&self, token: &str) -> bool;
    fn provider_name(&self) -> &'static str;
}

pub struct NGNPriceProvider;

impl PriceProvider for NGNPriceProvider {
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

    fn supports_token(&self, token: &str) -> bool {
        matches!(token, "ungn" | "NGN" | "ngn")
    }

    fn provider_name(&self) -> &'static str {
        "NGN price"
    }
}

impl NGNPriceProvider {
    fn fetch_usd_ngn_rate_direct(
        &self,
        client: &Client,
    ) -> Pin<Box<dyn Future<Output = Result<Decimal>> + Send + '_>> {
        let client = client.clone();

        Box::pin(async move {
            let url = "https://api.exchangerate-api.com/v4/latest/USD";

            let response: Value = client
                .get(url)
                .header("User-Agent", "intent-executor/1.0")
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

pub struct AtomCoinGeckoProvider;

impl PriceProvider for AtomCoinGeckoProvider {
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
                .header("User-Agent", "intent-executor/1.0")
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

    fn supports_token(&self, token: &str) -> bool {
        matches!(token, "uatom" | "ATOM" | "atom")
    }

    fn provider_name(&self) -> &'static str {
        "CoinGecko-ATOM"
    }
}
