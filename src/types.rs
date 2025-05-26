//! # Types Module for Intent System
//!
//! This module defines the core data structures and types used in a blockchain-based intent system,
//! likely for a Cosmos-based smart contract platform. The types are designed to support intent creation,
//! execution, and querying, with a focus on token swaps and associated metadata. The system integrates
//! with the CosmWasm framework, using `cosmwasm_std` for blockchain-specific types like `Addr` and
//! `Uint128`, and `schemars`/`serde` for JSON serialization and schema generation.
//!
//! ## Overview
//!
//! The module includes types for:
//! - **Events**: Representing intent creation events emitted by the blockchain (`IntentCreatedEvent`).
//! - **Intents**: Defining intent structures, including swap intents (`Intent`, `IntentType`, `SwapIntent`).
//! - **Tokens**: Handling input and output tokens for intents (`BeepCoin`, `Token`).
//! - **Messages**: Querying and executing intents (`QueryMsg`, `ExecuteMsg`, and their variants).
//! - **Results**: Tracking transaction outcomes and profit analysis (`TxResult`, `ProfitAnalysis`).
//! - **Utilities**: Supporting event attributes (`EventAttributes`).
//!
//! These types are used in conjunction with an event sourcing and consumption system (e.g., as seen in
//! the `event` module) to process blockchain events and manage intents.
//!
//! ## Key Features
//!
//! - **Serialization**: All types implement `Serialize`, `Deserialize`, and `JsonSchema` for JSON
//!   compatibility and schema generation, enabling interaction with smart contracts and frontends.
//! - **Type Safety**: Uses `cosmwasm_std` types like `Addr` and `Uint128` for blockchain-specific data,
//!   ensuring compatibility with CosmWasm smart contracts.
//! - **Extensibility**: Enums like `IntentType` and message variants allow for future expansion (e.g.,
//!   adding new intent types beyond swaps).
//! - **Profit Analysis**: Includes a `ProfitAnalysis` struct for evaluating the financial outcome of
//!   intent execution, useful for executors assessing profitability.
//!
//! ## Dependencies
//!
//! - `cosmwasm_std`: Provides blockchain-specific types like `Addr` and `Uint128`.
//! - `rust_decimal`: Used for precise decimal arithmetic in `ProfitAnalysis`.
//! - `schemars`: Generates JSON schemas for types, useful for contract interfaces.
//! - `serde`: Handles serialization and deserialization to/from JSON.
//! - `std::collections`: Provides `HashMap` for `EventAttributes`.
//!
//! ## Integration with Intent Event System
//!
//! The `IntentCreatedEvent` and `EventAttributes` types are designed to work with the event sourcing
//! system (e.g., `IntentEventSourcer` from the `event` module). The sourcer parses blockchain
//! events into `IntentCreatedEvent`s, which are then processed by the consumer. Other types like
//! `Intent` and `ExecuteMsg` are used for interacting with the smart contract that manages intents.
use cosmwasm_std::{Addr, Uint128};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents an intent creation event emitted by the blockchain.
///
/// This struct captures metadata about an intent creation event, typically parsed from WebSocket
/// transaction events. It is used by the event sourcing system to track new intents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct IntentCreatedEvent {
    /// Unique identifier for the intent.
    pub intent_id: String,
    /// Status of the intent (e.g., "Active", "Completed"), parsed as a string from event attributes.
    pub status: String,
    /// Address of the entity that created the intent.
    pub sender: String,
    /// Blockchain block height at which the event occurred.
    pub block_height: u64,
}

/// Defines the possible statuses of an intent.
///
/// Used to track the lifecycle of an intent within the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub enum IntentStatus {
    /// The intent is active and available for execution.
    Active,
    /// The intent has been successfully fulfilled.
    Completed,
    /// The intent has expired and can no longer be executed.
    Expired,
}

/// Represents an input token for an intent, typically used by the creator.
///
/// This struct defines a token (native or CW20) and its amount, used as input for intents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct BeepCoin {
    /// Denomination or contract address of the token (e.g., "uusd" for native, contract address for CW20).
    pub token: String,
    /// Amount of the token.
    pub amount: Uint128,
    /// Whether the token is a native token (true) or a CW20 token (false).
    pub is_native: bool,
}

/// Represents an output token for an intent, typically provided by the executor.
///
/// This struct includes an optional target address for token delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Token {
    /// Denomination or contract address of the token.
    pub token: String,
    /// Amount of the token.
    pub amount: Uint128,
    /// Whether the token is a native token (true) or a CW20 token (false).
    pub is_native: bool,
    /// Optional address to receive the token (e.g., for CW20 transfers).
    pub target_address: Option<Addr>,
}

/// Defines a swap intent, specifying desired output tokens.
///
/// Used as part of an `IntentType::Swap` to describe the tokens the creator expects in return.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct SwapIntent {
    /// List of output tokens expected from the swap.
    pub output_tokens: Vec<Token>,
}

/// Specifies the type of intent, currently supporting swaps.
///
/// Uses `serde(tag = "type")` to include a "type" field in JSON serialization for variant identification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(tag = "type")]
pub enum IntentType {
    /// A swap intent, where input tokens are exchanged for specified output tokens.
    Swap(SwapIntent),
}

/// Represents a complete intent, including all relevant details.
///
/// This struct is the core data structure for intents, used for creation, execution, and querying.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Intent {
    /// Unique identifier for the intent.
    pub id: String,
    /// Address of the creator of the intent.
    pub creator: Addr,
    /// List of input tokens provided by the creator.
    pub input_tokens: Vec<BeepCoin>,
    /// Type of intent (e.g., swap).
    pub intent_type: IntentType,
    /// Optional address of the executor who fulfilled the intent.
    pub executor: Option<Addr>,
    /// Current status of the intent.
    pub status: IntentStatus,
    /// Timestamp (Unix seconds) when the intent was created.
    pub created_at: u64,
    /// Timestamp (Unix seconds) after which the intent expires.
    pub timeout: u64,
    /// Tip provided to incentivize execution, in the form of a token.
    pub tip: BeepCoin,
}

/// Wraps an `Intent` for query responses.
///
/// Used to return intent details in response to queries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct IntentResponse {
    /// The intent being returned.
    pub intent: Intent,
}

/// Query message for interacting with the smart contract.
///
/// Uses `flatten` to include the `QueryMsgEnum` fields directly in the JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct QueryMsg {
    /// The specific query to execute.
    #[serde(flatten)]
    pub msg: QueryMsgEnum,
}

/// Enumerates possible query types for the smart contract.
///
/// Uses `snake_case` for JSON field names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsgEnum {
    /// Retrieves details of an intent by its ID.
    GetIntent { intent_id: String },
}

/// Execute message for interacting with the smart contract.
///
/// Uses `flatten` to include the `ExecuteMsgEnum` fields directly in the JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ExecuteMsg {
    /// The specific execution action to perform.
    #[serde(flatten)]
    pub msg: ExecuteMsgEnum,
}

/// Enumerates possible execution actions for the smart contract.
///
/// Uses `snake_case` for JSON field names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsgEnum {
    /// Fulfills an intent by providing the specified output tokens.
    FillIntent {
        /// ID of the intent to fulfill.
        intent_id: String,
        /// Output tokens provided by the executor.
        output_tokens: Vec<Token>,
    },
    /// Increases the allowance for a spender to use a specific amount of tokens.
    IncreaseAllowance {
        /// Address of the spender.
        spender: String,
        /// Amount of tokens to allow.
        amount: Uint128,
    },
}

/// Represents the result of a blockchain transaction.
///
/// Used to track transaction outcomes, such as gas usage and transaction hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct TxResult {
    /// Hash of the transaction.
    pub transaction_hash: String,
    /// Amount of gas used by the transaction.
    pub gas_used: u64,
}

/// Type alias for event attributes parsed from blockchain events.
///
/// Maps attribute keys to their values, used in event parsing (e.g., in `IntentCreatedEvent`).
pub type EventAttributes = HashMap<String, String>;

/// Analyzes the profitability of fulfilling an intent.
///
/// Provides financial metrics for executors to evaluate whether fulfilling an intent is worthwhile.
#[derive(Debug)]
pub struct ProfitAnalysis {
    /// USD value of the input tokens provided by the creator.
    pub input_value_usd: Decimal,
    /// USD value of the output tokens provided by the executor.
    pub output_value_usd: Decimal,
    /// USD value of the tip provided by the creator.
    pub tip_value_usd: Decimal,
    /// Total revenue (output tokens + tip) in USD.
    pub total_revenue_usd: Decimal,
    /// Profit in USD (total revenue - input value).
    pub profit_usd: Decimal,
    /// Profit as a percentage of input value.
    pub profit_percentage: Decimal,
    /// Whether the intent is profitable (profit_usd > 0).
    pub is_profitable: bool,
}
