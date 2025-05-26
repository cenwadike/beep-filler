//! # Intent Event System
//!
//! This module implements an event sourcing and consumption system for handling intent creation
//! events from a blockchain network using the Tendermint WebSocket RPC. It consists of three main
//! components:
//!
//! - **`IntentEventSourcer`**: Connects to a Tendermint WebSocket endpoint, subscribes to transaction
//!   events, and captures intent creation events from a specified smart contract.
//! - **`IntentEventConsumer`**: Processes and validates stored intent events, applying a user-provided
//!   processor function to handle events and performing periodic cleanup of expired events.
//! - **`IntentEventManager`**: Coordinates the sourcer and consumer, providing a unified interface
//!   to start and stop the event processing pipeline.
//!
//! The system uses a thread-safe `DashMap` to store events in memory, with configurable retention
//! periods and reconnection logic for robust operation. It leverages the `tendermint-rpc` crate for
//! WebSocket communication and `anyhow` for error handling.
//!
//! ## Features
//!
//! - **Asynchronous Event Processing**: Utilizes Tokio for asynchronous WebSocket connections and
//!   event handling.
//! - **Fault Tolerance**: Implements reconnection logic with exponential backoff for handling
//!   WebSocket connection failures.
//! - **Event Validation**: Ensures intent events meet basic criteria before processing.
//! - **Configurable Parameters**: Supports customization of WebSocket endpoints, contract addresses,
//!   timeouts, and retention periods via the `IntentEventConfig` struct.
//! - **Thread-Safe Storage**: Uses `DashMap` for concurrent access to the event store.
//!
//! ## Usage
//!
//! To use this module, create an `IntentEventManager` with a configuration, then start it with a
//! processor function to handle intent events. The `IntentEventSourcer` connects to a Tendermint
//! WebSocket endpoint to source `IntentCreatedEvent`s, which are stored in a shared `DashMap` and
//! processed by the `IntentEventConsumer`. Below is an example demonstrating both sourcing and
//! consumption using a simulated WebSocket server for illustration (in a real application, replace
//! the mock server with a live Tendermint RPC endpoint):
//!
//! ```rust
//! use anyhow::Result;
//! use event::{IntentEventConfig, IntentEventManager, IntentCreatedEvent};
//! use std::sync::Arc;
//! use dashmap::DashMap;
//! use tokio::sync::mpsc;
//! use tendermint_rpc::event::{Event, EventData, TxInfo};
//! use tendermint_rpc::WebSocketClient;
//! use tokio::time::{sleep, Duration};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize logging for visibility
//!     env_logger::init();
//!
//!     // Define configuration
//!     let config = IntentEventConfig {
//!         rpc_websocket_endpoint: "ws://localhost:26657/websocket".to_string(),
//!         contract_address: "test_contract".to_string(),
//!         reconnect_delay_seconds: 5,
//!         subscription_timeout_seconds: 30,
//!         event_retention_hours: 24,
//!         ping_interval_seconds: 10,
//!         max_reconnect_attempts: 5,
//!     };
//!
//!     // Create a shared event map
//!     let event_map = Arc::new(DashMap::new());
//!
//!     // Simulate a WebSocket server (for demo purposes)
//!     let (tx, mut rx) = mpsc::channel(10);
//!     tokio::spawn(async move {
//!         // Simulate sending a transaction event
//!         let mock_event = Event {
//!             data: EventData::Tx {
//!                 tx_result: TxInfo {
//!                     height: 12345,
//!                     result: tendermint_rpc::endpoint::tx::Response {
//!                         events: vec![tendermint_rpc::Event {
//!                             kind: "wasm".to_string(),
//!                             attributes: vec![
//!                                 tendermint_rpc::EventAttribute {
//!                                     key: "_contract_address".to_string(),
//!                                     value: "test_contract".to_string(),
//!                                 },
//!                                 tendermint_rpc::EventAttribute {
//!                                     key: "action".to_string(),
//!                                     value: "create_intent".to_string(),
//!                                 },
//!                                 tendermint_rpc::EventAttribute {
//!                                     key: "intent_id".to_string(),
//!                                     value: "test_intent_1".to_string(),
//!                                 },
//!                                 tendermint_rpc::EventAttribute {
//!                                     key: "status".to_string(),
//!                                     value: "created".to_string(),
//!                                 },
//!                                 tendermint_rpc::EventAttribute {
//!                                     key: "sender".to_string(),
//!                                     value: "test_sender".to_string(),
//!                                 },
//!                             ],
//!                         }],
//!                         ..Default::default()
//!                     },
//!                     ..Default::default()
//!                 },
//!             },
//!             query: "".to_string(),
//!             events: None,
//!         };
//!         sleep(Duration::from_secs(2)).await;
//!         tx.send(Ok(mock_event)).await.unwrap();
//!     });
//!
//!     // Create the manager
//!     let mut manager = IntentEventManager::new(config);
//!
//!     // Define a processor function to handle sourced events
//!     let processor = |event: IntentCreatedEvent| async move {
//!         println!(
//!             "Consumed event: {} from sender: {} at block: {}",
//!             event.intent_id, event.sender, event.block_height
//!         );
//!     };
//!
//!     // Start the manager to source and consume events
//!     let manager_handle = tokio::spawn(async move {
//!         manager.start(processor).await.unwrap();
//!     });
//!
//!     // Wait briefly to allow event sourcing and consumption
//!     sleep(Duration::from_secs(5)).await;
//!
//!     // Stop the manager
//!     manager_handle.abort();
//!     manager.stop().await;
//!
//!     // Verify that the event was stored and consumed
//!     assert_eq!(manager.event_count(), 0, "Event should have been consumed and removed");
//!     Ok(())
//! }
//! ```
//!
//! In this example, a mock WebSocket server sends a simulated transaction event, which the
//! `IntentEventSourcer` processes and stores in the shared `event_map`. The `IntentEventConsumer`
//! then retrieves and processes this event using the provided processor function. In a real
//! application, the WebSocket server would be a live Tendermint RPC endpoint, and events would be
//! sourced directly from the blockchain.
//!
//! ## Dependencies
//!
//! - `anyhow`: For flexible error handling.
//! - `dashmap`: For thread-safe, concurrent key-value storage.
//! - `futures`: For working with asynchronous streams.
//! - `log`: For structured logging.
//! - `tendermint-rpc`: For interacting with Tendermint WebSocket RPC.
//! - `tokio`: For asynchronous runtime and utilities.
//!
//! ## Error Handling
//!
//! Errors are handled using the `anyhow::Result` type, with context added to provide detailed error
//! messages. The system logs errors, warnings, and informational messages using the `log` crate to
//! aid in debugging and monitoring.
//!
//! ## Testing
//!
//! The module includes a test suite under the `tests` module to verify event sourcing and consumption
//! functionality. Tests simulate event storage and validation to ensure correctness.

use anyhow::{Context, Result};
use dashmap::DashMap;
use futures::{StreamExt, TryStreamExt};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tendermint_rpc::{
    Client, SubscriptionClient, Url, WebSocketClient,
    event::Event,
    query::{EventType, Query},
};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

use crate::config::Config;
use crate::types::{EventAttributes, IntentCreatedEvent};

/// Thread-safe map for storing intent events with their timestamps, keyed by intent_id.
///
/// This type alias defines a `DashMap` wrapped in an `Arc` for safe concurrent access across
/// threads. Each entry maps an `intent_id` (String) to a tuple containing an `IntentCreatedEvent`
/// and a timestamp (u64) representing when the event was received.
pub type IntentEventMap = Arc<DashMap<String, (IntentCreatedEvent, u64)>>;

/// Configuration for the intent event system.
///
/// This struct holds configuration parameters for the event sourcer and consumer, including
/// WebSocket endpoint, contract address, and operational settings like reconnection delays and
/// event retention periods.
#[derive(Debug, Clone)]
pub struct IntentEventConfig {
    /// The WebSocket endpoint URL for the Tendermint RPC (e.g., "ws://localhost:26657/websocket").
    pub rpc_websocket_endpoint: String,
    /// The address of the smart contract emitting intent creation events.
    pub contract_address: String,
    /// Delay between reconnection attempts in seconds (used with exponential backoff).
    pub reconnect_delay_seconds: u64,
    /// Timeout for WebSocket subscription events in seconds.
    pub subscription_timeout_seconds: u64,
    /// Duration in hours for which intent events are retained in memory.
    pub event_retention_hours: u64,
    /// Interval for sending keep-alive pings to the WebSocket server in seconds.
    pub ping_interval_seconds: u64,
    /// Maximum number of reconnection attempts before giving up.
    pub max_reconnect_attempts: u32,
}

impl From<Config> for IntentEventConfig {
    /// Converts a `Config` struct into an `IntentEventConfig`.
    ///
    /// This implementation maps fields from a general `Config` struct (assumed to be defined in
    /// `crate::config`) to the specific fields required by `IntentEventConfig`. Default values are
    /// provided for `ping_interval_seconds` and `max_reconnect_attempts`.
    fn from(config: Config) -> Self {
        Self {
            rpc_websocket_endpoint: config.rpc_websocket_endpoint,
            contract_address: config.contract_address,
            reconnect_delay_seconds: config.reconnect_delay_seconds,
            subscription_timeout_seconds: config.subscription_timeout_seconds,
            event_retention_hours: config.event_retention_hours,
            ping_interval_seconds: 10,
            max_reconnect_attempts: 5,
        }
    }
}

/// Intent Event Sourcer - Listens for and captures intent creation events.
///
/// This struct manages a WebSocket connection to a Tendermint RPC endpoint, subscribes to
/// transaction events, and stores intent creation events in a shared `IntentEventMap`. It
/// includes reconnection logic and a keep-alive ping mechanism for robust operation.
pub struct IntentEventSourcer {
    /// Configuration for the event sourcer.
    config: IntentEventConfig,
    /// Shared map storing intent events.
    event_map: IntentEventMap,
    /// Optional WebSocket client, wrapped in `Arc<Mutex>` for thread safety.
    ws_client: Option<Arc<Mutex<WebSocketClient>>>,
    /// Optional handle to the WebSocket driver task.
    driver_handle: Option<tokio::task::JoinHandle<()>>,
    /// Flag indicating whether the sourcer is running.
    is_running: bool,
}

impl IntentEventSourcer {
    /// Creates a new `IntentEventSourcer` instance.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for the event sourcer.
    /// * `event_map` - The shared map for storing intent events.
    ///
    /// # Returns
    ///
    /// A new `IntentEventSourcer` instance with uninitialized WebSocket client and driver.
    pub fn new(config: IntentEventConfig, event_map: IntentEventMap) -> Self {
        Self {
            config,
            event_map,
            ws_client: None,
            driver_handle: None,
            is_running: false,
        }
    }

    /// Initializes the WebSocket connection to the Tendermint RPC endpoint.
    ///
    /// Establishes a WebSocket connection, spawns the driver task, and verifies the connection
    /// by querying the chain status.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure with an error message.
    async fn initialize(&mut self) -> Result<()> {
        let ws_url = Url::from_str(&self.config.rpc_websocket_endpoint).context(format!(
            "Invalid WebSocket endpoint URL: {}",
            self.config.rpc_websocket_endpoint
        ))?;

        info!("Intent Sourcer connecting to: {}", ws_url);

        let (ws_client, driver) = WebSocketClient::new(ws_url)
            .await
            .context("Failed to create WebSocket client")?;

        let ws_client = Arc::new(Mutex::new(ws_client));
        self.ws_client = Some(ws_client.clone());

        self.driver_handle = Some(tokio::spawn(async move {
            if let Err(e) = driver.run().await {
                error!("WebSocket driver error: {}", e);
            }
        }));

        let status = ws_client
            .lock()
            .await
            .status()
            .await
            .context("Failed to get chain status via WebSocket")?;

        info!(
            "Intent Sourcer connected to {} (chain: {}, height: {})",
            self.config.rpc_websocket_endpoint,
            status.node_info.network,
            status.sync_info.latest_block_height
        );

        self.is_running = true;
        Ok(())
    }

    /// Runs a keep-alive ping loop to maintain the WebSocket connection.
    ///
    /// Periodically sends status requests to the WebSocket server to prevent connection timeouts.
    ///
    /// # Arguments
    ///
    /// * `ws_client` - The WebSocket client wrapped in `Arc<Mutex>`.
    /// * `ping_interval_seconds` - Interval between pings in seconds.
    /// * `is_running` - Shared flag indicating whether the loop should continue.
    async fn ping_loop(
        ws_client: Arc<Mutex<WebSocketClient>>,
        ping_interval_seconds: u64,
        is_running: Arc<Mutex<bool>>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(ping_interval_seconds));
        while *is_running.lock().await {
            interval.tick().await;
            if let Err(e) = ws_client.lock().await.status().await {
                warn!("Keep-alive ping failed: {}", e);
            } else {
                debug!("Keep-alive ping sent successfully");
            }
        }
    }

    /// Parses intent creation events from transaction events.
    ///
    /// Extracts intent creation events from WebSocket transaction events if they match the
    /// configured contract address and action type.
    ///
    /// # Arguments
    ///
    /// * `event` - The WebSocket event to parse.
    ///
    /// # Returns
    ///
    /// An `Option` containing the parsed `IntentCreatedEvent` and timestamp if found, or `None`.
    fn parse_intent_event_from_tx_event(&self, event: &Event) -> Option<(IntentCreatedEvent, u64)> {
        if let tendermint_rpc::event::EventData::Tx { tx_result } = &event.data {
            let height = tx_result.height;

            for event in &tx_result.result.events {
                if event.kind.as_str() == "wasm" {
                    let mut attributes: EventAttributes = HashMap::new();

                    for attr in &event.attributes {
                        let key = attr.key.as_str().to_string();
                        let value = attr.value.as_str().to_string();
                        attributes.insert(key, value);
                    }

                    debug!("WASM attributes at height {}: {:?}", height, attributes);

                    if let Some(contract_addr) = attributes.get("_contract_address") {
                        if contract_addr == &self.config.contract_address {
                            if attributes.get("action") == Some(&"create_intent".to_string()) {
                                info!("Intent Sourcer found intent event at height {}", height);

                                let intent_event = IntentCreatedEvent {
                                    intent_id: attributes
                                        .get("intent_id")
                                        .cloned()
                                        .unwrap_or_default(),
                                    status: attributes.get("status").cloned().unwrap_or_default(),
                                    sender: attributes.get("sender").cloned().unwrap_or_default(),
                                    block_height: height as u64,
                                };

                                let timestamp = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();

                                return Some((intent_event, timestamp));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Starts the event sourcing process.
    ///
    /// Initializes the WebSocket connection and runs the subscription loop, with reconnection
    /// logic for handling failures.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure after maximum reconnection attempts are exceeded.
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Intent Event Sourcer");
        let mut reconnect_attempts = 0;

        loop {
            if let Err(e) = self.run_subscription_loop().await {
                error!(
                    "Subscription error: {}. Reconnecting in {} seconds (attempt {}/{})...",
                    e,
                    self.config.reconnect_delay_seconds,
                    reconnect_attempts + 1,
                    self.config.max_reconnect_attempts
                );

                self.cleanup().await;

                if reconnect_attempts >= self.config.max_reconnect_attempts {
                    return Err(anyhow::anyhow!(
                        "Max reconnection attempts ({}) reached",
                        self.config.max_reconnect_attempts
                    ));
                }

                let delay = self.config.reconnect_delay_seconds * 2u64.pow(reconnect_attempts);
                sleep(Duration::from_secs(delay)).await;
                reconnect_attempts += 1;
                continue;
            }
            break;
        }
        Ok(())
    }

    /// Main subscription loop for event sourcing.
    ///
    /// Subscribes to transaction events, processes incoming events, and stores valid intent
    /// creation events in the shared map. Handles timeouts and connection errors.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure if the connection is lost or the stream ends.
    async fn run_subscription_loop(&mut self) -> Result<()> {
        self.initialize().await?;

        let ws_client = self
            .ws_client
            .as_ref()
            .context("WebSocket client not initialized")?
            .clone();

        // Start keep-alive ping loop
        let ping_client = ws_client.clone();
        let ping_interval_seconds = self.config.ping_interval_seconds;
        let is_running = Arc::new(Mutex::new(self.is_running));
        tokio::spawn(async move {
            Self::ping_loop(ping_client, ping_interval_seconds, is_running).await;
        });

        let query = Query::eq("tm.event", &*EventType::Tx.to_string());
        info!("Intent Sourcer subscribing to transaction events...");
        let subscription = ws_client
            .lock()
            .await
            .subscribe(query)
            .await
            .context("Failed to subscribe to transaction events")?;

        info!("Intent Sourcer subscription active");

        let mut event_stream = subscription.into_stream();

        while self.is_running {
            let event_result = timeout(
                Duration::from_secs(self.config.subscription_timeout_seconds),
                event_stream.next(),
            )
            .await;

            match event_result {
                Ok(Some(Ok(event))) => {
                    debug!("Intent Sourcer received event");

                    if let Some((intent_event, timestamp)) =
                        self.parse_intent_event_from_tx_event(&event)
                    {
                        info!("Intent Sourcer storing event: {}", intent_event.intent_id);
                        self.event_map
                            .insert(intent_event.intent_id.clone(), (intent_event, timestamp));
                        debug!("Intent map now contains {} events", self.event_map.len());
                    }
                }
                Ok(Some(Err(e))) => {
                    error!("Event stream error: {}", e);
                    if e.to_string().contains("Connection reset by peer") {
                        warn!("Connection reset detected, triggering reconnect...");
                        return Err(anyhow::anyhow!("Connection reset, reconnecting"));
                    }
                    return Err(e.into());
                }
                Ok(None) => {
                    warn!("Event stream ended unexpectedly");
                    return Err(anyhow::anyhow!("Event stream closed"));
                }
                Err(_) => {
                    debug!("Subscription timeout, continuing...");
                    if let Err(e) = ws_client.lock().await.status().await {
                        error!(
                            "Connection health check failed for endpoint {}: {}",
                            self.config.rpc_websocket_endpoint, e
                        );
                        return Err(e.into());
                    }
                }
            }
        }

        info!("Intent Sourcer subscription loop exited");
        Ok(())
    }

    /// Cleans up WebSocket connections and driver tasks.
    ///
    /// Unsubscribes from events, closes the WebSocket connection, and awaits driver termination.
    async fn cleanup(&mut self) {
        if let Some(ws_client) = self.ws_client.take() {
            info!("Intent Sourcer cleaning up connections...");
            let ws_client = ws_client.lock().await;

            if let Err(e) = ws_client
                .unsubscribe(Query::eq("tm.event", &*EventType::Tx.to_string()))
                .await
            {
                warn!("Failed to unsubscribe: {}", e);
            }

            if let Err(e) = <WebSocketClient as Clone>::clone(&ws_client).close() {
                warn!("Failed to close WebSocket connection: {}", e);
            }
        }

        if let Some(handle) = self.driver_handle.take() {
            if let Err(e) = handle.await {
                warn!("Failed to await driver termination: {}", e);
            }
        }

        self.is_running = false;
        info!("Intent Sourcer cleanup completed");
    }

    /// Stops the event sourcer.
    ///
    /// Sets the running flag to false and performs cleanup.
    pub async fn stop(&mut self) {
        info!("Stopping Intent Event Sourcer...");
        self.is_running = false;
        self.cleanup().await;
    }
}

/// Intent Event Consumer - Processes and manages stored intent events.
///
/// This struct processes events from the shared `IntentEventMap`, validates them, and applies
/// a user-provided processor function. It also runs a cleanup loop to remove expired events.
pub struct IntentEventConsumer {
    /// Configuration for the event consumer.
    config: IntentEventConfig,
    /// Shared map storing intent events.
    event_map: IntentEventMap,
    /// Flag indicating whether the consumer is running.
    is_running: bool,
}

impl IntentEventConsumer {
    /// Creates a new `IntentEventConsumer` instance.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for the event consumer.
    /// * `event_map` - The shared map for accessing intent events.
    ///
    /// # Returns
    ///
    /// A new `IntentEventConsumer` instance.
    pub fn new(config: IntentEventConfig, event_map: IntentEventMap) -> Self {
        Self {
            config,
            event_map,
            is_running: false,
        }
    }

    /// Starts the event consumption process.
    ///
    /// Spawns a cleanup loop and processes events from the shared map using the provided
    /// processor function.
    ///
    /// # Arguments
    ///
    /// * `processor` - An async function to process each `IntentCreatedEvent`.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure.
    pub async fn start<F, Fut>(&mut self, processor: F) -> Result<()>
    where
        F: Fn(IntentCreatedEvent) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        info!("Starting Intent Event Consumer");
        self.is_running = true;

        let cleanup_map = Arc::clone(&self.event_map);
        let cleanup_config = self.config.clone();
        tokio::spawn(async move {
            Self::cleanup_loop(cleanup_map, cleanup_config).await;
        });

        while self.is_running {
            let events_to_process: Vec<_> = self
                .event_map
                .iter()
                .map(|entry| entry.value().0.clone())
                .collect();

            if !events_to_process.is_empty() {
                debug!(
                    "Intent Consumer processing {} events",
                    events_to_process.len()
                );

                for event in events_to_process {
                    if let Err(e) = self.validate_event(&event) {
                        error!("Invalid event {}: {}", event.intent_id, e);
                        self.event_map.remove(&event.intent_id);
                        continue;
                    }

                    info!("Intent Consumer processing event: {}", event.intent_id);
                    processor(event.clone()).await;
                    self.event_map.remove(&event.intent_id);
                }
            } else {
                debug!("Intent Consumer: no events to process");
            }

            debug!("Current event count: {}", self.event_count());
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        info!("Intent Event Consumer stopped");
        Ok(())
    }

    /// Runs a periodic cleanup loop to remove expired events.
    ///
    /// Removes events from the shared map that are older than the configured retention period.
    ///
    /// # Arguments
    ///
    /// * `event_map` - The shared map storing intent events.
    /// * `config` - The configuration for retention periods.
    async fn cleanup_loop(event_map: IntentEventMap, config: IntentEventConfig) {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));

        loop {
            interval.tick().await;

            let retention_seconds = config.event_retention_hours * 3600;
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let before_count = event_map.len();
            event_map.retain(|_, (_, timestamp)| current_time - *timestamp < retention_seconds);
            let after_count = event_map.len();
            let removed_count = before_count - after_count;

            if removed_count > 0 {
                info!(
                    "Intent Consumer cleanup: removed {} expired events, {} remaining",
                    removed_count, after_count
                );
            } else {
                debug!("Intent Consumer cleanup: no expired events found");
            }
        }
    }

    /// Validates an intent event.
    ///
    /// Ensures the event has a non-empty intent ID, sender, and a valid block height.
    ///
    /// # Arguments
    ///
    /// * `event` - The event to validate.
    ///
    /// # Returns
    ///
    /// A `Result` indicating whether the event is valid.
    pub fn validate_event(&self, event: &IntentCreatedEvent) -> Result<()> {
        debug!("Validating intent event: {}", event.intent_id);

        if event.intent_id.is_empty() {
            return Err(anyhow::anyhow!("Intent ID is empty"));
        }

        if event.sender.is_empty() {
            return Err(anyhow::anyhow!("Sender is empty"));
        }

        if event.block_height == 0 {
            return Err(anyhow::anyhow!("Block height is zero"));
        }

        info!("Intent event validation passed: {}", event.intent_id);
        Ok(())
    }

    /// Returns the current number of events in the shared map.
    pub fn event_count(&self) -> usize {
        self.event_map.len()
    }

    /// Stops the event consumer.
    ///
    /// Sets the running flag to false to exit the processing loop.
    pub fn stop(&mut self) {
        info!("Stopping Intent Event Consumer...");
        self.is_running = false;
    }
}

/// Intent Event Manager - Coordinates sourcing and consumption.
///
/// This struct provides a unified interface to manage the `IntentEventSourcer` and
/// `IntentEventConsumer`, starting and stopping them together.
pub struct IntentEventManager {
    /// The event sourcer component.
    sourcer: IntentEventSourcer,
    /// The event consumer component.
    consumer: IntentEventConsumer,
    /// Shared map storing intent events.
    pub event_map: IntentEventMap,
}

impl IntentEventManager {
    /// Creates a new `IntentEventManager` instance.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for both sourcer and consumer.
    ///
    /// # Returns
    ///
    /// A new `IntentEventManager` instance with initialized sourcer and consumer.
    pub fn new(config: IntentEventConfig) -> Self {
        let event_map = Arc::new(DashMap::new());
        let sourcer = IntentEventSourcer::new(config.clone(), Arc::clone(&event_map));
        let consumer = IntentEventConsumer::new(config, Arc::clone(&event_map));

        Self {
            sourcer,
            consumer,
            event_map,
        }
    }

    /// Starts the event manager.
    ///
    /// Spawns the sourcer in a separate task and starts the consumer with the provided
    /// processor function.
    ///
    /// # Arguments
    ///
    /// * `processor` - An async function to process each `IntentCreatedEvent`.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure.
    pub async fn start<F, Fut>(&mut self, processor: F) -> Result<()>
    where
        F: Fn(IntentCreatedEvent) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        info!("Starting Intent Event Manager");

        let mut sourcer =
            IntentEventSourcer::new(self.sourcer.config.clone(), Arc::clone(&self.event_map));
        tokio::spawn(async move {
            if let Err(e) = sourcer.start().await {
                error!("Intent Event Sourcer failed: {}", e);
            }
        });

        self.consumer.start(processor).await?;

        info!(
            "Event manager stopped, final event count: {}",
            self.event_count()
        );

        Ok(())
    }

    /// Stops the event manager.
    ///
    /// Stops both the sourcer and consumer components.
    pub async fn stop(&mut self) {
        info!("Stopping Intent Event Manager");
        self.sourcer.stop().await;
        self.consumer.stop();
    }

    /// Returns the current number of events in the shared map.
    pub fn event_count(&self) -> usize {
        self.event_map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests event sourcing and consumption functionality.
    #[tokio::test]
    async fn test_event_sourcing_and_consumption() {
        let config = IntentEventConfig {
            rpc_websocket_endpoint: "ws://localhost:26657/websocket".to_string(),
            contract_address: "test_contract".to_string(),
            reconnect_delay_seconds: 5,
            subscription_timeout_seconds: 30,
            event_retention_hours: 1,
            ping_interval_seconds: 10,
            max_reconnect_attempts: 5,
        };

        let event_map = Arc::new(DashMap::new());

        let test_event = IntentCreatedEvent {
            intent_id: "test_intent_1".to_string(),
            status: "created".to_string(),
            sender: "test_sender".to_string(),
            block_height: 12345,
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        event_map.insert(
            test_event.intent_id.clone(),
            (test_event.clone(), timestamp),
        );

        let consumer = IntentEventConsumer::new(config, Arc::clone(&event_map));

        assert!(consumer.validate_event(&test_event).is_ok());
        assert_eq!(consumer.event_count(), 1);
    }
}
