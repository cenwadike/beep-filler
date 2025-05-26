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
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tendermint_rpc::{Client as TmClient, HttpClient, Url};

use crate::forex_providers::{
    CurrencyLayerProvider, CurrencyRate, ExchangeRateApiProvider, ForexProvider,
};
use crate::price_providers::{AtomCoinGeckoProvider, NGNPriceProvider, PriceProvider};
use crate::types::{
    BeepCoin, ExecuteMsg, ExecuteMsgEnum, IntentCreatedEvent, IntentResponse, IntentStatus,
    QueryMsg, QueryMsgEnum, Token, TxResult,
};
use crate::{config::Config, types::ProfitAnalysis};

#[derive(Clone)]
pub struct IntentExecutor {
    config: Config,
    http_client: Client,
    tm_client: HttpClient,
    signing_key: Arc<SigningKey>,
    account_id: AccountId,
    min_profit_percentage: Decimal,
    price_cache: Arc<tokio::sync::RwLock<HashMap<String, (Decimal, std::time::Instant)>>>,
    forex_cache: Arc<tokio::sync::RwLock<HashMap<String, CurrencyRate>>>,
    forex_cache_duration_secs: u64,
    forex_providers: Vec<Arc<Box<dyn ForexProvider>>>,
    price_providers: Vec<Arc<Box<dyn PriceProvider>>>,
}

#[derive(Debug)]
struct AccountInfo {
    account_number: u64,
    sequence: u64,
}

impl IntentExecutor {
    pub async fn new(
        config: Config,
        mnemonic: String,
        min_profit_percentage: Option<f64>,
    ) -> Result<Self> {
        let http_client = Client::new();

        let url = Url::from_str(&config.rpc_http_endpoint).context("Invalid RPC endpoint URL")?;
        let tm_client = HttpClient::new(url).context("Failed to create Tendermint client")?;

        let signing_key = Self::signing_key_from_mnemonic(&mnemonic)?;
        let public_key = signing_key.public_key();
        let account_id = public_key
            .account_id("neutron")
            .map_err(|e| anyhow::anyhow!("Failed to get account ID: {}", e))?;

        let min_profit_percentage = Decimal::from_f64(min_profit_percentage.unwrap_or(0.003))
            .unwrap_or_else(|| Decimal::from_str("0.003").unwrap());

        let price_providers: Vec<Arc<Box<dyn PriceProvider>>> = vec![
            Arc::new(Box::new(NGNPriceProvider)),
            Arc::new(Box::new(AtomCoinGeckoProvider)),
        ];

        let forex_providers: Vec<Arc<Box<dyn ForexProvider>>> = vec![
            Arc::new(Box::new(ExchangeRateApiProvider)),
            Arc::new(Box::new(CurrencyLayerProvider::new(None))),
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

    fn signing_key_from_mnemonic(mnemonic: &str) -> Result<SigningKey> {
        let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)?;
        let seed = Seed::new(mnemonic.to_seed(""));

        let master_key = XPrv::new(seed.as_bytes())?;
        let child_number = ChildNumber::default();
        let derived_key = master_key.derive_child(child_number)?;
        let signing_key =
            SigningKey::from_slice(derived_key.private_key().to_bytes().as_slice()).unwrap();

        Ok(signing_key)
    }

    pub async fn handle_intent_event(&self, event: IntentCreatedEvent) -> Result<()> {
        info!("New Intent Created:");
        info!("Intent ID: {}", event.intent_id);
        info!("Status: {}", event.status);
        info!("Creator: {}", event.sender);
        info!("Block Height: {}", event.block_height);

        info!("Validating Intent...");
        let intent_response = self.get_intent(&event.intent_id).await?;

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
                        self.increase_allowance(
                            &token.token,
                            &self.config.contract_address,
                            &token.amount,
                        )
                        .await
                        .context(format!(
                            "Failed to increase allowance for token {}",
                            token.token
                        ))?;
                    }
                }

                info!("Filling intent");
                let tx_result = self
                    .fill_intent(&event.intent_id, &swap_intent.output_tokens)
                    .await
                    .context("Failed to fill intent")?;

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

    async fn get_token_price_usd(&self, token: &str) -> Result<Decimal> {
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

    async fn fetch_token_price_from_api(&self, token: &str) -> Result<Decimal> {
        let price_provider = self.get_price_provider_for_token(token)?;
        price_provider
            .fetch_price(token, &self.http_client, Some(self))
            .await
    }

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

    pub fn add_price_provider(&mut self, provider: Box<dyn PriceProvider>) {
        info!("Adding new price provider: {}", provider.provider_name());
        self.price_providers.push(Arc::new(provider));
    }

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

    pub fn list_price_providers(&self) -> Vec<&str> {
        self.price_providers
            .iter()
            .map(|p| p.provider_name())
            .collect()
    }

    pub async fn get_exchange_rate(
        &self,
        base_currency: &str,
        target_currency: &str,
    ) -> Result<Decimal> {
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

    async fn fetch_exchange_rate_from_providers(
        &self,
        base_currency: &str,
        target_currency: &str,
    ) -> Result<Decimal> {
        let mut last_error = None;

        for provider in self.forex_providers.iter() {
            if provider.supports_currency_pair(base_currency, target_currency) {
                match provider
                    .fetch_exchange_rate(base_currency, target_currency, &self.http_client)
                    .await
                {
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
                            "Failed to fetch {}/{} rate from {}: {}",
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

    pub async fn get_usd_ngn_rate(&self) -> Result<Decimal> {
        self.get_exchange_rate("USD", "NGN").await
    }

    pub fn add_forex_provider(&mut self, provider: Box<dyn ForexProvider>) {
        info!(
            "Adding new forex provider: {} (supports {} currencies)",
            provider.provider_name(),
            provider.get_supported_currencies().len()
        );
        self.forex_providers.push(Arc::new(provider));
    }

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

    pub fn list_forex_providers(&self) -> Vec<&str> {
        self.forex_providers
            .iter()
            .map(|p| p.provider_name())
            .collect()
    }

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

    pub fn supports_currency_pair(&self, base_currency: &str, target_currency: &str) -> bool {
        self.forex_providers
            .iter()
            .any(|provider| provider.supports_currency_pair(base_currency, target_currency))
    }

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

    pub fn set_min_profit_percentage(&mut self, percentage: f64) -> Result<()> {
        self.min_profit_percentage =
            Decimal::from_f64(percentage).context("Invalid profit percentage")?;
        info!(
            "Updated minimum profit percentage to: {}%",
            self.min_profit_percentage * Decimal::from(100)
        );
        Ok(())
    }

    pub fn get_min_profit_percentage(&self) -> f64 {
        self.min_profit_percentage.to_f64().unwrap_or(0.003)
    }

    pub async fn clear_price_cache(&self) {
        let mut cache = self.price_cache.write().await;
        cache.clear();
        info!("Token price cache cleared");
    }

    pub fn set_forex_cache_duration_hours(&mut self, hours: u64) {
        self.forex_cache_duration_secs = hours * 60 * 60;
        info!("Updated forex cache duration to {} hours", hours);
    }

    pub fn get_forex_cache_duration_hours(&self) -> u64 {
        self.forex_cache_duration_secs / 3600
    }

    fn uint128_to_decimal(&self, value: Uint128) -> Result<Decimal> {
        let value_str = value.to_string();
        Decimal::from_str(&value_str).context("Failed to convert Uint128 to Decimal")
    }

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

        let response = self
            .http_client
            .post(&format!(
                "{}/cosmwasm/wasm/v1/contract/{}/smart",
                &self.config.rpc_http_endpoint, &self.config.contract_address
            ))
            .json(&query_data)
            .send()
            .await
            .context("Failed to query intent")?;

        let result: Value = response
            .json()
            .await
            .context("Failed to parse query response")?;

        let intent_response: IntentResponse = serde_json::from_value(result["data"].clone())
            .context("Failed to deserialize intent response")?;

        Ok(intent_response)
    }

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

    async fn execute_contract(
        &self,
        contract_address: &str,
        msg: &ExecuteMsg,
        funds: &[Coin],
    ) -> Result<TxResult> {
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
                    .map_err(|e| anyhow::anyhow!("Failed to convert message to Any: {}", e))?,
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

        let broadcast_result = self
            .tm_client
            .broadcast_tx_commit(tx_bytes)
            .await
            .context("Failed to broadcast transaction")?;

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

    async fn get_account_info(&self) -> Result<AccountInfo> {
        let response = self
            .http_client
            .get(&format!(
                "{}/cosmos/auth/v1beta1/accounts/{}",
                self.config.rpc_http_endpoint, self.account_id
            ))
            .send()
            .await
            .context("Failed to get account info")?;

        let result: Value = response
            .json()
            .await
            .context("Failed to parse account response")?;

        let account_number = result["account"]["account_number"]
            .as_str()
            .context("Missing account number")?
            .parse::<u64>()
            .context("Invalid account number")?;

        let sequence = result["account"]["sequence"]
            .as_str()
            .context("Missing sequence")?
            .parse::<u64>()
            .context("Invalid sequence")?;

        Ok(AccountInfo {
            account_number,
            sequence,
        })
    }
}
