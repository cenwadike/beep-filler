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
use tokio::time::{sleep, timeout};

use crate::config::Config;
use crate::types::{EventAttributes, IntentCreatedEvent};

/// Thread-safe map for storing intent events with their timestamps, keyed by intent_id
pub type IntentEventMap = Arc<DashMap<String, (IntentCreatedEvent, u64)>>;

/// Configuration for the intent event system
#[derive(Debug, Clone)]
pub struct IntentEventConfig {
    pub rpc_websocket_endpoint: String,
    pub contract_address: String,
    pub reconnect_delay_seconds: u64,
    pub subscription_timeout_seconds: u64,
    pub event_retention_hours: u64,
}

impl From<Config> for IntentEventConfig {
    fn from(config: Config) -> Self {
        Self {
            rpc_websocket_endpoint: config.rpc_websocket_endpoint,
            contract_address: config.contract_address,
            reconnect_delay_seconds: config.reconnect_delay_seconds,
            subscription_timeout_seconds: config.subscription_timeout_seconds,
            event_retention_hours: config.event_retention_hours,
        }
    }
}

/// Intent Event Sourcer - Listens for and captures intent creation events
pub struct IntentEventSourcer {
    config: IntentEventConfig,
    event_map: IntentEventMap,
    ws_client: Option<WebSocketClient>,
    is_running: bool,
}

impl IntentEventSourcer {
    /// Creates a new IntentEventSourcer instance
    pub fn new(config: IntentEventConfig, event_map: IntentEventMap) -> Self {
        Self {
            config,
            event_map,
            ws_client: None,
            is_running: false,
        }
    }

    /// Initializes the WebSocket connection
    async fn initialize(&mut self) -> Result<()> {
        let ws_url = Url::from_str(&self.config.rpc_websocket_endpoint)
            .context("Invalid WebSocket endpoint URL")?;

        info!(
            "Intent Sourcer connecting to: {}",
            self.config.rpc_websocket_endpoint
        );

        let (ws_client, driver) = WebSocketClient::new(ws_url)
            .await
            .context("Failed to create WebSocket client")?;

        // Spawn the driver to handle the WebSocket connection
        tokio::spawn(async move {
            if let Err(e) = driver.run().await {
                error!("WebSocket driver error: {}", e);
            }
        });

        // Test the connection by getting status
        let status = ws_client
            .status()
            .await
            .context("Failed to get chain status via WebSocket")?;

        info!(
            "Intent Sourcer connected to {} (chain: {}, height: {})",
            self.config.rpc_websocket_endpoint,
            status.node_info.network,
            status.sync_info.latest_block_height
        );

        self.ws_client = Some(ws_client);
        self.is_running = true;

        Ok(())
    }

    /// Parses intent creation events from transaction events
    fn parse_intent_event_from_tx_event(&self, event: &Event) -> Option<(IntentCreatedEvent, u64)> {
        if let tendermint_rpc::event::EventData::Tx { tx_result } = &event.data {
            let height = tx_result.height;

            // Look for wasm events in the transaction events
            for event in &tx_result.result.events {
                if event.kind.as_str() == "wasm" {
                    let mut attributes: EventAttributes = HashMap::new();

                    for attr in &event.attributes {
                        let key = attr.key.as_str().to_string();
                        let value = attr.value.as_str().to_string();
                        attributes.insert(key.to_string(), value.to_string());
                    }

                    debug!("WASM attributes at height {}: {:?}", height, attributes);

                    // Check if this is our contract and has create_intent action
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

    /// Starts the event sourcing process
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Intent Event Sourcer");

        loop {
            if let Err(e) = self.run_subscription_loop().await {
                error!(
                    "Subscription error: {}. Reconnecting in {} seconds...",
                    e, self.config.reconnect_delay_seconds
                );
                self.cleanup().await;
                sleep(Duration::from_secs(self.config.reconnect_delay_seconds)).await;
                continue;
            }
            break;
        }
        Ok(())
    }

    /// Main subscription loop for event sourcing
    async fn run_subscription_loop(&mut self) -> Result<()> {
        self.initialize().await?;

        let ws_client = self
            .ws_client
            .as_ref()
            .context("WebSocket client not initialized")?;

        // Subscribe to transaction events
        let query = Query::eq("tm.event", &*EventType::Tx.to_string());

        info!("Intent Sourcer subscribing to transaction events...");
        let subscription = ws_client
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

                        // Store the event and timestamp in the shared map
                        self.event_map
                            .insert(intent_event.intent_id.clone(), (intent_event, timestamp));

                        debug!("Intent map now contains {} events", self.event_map.len());
                    }
                }
                Ok(Some(Err(e))) => {
                    error!("Event stream error: {}", e);
                    return Err(e.into());
                }
                Ok(None) => {
                    warn!("Event stream ended unexpectedly");
                    return Err(anyhow::anyhow!("Event stream closed"));
                }
                Err(_) => {
                    // Timeout - this is normal, just continue
                    debug!("Subscription timeout, continuing...");

                    // Ping the connection to make sure it's still alive
                    if let Err(e) = ws_client.status().await {
                        error!("Connection health check failed: {}", e);
                        return Err(e.into());
                    }
                }
            }
        }

        info!("Intent Sourcer subscription loop exited");
        Ok(())
    }

    /// Cleanup WebSocket connections
    async fn cleanup(&mut self) {
        if let Some(ws_client) = &self.ws_client {
            info!("Intent Sourcer cleaning up connections...");

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

        self.ws_client = None;
        self.is_running = false;
        info!("Intent Sourcer cleanup completed");
    }

    /// Stops the event sourcer
    pub async fn stop(&mut self) {
        info!("Stopping Intent Event Sourcer...");
        self.is_running = false;
        self.cleanup().await;
    }
}

/// Intent Event Consumer - Processes and manages stored intent events
pub struct IntentEventConsumer {
    config: IntentEventConfig,
    event_map: IntentEventMap,
    is_running: bool,
}

impl IntentEventConsumer {
    /// Creates a new IntentEventConsumer instance
    pub fn new(config: IntentEventConfig, event_map: IntentEventMap) -> Self {
        Self {
            config,
            event_map,
            is_running: false,
        }
    }

    /// Starts the event consumer
    pub async fn start<F, Fut>(&mut self, processor: F) -> Result<()>
    where
        F: Fn(IntentCreatedEvent) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        info!("Starting Intent Event Consumer");
        self.is_running = true;

        // Spawn cleanup task
        let cleanup_map = Arc::clone(&self.event_map);
        let cleanup_config = self.config.clone();
        tokio::spawn(async move {
            Self::cleanup_loop(cleanup_map, cleanup_config).await;
        });

        // Main processing loop
        while self.is_running {
            let events_to_process: Vec<_> = self
                .event_map
                .iter()
                .map(|entry| entry.value().0.clone()) // Extract IntentCreatedEvent from tuple
                .collect();

            if !events_to_process.is_empty() {
                debug!(
                    "Intent Consumer processing {} events",
                    events_to_process.len()
                );

                for event in events_to_process {
                    // Validate event before processing
                    if let Err(e) = self.validate_event(&event) {
                        error!("Invalid event {}: {}", event.intent_id, e);
                        self.event_map.remove(&event.intent_id);
                        continue;
                    }

                    info!("Intent Consumer processing event: {}", event.intent_id);

                    // Process the event with the provided processor
                    processor(event.clone()).await;

                    // Remove processed event from map
                    self.event_map.remove(&event.intent_id);
                }
            } else {
                debug!("Intent Consumer: no events to process");
            }

            // Log current event count
            debug!("Current event count: {}", self.event_count());

            // Sleep to prevent tight loop
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        info!("Intent Event Consumer stopped");
        Ok(())
    }

    /// Background cleanup loop for expired events
    async fn cleanup_loop(event_map: IntentEventMap, config: IntentEventConfig) {
        let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Cleanup every hour

        loop {
            interval.tick().await;

            let retention_seconds = config.event_retention_hours * 3600;
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let before_count = event_map.len();

            // Remove expired events
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

    /// Validates an intent event (can be extended with custom validation logic)
    pub fn validate_event(&self, event: &IntentCreatedEvent) -> Result<()> {
        debug!("Validating intent event: {}", event.intent_id);

        // Basic validation
        if event.intent_id.is_empty() {
            return Err(anyhow::anyhow!("Intent ID is empty"));
        }

        if event.sender.is_empty() {
            return Err(anyhow::anyhow!("Sender is empty"));
        }

        if event.block_height == 0 {
            return Err(anyhow::anyhow!("Block height is zero"));
        }

        // Add custom validation logic here
        info!("Intent event validation passed: {}", event.intent_id);
        Ok(())
    }

    /// Gets event count
    pub fn event_count(&self) -> usize {
        self.event_map.len()
    }

    /// Stops the event consumer
    pub fn stop(&mut self) {
        info!("Stopping Intent Event Consumer...");
        self.is_running = false;
    }
}

/// Intent Event Manager - Coordinates sourcing and consumption
pub struct IntentEventManager {
    sourcer: IntentEventSourcer,
    consumer: IntentEventConsumer,
    pub event_map: IntentEventMap,
}

impl IntentEventManager {
    /// Creates a new IntentEventManager
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

    /// Starts both sourcer and consumer
    pub async fn start<F, Fut>(&mut self, processor: F) -> Result<()>
    where
        F: Fn(IntentCreatedEvent) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        info!("Starting Intent Event Manager");

        // Start sourcer in background
        let mut sourcer =
            IntentEventSourcer::new(self.sourcer.config.clone(), Arc::clone(&self.event_map));
        tokio::spawn(async move {
            if let Err(e) = sourcer.start().await {
                error!("Intent Event Sourcer failed: {}", e);
            }
        });

        // Start consumer (blocks)
        self.consumer.start(processor).await?;

        // Log final event count
        info!("Event manager stopped, final event count: {}", self.event_count());

        Ok(())
    }

    /// Stops both components
    pub async fn stop(&mut self) {
        info!("Stopping Intent Event Manager");
        self.sourcer.stop().await;
        self.consumer.stop();
    }

    /// Gets current event count
    pub fn event_count(&self) -> usize {
        self.event_map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_sourcing_and_consumption() {
        let config = IntentEventConfig {
            rpc_websocket_endpoint: "ws://localhost:26657/websocket".to_string(),
            contract_address: "test_contract".to_string(),
            reconnect_delay_seconds: 5,
            subscription_timeout_seconds: 30,
            event_retention_hours: 1,
        };

        let event_map = Arc::new(DashMap::new());

        // Add a test event
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

        // Test validation
        assert!(consumer.validate_event(&test_event).is_ok());

        // Test count
        assert_eq!(consumer.event_count(), 1);
    }
}