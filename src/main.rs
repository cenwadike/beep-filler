use anyhow::{Context, Result};
use clap::Parser;
use dotenv::dotenv;
use log::{error, info, warn};
use std::env;
use std::sync::Arc;
use tokio::signal;

mod config;
mod event;
mod executor;
mod types;

use config::Config;
use event::{IntentEventManager, IntentEventMap};
use executor::IntentExecutor;
use types::IntentCreatedEvent;

#[derive(Parser)]
#[command(name = "beep-filler")]
#[command(about = "An intent filler for beep")]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "config.json")]
    config: String,

    /// RPC HTTP endpoint override
    #[arg(long)]
    rpc_http_endpoint: Option<String>,

    /// RPC WebSocket endpoint override
    #[arg(long)]
    rpc_websocket_endpoint: Option<String>,

    /// Contract address override
    #[arg(long)]
    contract_address: Option<String>,

    /// Chain ID override
    #[arg(long)]
    chain_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    // Initialize logger
    env_logger::init();

    let cli = Cli::parse();

    // Load configuration
    let mut config = Config::from_file(&cli.config).unwrap_or_else(|e| {
        warn!("Could not load config file: {}. Using defaults.", e);
        Config::default()
    });

    // Override config with CLI arguments
    if let Some(rpc_http_endpoint) = cli.rpc_http_endpoint {
        config.rpc_http_endpoint = rpc_http_endpoint;
    }
    if let Some(rpc_websocket_endpoint) = cli.rpc_websocket_endpoint {
        config.rpc_websocket_endpoint = rpc_websocket_endpoint;
    }
    if let Some(contract_address) = cli.contract_address {
        config.contract_address = contract_address;
    }
    if let Some(chain_id) = cli.chain_id {
        config.chain_id = chain_id;
    }

    // Validate config
    config.validate().context("Invalid configuration")?;

    // Sanitize config for logging (avoid exposing mnemonic_file path)
    let config_log = format!(
        "Config {{ rpc_http_endpoint: {}, rpc_websocket_endpoint: {}, contract_address: {}, \
         reconnect_delay_seconds: {}, subscription_timeout_seconds: {}, event_retention_hours: {}, \
         default_timeout_height: {}, chain_id: {}, fee_denom: {}, gas_limit: {} }}",
        config.rpc_http_endpoint,
        config.rpc_websocket_endpoint,
        config.contract_address,
        config.reconnect_delay_seconds,
        config.subscription_timeout_seconds,
        config.event_retention_hours,
        config.default_timeout_height,
        config.chain_id,
        config.fee_denom,
        config.gas_limit
    );
    info!("Starting filler with config: {}", config_log);

    // Load mnemonic from environment or config
    let mnemonic = env::var("MNEMONIC")
        .ok()
        .or_else(|| config.load_mnemonic().ok().flatten())
        .context("Mnemonic not provided in MNEMONIC env var or config.mnemonic_file")?;

    // Initialize filler
    let intent_executor = IntentExecutor::new(config.clone(), mnemonic)
        .await
        .context("Failed to initialize IntentExecutor")?;

    // Initialize event manager
    let mut event_manager = IntentEventManager::new(config.clone().into());

    // Get event map for conditional event removal
    let event_map: IntentEventMap = Arc::clone(&event_manager.event_map);

    // Handle new intent events
    let handle_new_intent = move |event: IntentCreatedEvent| {
        let executor = intent_executor.clone();
        let event_map = Arc::clone(&event_map);
        async move {
            match executor.handle_intent_event(event.clone()).await {
                Ok(()) => {
                    event_map.remove(&event.intent_id);
                    info!(
                        "Successfully processed and removed event: {}",
                        event.intent_id
                    );
                }
                Err(e) => {
                    error!("Failed to handle intent event {}: {}", event.intent_id, e);
                }
            }
        }
    };

    // Start event manager
    let mut task_event_manager = IntentEventManager::new(config.into());
    let manager_handle = tokio::spawn(async move {
        if let Err(e) = task_event_manager.start(handle_new_intent).await {
            error!("Event manager failed: {}", e);
        }
    });

    // Wait for shutdown signal
    info!("filler started. Press Ctrl+C to stop.");
    signal::ctrl_c()
        .await
        .context("Failed to wait for shutdown signal")?;

    info!("Shutdown signal received. Cleaning up...");

    // Stop the event manager gracefully
    event_manager.stop().await;

    // Wait for the manager task to complete
    if let Err(e) = manager_handle.await {
        warn!("Event manager task did not shut down cleanly: {}", e);
    }

    info!("Shutdown complete.");
    Ok(())
}
