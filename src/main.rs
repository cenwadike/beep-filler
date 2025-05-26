use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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

#[derive(Parser)]
#[command(name = "beep-filler")]
#[command(about = "An intent filler for beep")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path
    #[arg(short, long, default_value = "config.json", global = true)]
    config: String,

    /// RPC HTTP endpoint override
    #[arg(long, global = true)]
    rpc_http_endpoint: Option<String>,

    /// RPC WebSocket endpoint override
    #[arg(long, global = true)]
    rpc_websocket_endpoint: Option<String>,

    /// Contract address override
    #[arg(long, global = true)]
    contract_address: Option<String>,

    /// Chain ID override
    #[arg(long, global = true)]
    chain_id: Option<String>,

    /// Minimum profit percentage override
    #[arg(long, global = true)]
    min_profit_percentage: Option<f64>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the intent filler with an interactive CLI
    Run,
    /// Add a price provider
    AddPriceProvider {
        #[arg(long)]
        provider_name: String,
    },
    /// Remove a price provider
    RemovePriceProvider {
        #[arg(long)]
        provider_name: String,
    },
    /// List all price providers
    ListPriceProviders,
    /// Add a forex provider
    AddForexProvider {
        #[arg(long)]
        provider_name: String,
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Remove a forex provider
    RemoveForexProvider {
        #[arg(long)]
        provider_name: String,
    },
    /// List all forex providers
    ListForexProviders,
    /// List supported currency pairs
    ListSupportedCurrencyPairs,
    /// Check if a currency pair is supported
    CheckCurrencyPair {
        #[arg(long)]
        base_currency: String,
        #[arg(long)]
        target_currency: String,
    },
    /// Get exchange rates for multiple currency pairs
    GetMultipleExchangeRates {
        #[arg(long, value_delimiter = ',')]
        pairs: Vec<String>,
    },
    /// Clear the forex cache
    ClearForexCache {
        #[arg(long)]
        currency_pair: Option<String>,
    },
    /// Clear the price cache
    ClearPriceCache,
    /// Set the minimum profit percentage
    SetMinProfitPercentage {
        #[arg(long)]
        percentage: f64,
    },
    /// Get the minimum profit percentage
    GetMinProfitPercentage,
    /// Set the forex cache duration in hours
    SetForexCacheDuration {
        #[arg(long)]
        hours: u64,
    },
    /// Get the forex cache duration in hours
    GetForexCacheDuration,
    /// Get forex cache statistics
    GetForexCacheStats,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();
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
    let config_log = serde_json::json!(config_log).to_string();
    info!("Starting with config: {}", config_log);

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            // Wrap IntentExecutor in Arc<Mutex<>> for shared mutable access
            let intent_executor = Arc::new(Mutex::new(
                IntentExecutor::new(config.clone(), mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?,
            ));

            // Access executor for initialization logging
            {
                let executor = intent_executor.lock().await;
                info!(
                    "Current minimum profit percentage: {}%",
                    executor.get_min_profit_percentage() * 100.0
                );
                info!(
                    "Forex cache duration: {} hours",
                    executor.get_forex_cache_duration_hours()
                );
                let supported_pairs = executor.get_supported_currency_pairs();
                for (provider, currencies) in supported_pairs {
                    info!(
                        "Provider {} supports {} currencies",
                        provider,
                        currencies.len()
                    );
                }
            }

            let event_manager = Arc::new(Mutex::new(IntentEventManager::new(config.into())));
            let event_map: IntentEventMap = Arc::clone(&event_manager.lock().await.event_map);

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

            let event_manager_handle = {
                let event_manager = Arc::clone(&event_manager);
                tokio::spawn(async move {
                    if let Err(e) = event_manager.lock().await.start(handle_new_intent).await {
                        error!("Event manager failed: {}", e);
                    }
                })
            };

            // Start the CLI loop
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
        }
        Commands::AddPriceProvider { provider_name } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            let provider: Box<dyn PriceProvider> = match provider_name.as_str() {
                "ngn_price_provider" => Box::new(NGNPriceProvider),
                "atom_coingecko" => Box::new(AtomCoinGeckoProvider),
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unknown price provider: {}. Supported: ngn_price_provider, atom_coingecko",
                        provider_name
                    ));
                }
            };

            intent_executor.add_price_provider(provider);
            info!("Successfully added price provider: {}", provider_name);
        }
        Commands::RemovePriceProvider { provider_name } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            if intent_executor.remove_price_provider(&provider_name) {
                info!("Successfully removed price provider: {}", provider_name);
            } else {
                warn!("Failed to remove price provider: {}", provider_name);
            }
        }
        Commands::ListPriceProviders => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let providers = intent_executor.list_price_providers();
            info!("Price Providers:");
            for provider in providers {
                info!("- {}", provider);
            }
        }
        Commands::AddForexProvider {
            provider_name,
            api_key,
        } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            let provider: Box<dyn ForexProvider> = match provider_name.as_str() {
                "exchangerate_api" => Box::new(ExchangeRateApiProvider),
                "currencylayer" => Box::new(CurrencyLayerProvider::new(api_key)),
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unknown forex provider: {}. Supported: exchangerate_api, currencylayer",
                        provider_name
                    ));
                }
            };

            intent_executor.add_forex_provider(provider);
            info!("Successfully added forex provider: {}", provider_name);
        }
        Commands::RemoveForexProvider { provider_name } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            if intent_executor.remove_forex_provider(&provider_name) {
                info!("Successfully removed forex provider: {}", provider_name);
            } else {
                warn!("Failed to remove forex provider: {}", provider_name);
            }
        }
        Commands::ListForexProviders => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let providers = intent_executor.list_forex_providers();
            info!("Forex Providers:");
            for provider in providers {
                info!("- {}", provider);
            }
        }
        Commands::ListSupportedCurrencyPairs => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let pairs = intent_executor.get_supported_currency_pairs();
            info!("Supported Currency Pairs:");
            for (provider, currencies) in pairs {
                info!("Provider: {}", provider);
                info!("  Currencies: {}", currencies.join(", "));
            }
        }
        Commands::CheckCurrencyPair {
            base_currency,
            target_currency,
        } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let supported =
                intent_executor.supports_currency_pair(&base_currency, &target_currency);
            info!(
                "{}/{} is {}supported",
                base_currency,
                target_currency,
                if supported { "" } else { "not " }
            );
        }
        Commands::GetMultipleExchangeRates { pairs } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let currency_pairs: Vec<(String, String)> = pairs
                .into_iter()
                .filter_map(|pair| {
                    let parts: Vec<&str> = pair.split('/').collect();
                    if parts.len() == 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        error!("Invalid currency pair format: {}", pair);
                        None
                    }
                })
                .collect();

            let rates = intent_executor
                .get_multiple_exchange_rates(&currency_pairs)
                .await
                .context("Failed to fetch exchange rates")?;

            info!("Exchange Rates:");
            for (key, rate) in rates {
                info!("{}: {:.4}", key, rate);
            }
        }
        Commands::ClearForexCache { currency_pair } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let pair = currency_pair.and_then(|pair| {
                let parts: Vec<&str> = pair.split('/').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    error!("Invalid currency pair format: {}", pair);
                    None
                }
            });

            intent_executor
                .clear_forex_cache(
                    pair.as_ref()
                        .map(|(base, quote)| (base.as_str(), quote.as_str())),
                )
                .await;
        }
        Commands::ClearPriceCache => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            intent_executor.clear_price_cache().await;
        }
        Commands::SetMinProfitPercentage { percentage } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            intent_executor
                .set_min_profit_percentage(percentage)
                .context("Failed to set minimum profit percentage")?;
        }
        Commands::GetMinProfitPercentage => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            info!(
                "Minimum Profit Percentage: {}%",
                intent_executor.get_min_profit_percentage() * 100.0
            );
        }
        Commands::SetForexCacheDuration { hours } => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let mut intent_executor =
                IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                    .await
                    .context("Failed to initialize IntentExecutor")?;

            intent_executor.set_forex_cache_duration_hours(hours);
        }
        Commands::GetForexCacheDuration => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            info!(
                "Forex Cache Duration: {} hours",
                intent_executor.get_forex_cache_duration_hours()
            );
        }
        Commands::GetForexCacheStats => {
            let mnemonic = env::var("MNEMONIC")
                .ok()
                .or_else(|| config.load_mnemonic().ok().flatten())
                .context("Mnemonic not provided in MNEMONIC env var or mnemonic.txt")?;

            let intent_executor = IntentExecutor::new(config, mnemonic, cli.min_profit_percentage)
                .await
                .context("Failed to initialize IntentExecutor")?;

            let stats = intent_executor.get_forex_cache_stats().await;
            info!("Forex Cache Statistics:");
            for (key, value) in stats {
                info!("{}: {}", key, value);
            }
        }
    }

    Ok(())
}

// Process runtime CLI commands
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
  add-forex-provider <name> [api-key] - Add a forex provider (exchangerate_api, currencylayer)
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
                "exchangerate_api" => Box::new(ExchangeRateApiProvider),
                "currencylayer" => Box::new(CurrencyLayerProvider::new(api_key)),
                _ => {
                    info!(
                        "Unknown provider: {}. Supported: exchangerate_api, currencylayer",
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
