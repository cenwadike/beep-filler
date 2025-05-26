use cosmwasm_std::{Addr, Uint128};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct IntentCreatedEvent {
    pub intent_id: String,
    pub status: String, // String due to event parsing; maps to IntentStatus
    pub sender: String,
    pub block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub enum IntentStatus {
    Active,
    Completed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct BeepCoin {
    pub token: String,
    pub amount: Uint128,
    pub is_native: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Token {
    pub token: String,
    pub amount: Uint128,
    pub is_native: bool,
    pub target_address: Option<Addr>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct SwapIntent {
    pub output_tokens: Vec<Token>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(tag = "type")]
pub enum IntentType {
    Swap(SwapIntent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Intent {
    pub id: String,
    pub creator: Addr,
    pub input_tokens: Vec<BeepCoin>,
    pub intent_type: IntentType,
    pub executor: Option<Addr>,
    pub status: IntentStatus,
    pub created_at: u64,
    pub timeout: u64,
    pub tip: BeepCoin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct IntentResponse {
    pub intent: Intent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct QueryMsg {
    #[serde(flatten)]
    pub msg: QueryMsgEnum,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsgEnum {
    GetIntent { intent_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ExecuteMsg {
    #[serde(flatten)]
    pub msg: ExecuteMsgEnum,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsgEnum {
    FillIntent {
        intent_id: String,
        output_tokens: Vec<Token>,
    },
    IncreaseAllowance {
        spender: String,
        amount: Uint128, // Changed from String to Uint128
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct TxResult {
    pub transaction_hash: String,
    pub gas_used: u64,
}

pub type EventAttributes = HashMap<String, String>;

#[derive(Debug)]
pub struct ProfitAnalysis {
    pub input_value_usd: Decimal,
    pub output_value_usd: Decimal,
    pub tip_value_usd: Decimal,
    pub total_revenue_usd: Decimal,
    pub profit_usd: Decimal,
    pub profit_percentage: Decimal,
    pub is_profitable: bool,
}
