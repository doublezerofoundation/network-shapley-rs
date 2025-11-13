# Network Shapley

[![CI](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml)

[![codecov](https://codecov.io/github/doublezerofoundation/network-shapley-rs/graph/badge.svg?token=S3QVQV7CFJ)](https://codecov.io/github/doublezerofoundation/network-shapley-rs)

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley).

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

## Local Development

### Install Dependencies

1. Install Rust (if not already installed):

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Install Just command runner:

   ```bash
   # macOS
   brew install just

   # Linux
   cargo install just

   # Or see: https://github.com/casey/just#installation
   ```

### Build the Project

```bash
# Development build (all features)
just build

# Release build (optimized)
just build-release
```

### Run Tests

```bash
# Run all tests with nextest (all features)
just test

# Run tests with standard cargo
cargo test

# Run a specific test
cargo test <test_name>
```

### Code Quality

```bash
# Check formatting
just fmt

# Run clippy lints
just clippy

# Run full CI pipeline (fmt + build + clippy + test)
just ci
```

## Usage

Here's a simple example showing how to compute Shapley values for network operators:

```rust
use network_shapley::{
    error::Result,
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

fn main() -> Result<()> {
    // Define private links (operator-owned connections)
    let private_links = vec![
        PrivateLink::new("SIN1".to_string(), "FRA1".to_string(), 50.0, 10.0, 1.0, None),
        PrivateLink::new("FRA1".to_string(), "AMS1".to_string(), 3.0, 10.0, 1.0, None),
        PrivateLink::new("FRA1".to_string(), "LON1".to_string(), 5.0, 10.0, 1.0, None),
    ];

    // Define devices (network nodes) and their operators
    let devices = vec![
        Device::new("SIN1".to_string(), 1, "Alpha".to_string()),
        Device::new("FRA1".to_string(), 1, "Alpha".to_string()),
        Device::new("AMS1".to_string(), 1, "Beta".to_string()),
        Device::new("LON1".to_string(), 1, "Beta".to_string()),
    ];

    // Define public links (available to all operators)
    let public_links = vec![
        PublicLink::new("SIN".to_string(), "FRA".to_string(), 100.0),
        PublicLink::new("SIN".to_string(), "AMS".to_string(), 102.0),
        PublicLink::new("FRA".to_string(), "LON".to_string(), 7.0),
        PublicLink::new("FRA".to_string(), "AMS".to_string(), 5.0),
    ];

    // Define network demands (traffic requests)
    let demands = vec![
        Demand::new("SIN".to_string(), "AMS".to_string(), 1, 1.0, 1.0, 1, true),
        Demand::new("SIN".to_string(), "LON".to_string(), 5, 1.0, 2.0, 1, true),
        Demand::new("AMS".to_string(), "LON".to_string(), 2, 3.0, 1.0, 2, false),
        Demand::new("AMS".to_string(), "FRA".to_string(), 1, 3.0, 1.0, 2, false),
    ];

    // Create input with configuration parameters
    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: 1.0,
    };

    // Compute Shapley values
    let result = input.compute()?;
    println!("{:?}", result);

    Ok(())
}
```

Run the example:

```bash
cargo run --example simple --features serde
```

Expected output:

```
 operator | value              | proportion
 Alpha    | 173.67559751778526 | 0.6701709231265766
 Beta     | 85.47560036995537  | 0.3298290768734235
```

The Shapley values represent each operator's contribution to the network's capacity to satisfy demands.

## Development

```bash
$ just
just -l
Available recipes:
    build             # Build (all features)
    build-release     # Build (release)
    build-release-all # Build (release, all features)
    ci                # Run CI pipeline
    clean             # Clean
    clippy            # Run clippy (all features)
    cov               # Coverage
    default           # Default (list of commands)
    example-demand1   # Run the demand1.csv example
    example-demand2   # Run the demand2.csv example
    example-simple    # Run the simple example
    fmt               # Check fmt
    test              # Run tests (all features)
```

## Examples (from python)

```bash
$ just example-simple
cargo run --example simple --release --features serde
    Finished `release` profile [optimized] target(s) in 0.04s
     Running `target/release/examples/simple`
 operator | value              | proportion
 Alpha    | 173.67559751778526 | 0.6701709231265766
 Beta     | 85.47560036995537  | 0.3298290768734235

$ just example-demand1
cargo run --example csv_demand1 --release --features serde
    Finished `release` profile [optimized] target(s) in 0.04s
     Running `target/release/examples/csv_demand1`
 operator | value               | proportion
 Alpha    | 21.536993608342087  | 0.020784847799528817
 Beta     | 10.659494463261947  | 0.010287228295085847
 Delta    | 13.525665991216918  | 0.013053303266330309
 Epsilon  | 0.04067654053843246 | 0.000039255975995424266
 Gamma    | 487.10943111827703  | 0.4700978962814409
 Kappa    | 0.06033839683996502 | 0.00005823117272507682
 Theta    | 503.1152843990428   | 0.48554477017629355
 Zeta     | 0.13933302036043532 | 0.00013446703260006367

$ just example-demand2
cargo run --example csv_demand2 --release --features serde
    Finished `release` profile [optimized] target(s) in 0.04s
     Running `target/release/examples/csv_demand2`
 operator | value              | proportion
 Alpha    | 2.01543556870695   | 0.0016167797068735875
 Beta     | 187.1198885455384  | 0.1501073233251362
 Delta    | 111.67271822565925 | 0.08958370460559766
 Epsilon  | 88.50224476557943  | 0.07099638190942015
 Gamma    | 23.034343549068872 | 0.018478119464360853
 Kappa    | 10.642164133816427 | 0.008537129777763754
 Theta    | 333.5522918447079  | 0.26757520062111845
 Zeta     | 490.0349272158809  | 0.3931053605897293
```
