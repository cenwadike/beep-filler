# Beep Intent Filler: Powering Intent Execution on Beep

## Project Overview

Beep Intent Filler is a Rust-based service within the Beep mobile money platform, designed to process blockchain-based intents on the Neutron blockchain. It handles token swap intents (e.g., exchanging tokenized Naira, tNGN, for tokenized ATOM, tAtom) by evaluating profitability, fetching token prices and forex rates, and executing transactions via CosmWasm smart contracts. Built for fault tolerance, it integrates with external price and forex providers, caches data for performance, and ensures secure transaction signing using BIP-39 mnemonics.

## Key Features

- Intent Processing: Handles IntentCreatedEvents to validate and execute token swap intents.

- Profit Analysis: Evaluates swap profitability using real-time token prices and forex rates, ensuring a configurable minimum profit margin (default: 0.3%).

- Price and Forex Integration: Fetches token prices (e.g., tNGN, tAtom) and exchange rates (e.g., USD/NGN) from providers like CoinGecko and CurrencyLayer.

- Fault Tolerance: Includes retry logic for API and blockchain calls (3 attempts, exponential backoff), HTTP timeouts (10s), and input validation to prevent crashes.

- Caching: Caches price data (5 minutes) and forex rates (8 hours) to reduce API calls.

- Blockchain Transactions: Executes CosmWasm contract calls (e.g., increase allowance, fill intent) with secure signing.

- Extensibility: Supports dynamic addition/removal of price and forex providers.

## Getting Started

To set up Beep Intent Filler locally, follow these steps:

### Prerequisites

- Rust: rustup 1.28.1, rustc 1.85.1, cargo 1.85.1.

- Neutron Wallet: Fund with testnet NTRN (e.g., via Keplr).

### Setup

1. Clone the Repository:

```bash
    git clone https://github.com/cenwadike/beep-filler
    cd beep-filler
```

2. Install Dependencies:

```bash
    cargo build
```

3. Configure Environment Variables: Create a *.env* file using *.env.example*

```bash
    # Mnemonic for the account that will execute intents
    MNEMONIC="your twelve word mnemonic phrase goes here for the executing account"

    # Optional: Override RPC endpoint
    # RPC_ENDPOINT="https://rpc-palvus.pion-1.ntrn.tech"

    # Optional: Override contract address  
    # CONTRACT_ADDRESS="neutron13r9m3cn8zu6rnmkepajnm04zrry4g24exy9tunslseet0s9wrkkstcmkhr"

    # Optional: Log level (error, warn, info, debug, trace)
    RUST_LOG=info
```

- *Note: ADMIN_MNEMONIC must match a funded Neutron wallet. Obtain CURRENCY_LAYER_API_KEY from CurrencyLayer.*

4. Run the Intent Filler:

```bash
    cargo run
```

*The service listens for **IntentCreatedEvents** and processes intents.*

## Conclusion

Beep Intent Filler powers Beep’s mobile money platform by automating intent execution. Leveraging CosmWasm, Cosmos SDK, and fault-tolerant design, it ensures profitable and secure transactions. Explore the project at https://github.com/come-senusi-wale/beep-hackar and contribute to bridging fiat and crypto in the Cosmos ecosystem.