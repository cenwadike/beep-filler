//! # Beep Intent Filler
//!
//! This module implements the main entry point for the `beep-filler` application, an intent filler for
//! a blockchain-based intent system, likely built for a Cosmos-based smart contract platform. It processes
//! intent events (e.g., token swaps) by leveraging price and forex providers to fetch real-time market
//! data, enabling profitability analysis and transaction execution.
//!
//! ## Overview
//!
//! The `beep-filler` binary initializes an `IntentExecutor` to handle intent events, an
//! `IntentEventManager` to monitor blockchain events, and an interactive CLI for runtime configuration.
//! It integrates with the `price_providers` and `forex_providers` modules to fetch token prices (e.g.,
//! ATOM, NGN) and exchange rates (e.g., USD/EUR), respectively, for financial calculations.
//!
//! Key components:
//! - **`IntentExecutor`**: Manages intent processing, price/forex provider integration, and cache management.
//! - **`IntentEventManager`**: Subscribes to blockchain events (e.g., `IntentCreatedEvent`) and dispatches them for processing.
//! - **Interactive CLI**: Allows runtime commands to add/remove providers, fetch rates, manage cache, and configure settings.
//! - **`Config`**: Loads configuration from a JSON file or environment variables, with overrides via command-line arguments.
//!
//! ## Key Features
//!
//! - **Asynchronous Operation**: Uses `tokio` for non-blocking event handling, HTTP requests, and CLI input.
//! - **Configurability**: Supports configuration via JSON file, environment variables, and command-line overrides.
//! - **Extensibility**: Integrates pluggable price and forex providers via trait-based design.
//! - **Error Handling**: Uses `anyhow::Result` for robust error propagation with contextual messages.
//! - **Logging**: Employs `log` crate for detailed info, warning, and error logs.
//! - **Thread Safety**: Uses `Arc<Mutex<>>` for shared access to `IntentExecutor` and `IntentEventManager`.
//! - **Graceful Shutdown**: Handles Ctrl+C and CLI `exit` commands, ensuring cleanup of caches and event subscriptions.
//!
//! ## Usage
//!
//! Run the binary with optional configuration overrides:
//!
//! ```bash
//! # Run with default config
//! cargo run --bin beep-filler
//!
//! # Override configuration
//! cargo run --bin beep-filler -- --config custom_config.json --rpc-http-endpoint http://localhost:26657
//! ```
//!
//! Once running, the interactive CLI accepts commands like:
//!
//! ```text
//! > add-price-provider ngn_price_provider
//! > list-forex-providers
//! > get-multiple-exchange-rates USD/NGN,EUR/NGN
//! > exit
//! ```
//!
//! Type `help` in the CLI for a full list of commands.
//!
//! ## Dependencies
//!
//! - `anyhow`: For error handling with contextual information.
//! - `clap`: For parsing command-line arguments.
//! - `dotenv`: For loading environment variables from a `.env` file.
//! - `env_logger`: For initializing logging with environment-based configuration.
//! - `tokio`: For asynchronous runtime, I/O, and signal handling.
//! - `log`: For logging info, warnings, and errors.
//! - `std`: For environment variables and synchronization primitives (`Arc`, `Mutex`).
//! - Custom modules: `config`, `event`, `executor`, `forex_providers`, `price_providers`, `types`.
//!
//! ## Notes
//!
//! - **Mnemonic Requirement**: A mnemonic phrase must be provided via the `MNEMONIC` environment variable or a file specified in the config for blockchain interactions.
//! - **API Dependencies**: Relies on external APIs (e.g., `exchangerate-api.com`, `currencylayer.com`, `coingecko.com`) for price and forex data, which may have rate limits. Consider caching in production.
//! - **Configuration**: The config file (`config.json` by default) is optional; defaults are used if loading fails.
//! - **Shutdown**: The application supports graceful shutdown via Ctrl+C or the `exit` CLI command, clearing caches and stopping event subscriptions.
//! - **Thread Safety**: All shared resources (`IntentExecutor`, `IntentEventManager`) are wrapped in `Arc<Mutex<>>` for safe concurrent access.

use anyhow::{Context, Result};
use clap::Parser;
use dotenv::dotenv;
use log::{error, info, warn};
use std::env;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::signal;
use tokio::sync::Mutex;

mod config;
mod event;
mod executor;
mod forex_providers;
mod price_providers;
mod types;

use config::Config;
use event::{IntentEventManager, IntentEventMap};
use executor::IntentExecutor;
use forex_providers::{CurrencyLayerProvider, ExchangeRateApiProvider, ForexProvider};
use price_providers::{AtomCoinGeckoProvider, NGNPriceProvider, PriceProvider};
use types::IntentCreatedEvent;

/// Command-line arguments for the beep-filler application.
#[derive(Parser)]
#[command(name = "beep-filler")]
#[command(
    about = "An intent filler for beep, processing blockchain intents with price and forex data"
)]
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

    /// Minimum profit percentage override
    #[arg(long)]
    min_profit_percentage: Option<f64>,
}

/// Main entry point for the beep-filler application.
///
/// Initializes configuration, sets up the intent executor and event manager, and runs an interactive
/// CLI loop. Handles graceful shutdown via Ctrl+C or the `exit` command.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize environment variables and logging
    dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();

    // Load and override configuration
    let mut config = Config::from_file(&cli.config).unwrap_or_else(|e| {
        warn!("Could not load config file: {}. Using defaults.", e);
        Config::default()
    });

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

    config.validate().context("Invalid configuration")?;

    // Log configuration details
    info!("Starting with config:");
    info!("Config {{");
    info!("  rpc_http_endpoint: {}", config.rpc_http_endpoint);
    info!(
        "  rpc_websocket_endpoint: {}",
        config.rpc_websocket_endpoint
    );
    info!("  contract_address: {}", config.contract_address);
    info!(
        "  reconnect_delay_seconds: {}",
        config.reconnect_delay_seconds
    );
    info!(
        "  subscription_timeout_seconds: {}",
        config.subscription_timeout_seconds
    );
    info!("  event_retention_hours: {}", config.event_retention_hours);
    info!(
        "  default_timeout_height: {}",
        config.default_timeout_height
    );
    info!("  chain_id: {}", config.chain_id);
    info!("  fee_denom: {}", config.fee_denom);
    info!("  gas_limit: {}", config.gas_limit);
    info!("}}");

    // Load mnemonic for blockchain interactions
    let mnemonic = env::var("MNEMONIC")
        .ok()
        .or_else(|| config.load_mnemonic().ok().flatten())
        .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

    // Initialize IntentExecutor with shared mutable access
    let intent_executor = Arc::new(Mutex::new(
        IntentExecutor::new(config.clone(), mnemonic, cli.min_profit_percentage)
            .await
            .context("Failed to initialize IntentExecutor")?,
    ));

    // Initialize IntentEventManager for blockchain event monitoring
    let event_manager = Arc::new(Mutex::new(IntentEventManager::new(config.into())));
    let event_map: IntentEventMap = Arc::clone(&event_manager.lock().await.event_map);

    // Define callback for handling new intent events
    let handle_new_intent = {
        let executor = Arc::clone(&intent_executor);
        let event_map = Arc::clone(&event_map);
        move |event: IntentCreatedEvent| {
            let executor = Arc::clone(&executor);
            let event_map = Arc::clone(&event_map);
            async move {
                let executor = executor.lock().await;
                if !executor.supports_currency_pair("USD", "NGN") {
                    warn!(
                        "USD/NGN pair not supported, skipping intent: {}",
                        event.intent_id
                    );
                    return;
                }
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
        }
    };

    // Spawn event manager task to process blockchain events
    let event_manager_handle = {
        let event_manager = Arc::clone(&event_manager);
        tokio::spawn(async move {
            if let Err(e) = event_manager.lock().await.start(handle_new_intent).await {
                error!("Event manager failed: {}", e);
            }
        })
    };

    // Spawn interactive CLI loop
    let cli_handle = {
        let intent_executor = Arc::clone(&intent_executor);
        tokio::spawn(async move {
            let mut reader = BufReader::new(io::stdin());
            let mut line = String::new();
            info!("beep-filler CLI ready. Type 'help' for commands or 'exit' to quit.");

            loop {
                line.clear();
                print!("> ");
                io::stdout().flush().await.unwrap();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    // EOF
                    break;
                }
                let command = line.trim();
                if command.is_empty() {
                    continue;
                }

                match process_command(&intent_executor, command).await {
                    Ok(continue_running) => {
                        if !continue_running {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Command error: {}", e);
                    }
                }
            }
        })
    };

    // Wait for shutdown signal or CLI exit
    info!("Filler started. Press Ctrl+C or type 'exit' to stop.");
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Shutdown signal received.");
        }
        _ = cli_handle => {
            info!("CLI loop exited.");
        }
    }

    // Clean up resources
    info!("Cleaning up...");
    event_manager.lock().await.stop().await;
    {
        let executor = intent_executor.lock().await;
        executor.clear_price_cache().await;
        executor.clear_forex_cache(None).await;
    }

    if let Err(e) = event_manager_handle.await {
        warn!("Event manager task did not shut down cleanly: {}", e);
    }

    info!("Shutdown complete.");
    Ok(())
}

/// Processes runtime CLI commands for the beep-filler application.
///
/// Handles commands to manage price and forex providers, fetch exchange rates, and configure settings.
/// Returns `false` to exit the CLI loop when the `exit` command is issued, `true` otherwise.
///
/// # Arguments
///
/// * `intent_executor` - Shared reference to the `IntentExecutor` for executing commands.
/// * `command` - The command string entered by the user.
///
/// # Returns
///
/// A `Result` containing a boolean indicating whether to continue the CLI loop (`true`) or exit (`false`).
/// Errors are returned for invalid commands or execution failures.
///
/// # Commands
///
/// - `help`: Displays available commands.
/// - `exit`: Exits the CLI loop.
/// - `add-price-provider <name>`: Adds a price provider (e.g., `ngn_price_provider`, `atom_coingecko`).
/// - `remove-price-provider <name>`: Removes a price provider.
/// - `list-price-providers`: Lists all price providers.
/// - `add-forex-provider <name> [api-key]`: Adds a forex provider (e.g., `exchange_rate_api`, `currency_layer`).
/// - `remove-forex-provider <name>`: Removes a forex provider.
/// - `list-forex-providers`: Lists all forex providers.
/// - `list-supported-currency-pairs`: Lists supported currency pairs by provider.
/// - `check-currency-pair <base> <target>`: Checks if a currency pair is supported.
/// - `get-multiple-exchange-rates <pairs>`: Fetches rates for multiple pairs (e.g., `USD/NGN,EUR/NGN`).
/// - `clear-forex-cache [pair]`: Clears the forex cache, optionally for a specific pair.
/// - `clear-price-cache`: Clears the price cache.
/// - `set-min-profit-percentage <percentage>`: Sets the minimum profit percentage.
/// - `get-min-profit-percentage`: Gets the current minimum profit percentage.
/// - `set-forex-cache-duration <hours>`: Sets the forex cache duration in hours.
/// - `get-forex-cache-duration`: Gets the current forex cache duration.
/// - `get-forex-cache-stats`: Displays forex cache statistics.
async fn process_command(
    intent_executor: &Arc<Mutex<IntentExecutor>>,
    command: &str,
) -> Result<bool> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(true);
    }

    match parts[0].to_lowercase().as_str() {
        "help" => {
            println!(
                "Available commands:
  add-price-provider <name> - Add a price provider (ngn_price_provider, atom_coingecko)
  remove-price-provider <name> - Remove a price provider
  list-price-providers - List all price providers
  add-forex-provider <name> [api-key] - Add a forex provider (exchange_rate_api, currency_layer)
  remove-forex-provider <name> - Remove a forex provider
  list-forex-providers - List all forex providers
  list-supported-currency-pairs - List supported currency pairs
  check-currency-pair <base> <target> - Check if a currency pair is supported
  get-multiple-exchange-rates <pairs> - Get rates (e.g., USD/NGN,EUR/NGN)
  clear-forex-cache [pair] - Clear forex cache (e.g., USD/NGN)
  clear-price-cache - Clear price cache
  set-min-profit-percentage <percentage> - Set minimum profit percentage
  get-min-profit-percentage - Get minimum profit percentage
  set-forex-cache-duration <hours> - Set forex cache duration
  get-forex-cache-duration - Get forex cache duration
  get-forex-cache-stats - Get forex cache statistics
  exit - Exit the application
  help - Show this help message"
            );
            Ok(true)
        }
        "exit" => Ok(false),
        "add-price-provider" => {
            if parts.len() < 2 {
                info!("Usage: add-price-provider <name>");
                return Ok(true);
            }
            let provider_name = parts[1];
            let provider: Box<dyn PriceProvider> = match provider_name {
                "ngn_price_provider" => Box::new(NGNPriceProvider),
                "atom_coingecko" => Box::new(AtomCoinGeckoProvider),
                _ => {
                    info!(
                        "Unknown provider: {}. Supported: ngn_price_provider, atom_coingecko",
                        provider_name
                    );
                    return Ok(true);
                }
            };
            let mut executor = intent_executor.lock().await;
            executor.add_price_provider(provider);
            info!("Added price provider: {}", provider_name);
            Ok(true)
        }
        "remove-price-provider" => {
            if parts.len() < 2 {
                info!("Usage: remove-price-provider <name>");
                return Ok(true);
            }
            let provider_name = parts[1];
            let mut executor = intent_executor.lock().await;
            if executor.remove_price_provider(provider_name) {
                info!("Removed price provider: {}", provider_name);
            } else {
                info!("Provider not found: {}", provider_name);
            }
            Ok(true)
        }
        "list-price-providers" => {
            let executor = intent_executor.lock().await;
            let providers = executor.list_price_providers();
            info!("Price Providers:");
            for provider in providers {
                info!("- {}", provider);
            }
            Ok(true)
        }
        "add-forex-provider" => {
            if parts.len() < 2 {
                info!("Usage: add-forex-provider <name> [api-key]");
                return Ok(true);
            }
            let provider_name = parts[1];
            let api_key = parts.get(2).map(|s| s.to_string());
            let provider: Box<dyn ForexProvider> = match provider_name {
                "exchange_rate_api" => Box::new(ExchangeRateApiProvider),
                "currency_layer" => {
                    let api_key = api_key.context("API key required for currency_layer")?;
                    Box::new(CurrencyLayerProvider::new(api_key))
                }
                _ => {
                    info!(
                        "Unknown provider: {}. Supported: exchange_rate_api, currency_layer",
                        provider_name
                    );
                    return Ok(true);
                }
            };
            let mut executor = intent_executor.lock().await;
            executor.add_forex_provider(provider);
            info!("Added forex provider: {}", provider_name);
            Ok(true)
        }
        "remove-forex-provider" => {
            if parts.len() < 2 {
                info!("Usage: remove-forex-provider <name>");
                return Ok(true);
            }
            let provider_name = parts[1];
            let mut executor = intent_executor.lock().await;
            if executor.remove_forex_provider(provider_name) {
                info!("Removed forex provider: {}", provider_name);
            } else {
                info!("Provider not found: {}", provider_name);
            }
            Ok(true)
        }
        "list-forex-providers" => {
            let executor = intent_executor.lock().await;
            let providers = executor.list_forex_providers();
            info!("Forex Providers:");
            for provider in providers {
                info!("- {}", provider);
            }
            Ok(true)
        }
        "list-supported-currency-pairs" => {
            let executor = intent_executor.lock().await;
            let pairs = executor.get_supported_currency_pairs();
            info!("Supported Currency Pairs:");
            for (provider, currencies) in pairs {
                info!("Provider: {}", provider);
                info!("  Currencies: {}", currencies.join(", "));
            }
            Ok(true)
        }
        "check-currency-pair" => {
            if parts.len() < 3 {
                info!("Usage: check-currency-pair <base> <target>");
                return Ok(true);
            }
            let base = parts[1];
            let target = parts[2];
            let executor = intent_executor.lock().await;
            let supported = executor.supports_currency_pair(base, target);
            info!(
                "{}/{} is {}supported",
                base,
                target,
                if supported { "" } else { "not " }
            );
            Ok(true)
        }
        "get-multiple-exchange-rates" => {
            if parts.len() < 2 {
                info!("Usage: get-multiple-exchange-rates <pairs> (e.g., USD/NGN,EUR/NGN)");
                return Ok(true);
            }
            let pairs: Vec<(String, String)> = parts[1]
                .split(',')
                .filter_map(|pair| {
                    let parts: Vec<&str> = pair.split('/').collect();
                    if parts.len() == 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        error!("Invalid pair: {}", pair);
                        None
                    }
                })
                .collect();
            if pairs.is_empty() {
                info!("No valid pairs provided");
                return Ok(true);
            }
            let executor = intent_executor.lock().await;
            let rates = executor.get_multiple_exchange_rates(&pairs).await?;
            info!("Exchange Rates:");
            for (key, rate) in rates {
                info!("{}: {:.4}", key, rate);
            }
            Ok(true)
        }
        "clear-forex-cache" => {
            let pair = parts.get(1).and_then(|pair| {
                let parts: Vec<&str> = pair.split('/').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    error!("Invalid pair: {}", pair);
                    None
                }
            });
            let executor = intent_executor.lock().await;
            executor
                .clear_forex_cache(
                    pair.as_ref()
                        .map(|(base, quote)| (base.as_str(), quote.as_str())),
                )
                .await;
            info!(
                "Forex cache cleared{}",
                pair.map_or("", |_| " for specified pair")
            );
            Ok(true)
        }
        "clear-price-cache" => {
            let executor = intent_executor.lock().await;
            executor.clear_price_cache().await;
            info!("Price cache cleared");
            Ok(true)
        }
        "set-min-profit-percentage" => {
            if parts.len() < 2 {
                info!("Usage: set-min-profit-percentage <percentage>");
                return Ok(true);
            }
            let percentage = parts[1].parse::<f64>().context("Invalid percentage")?;
            let mut executor = intent_executor.lock().await;
            executor.set_min_profit_percentage(percentage)?;
            info!("Set minimum profit percentage to {}%", percentage);
            Ok(true)
        }
        "get-min-profit-percentage" => {
            let executor = intent_executor.lock().await;
            info!(
                "Minimum Profit Percentage: {}%",
                executor.get_min_profit_percentage() * 100.0
            );
            Ok(true)
        }
        "set-forex-cache-duration" => {
            if parts.len() < 2 {
                info!("Usage: set-forex-cache-duration <hours>");
                return Ok(true);
            }
            let hours = parts[1].parse::<u64>().context("Invalid hours")?;
            let mut executor = intent_executor.lock().await;
            executor.set_forex_cache_duration_hours(hours);
            info!("Set forex cache duration to {} hours", hours);
            Ok(true)
        }
        "get-forex-cache-duration" => {
            let executor = intent_executor.lock().await;
            info!(
                "Forex Cache Duration: {} hours",
                executor.get_forex_cache_duration_hours()
            );
            Ok(true)
        }
        "get-forex-cache-stats" => {
            let executor = intent_executor.lock().await;
            let stats = executor.get_forex_cache_stats().await;
            info!("Forex Cache Statistics:");
            for (key, value) in stats {
                info!("{}: {}", key, value);
            }
            Ok(true)
        }
        _ => {
            info!("Unknown command: {}. Type 'help' for commands.", parts[0]);
            Ok(true)
        }
    }
}
