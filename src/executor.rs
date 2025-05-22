use anyhow::{Context, Result};
use cosmrs::{
    AccountId, Coin,
    crypto::secp256k1::SigningKey,
    tx::{self, Fee, Msg, SignDoc, SignerInfo},
};
use cosmwasm_std::Uint128;
use log::{info, warn};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Digest;
use std::str::FromStr;
use std::sync::Arc;
use tendermint_rpc::{Client as TmClient, HttpClient, Url};

use crate::types::{
    ExecuteMsg, ExecuteMsgEnum, IntentCreatedEvent, IntentResponse, QueryMsg, QueryMsgEnum, Token,
    TxResult,
};
use crate::{config::Config, types::IntentStatus};

#[derive(Clone)]
pub struct IntentExecutor {
    config: Config,
    http_client: Client,
    tm_client: HttpClient,
    signing_key: Arc<SigningKey>, // Wrap in Arc for Clone
    account_id: AccountId,
}

impl IntentExecutor {
    pub async fn new(config: Config, mnemonic: String) -> Result<Self> {
        let http_client = Client::new();

        let url = Url::from_str(&config.rpc_http_endpoint).context("Invalid RPC endpoint URL")?;
        let tm_client = HttpClient::new(url).context("Failed to create Tendermint client")?;

        // Derive signing key from mnemonic
        let signing_key = Self::signing_key_from_mnemonic(&mnemonic)?;
        let public_key = signing_key.public_key();
        let account_id = public_key
            .account_id("neutron")
            .map_err(|e| anyhow::anyhow!("Failed to get account ID: {}", e))?;

        info!("Initialized intent filler for account: {}", account_id);

        Ok(Self {
            config,
            http_client,
            tm_client,
            signing_key: Arc::new(signing_key),
            account_id,
        })
    }

    fn signing_key_from_mnemonic(mnemonic: &str) -> Result<SigningKey> {
        // Simplified mnemonic derivation (use BIP-39/BIP-44 in production)
        let mut hasher = sha2::Sha256::new();
        hasher.update(mnemonic.as_bytes());
        let hash = hasher.finalize();

        SigningKey::from_slice(&hash)
            .map_err(|e| anyhow::anyhow!("Failed to create signing key: {}", e))
    }

    pub async fn handle_intent_event(&self, event: IntentCreatedEvent) -> Result<()> {
        info!("New Intent Created:");
        info!("Intent ID: {}", event.intent_id);
        info!("Status: {}", event.status);
        info!("Creator: {}", event.sender);
        info!("Block Height: {}", event.block_height);

        info!("Validating Intent...");
        let intent_response = self.get_intent(&event.intent_id).await?;

        // Align with contract's IntentStatus::Active
        if intent_response.intent.status != IntentStatus::Active {
            warn!("Skipping Intent. Reason: Intent not active");
            return Ok(());
        }

        match &intent_response.intent.intent_type {
            crate::types::IntentType::Swap(swap_intent) => {
                // Increase allowances for non-native output tokens
                info!("Increasing expected token allowance");
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

                // Fill the intent
                info!("Filling intent");
                let tx_result = self
                    .fill_intent(&event.intent_id, &swap_intent.output_tokens)
                    .await
                    .context("Failed to fill intent")?;

                info!(
                    "Filled intent successfully. Tx Hash: {}",
                    tx_result.transaction_hash
                );
                info!("------------------------");
            }
        }

        Ok(())
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
        // Get account info
        let account_info = self.get_account_info().await?;

        // Create the message
        let execute_contract_msg = cosmrs::cosmwasm::MsgExecuteContract {
            sender: self.account_id.clone(),
            contract: AccountId::from_str(contract_address)
                .map_err(|e| anyhow::anyhow!("Failed to parse contract address: {}", e))?,
            msg: serde_json::to_vec(msg)?,
            funds: funds.to_vec(),
        };

        // Create transaction body
        let tx_body = tx::Body::new(
            vec![
                execute_contract_msg
                    .to_any()
                    .map_err(|e| anyhow::anyhow!("Failed to convert message to Any: {}", e))?,
            ],
            "",
            0u32,
        );

        // Create signer info
        let signer_info =
            SignerInfo::single_direct(Some(self.signing_key.public_key()), account_info.sequence);

        // Create auth info with a fee (adjust fee as needed)
        let fee = Fee::from_amount_and_gas(
            Coin {
                denom: "untrn".parse().unwrap(), // Adjust denom for Neutron
                amount: 1000u64.into(),
            },
            200_000u64, // Gas limit
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

        // Broadcast transaction
        let broadcast_result = self
            .tm_client
            .broadcast_tx_commit(tx_bytes) // Use commit for confirmation
            .await
            .context("Failed to broadcast transaction")?;

        // Check transaction results
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

#[derive(Debug)]
struct AccountInfo {
    account_number: u64,
    sequence: u64,
}
