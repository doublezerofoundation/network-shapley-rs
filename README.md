# Network Shapley

[![CI](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml)

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley).

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

## Usage

```rust
use rust_decimal::dec;
use shapley::{
    Demand, DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks,
};

fn main() {
    // Private links
    let private_links = PrivateLinks::from_links(vec![
        {
            LinkBuilder::new("FRA1".to_string(), "NYC1".to_string())
                .cost(dec!(40))
                .bandwidth(dec!(10))
                .operator1("Alpha".to_string())
                .build()
        },
        {
            LinkBuilder::new("FRA1".to_string(), "SIN1".to_string())
                .cost(dec!(50))
                .bandwidth(dec!(10))
                .operator1("Beta".to_string())
                .build()
        },
        {
            LinkBuilder::new("SIN1".to_string(), "NYC1".to_string())
                .cost(dec!(80))
                .bandwidth(dec!(10))
                .operator1("Gamma".to_string())
                .build()
        },
    ]);

    // Public links
    let public_links = PublicLinks::from_links(vec![
        {
            LinkBuilder::new("FRA1".to_string(), "NYC1".to_string())
                .cost(dec!(70))
                .build()
        },
        {
            LinkBuilder::new("FRA1".to_string(), "SIN1".to_string())
                .cost(dec!(80))
                .build()
        },
        {
            LinkBuilder::new("SIN1".to_string(), "NYC1".to_string())
                .cost(dec!(120))
                .build()
        },
    ]);

    // Demand
    let demand = DemandMatrix::from_demands(vec![
        Demand::new("SIN".to_string(), "NYC".to_string(), dec!(5), 1),
        Demand::new("SIN".to_string(), "FRA".to_string(), dec!(5), 1),
    ]);

    // Compute shapley values
    let result = NetworkShapleyBuilder::new(private_links, public_links, demand)
        .operator_uptime(dec!(0.98))
        .hybrid_penalty(dec!(5.0))
        .demand_multiplier(dec!(1.0))
        .build()
        .compute();
    match result {
        Err(e) => {
            eprintln!("Error computing Shapley values: {}", e);
        }
        Ok(shapley_values) => {
            for sv in shapley_values {
                println!(
                    "Operator: {}, Shapley: {}, Percent: {}%",
                    sv.operator,
                    sv.value,
                    sv.percent * dec!(100)
                );
            }
        }
    }
}
```

## Examples

```
$ just simulated-example
cargo run --example simulated --features csv --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `target/release/examples/simulated`
 operator | value   | percent
 a        | 0.0082  | 0.0003
 b        | 0.1259  | 0.0049
 c        | 13.0194 | 0.5041
 d        | 7.1835  | 0.2781
 e        | 0.0727  | 0.0028
 f        | 0.0224  | 0.0009
 g        | 2.2136  | 0.0857
 h        | 0.3989  | 0.0154
 i        | 0.8118  | 0.0314
 j        | 1.9552  | 0.0757
 k        | 0.0177  | 0.0007

$ just csv-example
cargo run --example csv --features csv --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `target/release/examples/csv`
result1:
 operator | value    | percent
 Alpha    | 36.0066  | 0.0205
 Beta     | 29.6241  | 0.0168
 Delta    | 48.4246  | 0.0275
 Epsilon  | 1.4944   | 0.0008
 Gamma    | 874.2343 | 0.4972
 Kappa    | 241.6951 | 0.1375
 Theta    | 526.7844 | 0.2996
 Zeta     | 0.0506   | 0.0000
result2:
 operator | value    | percent
 Alpha    | 27.0093  | 0.0187
 Beta     | 298.6748 | 0.2066
 Delta    | 160.0684 | 0.1107
 Epsilon  | 0.0000   | 0
 Gamma    | 71.6712  | 0.0496
 Kappa    | 30.9147  | 0.0214
 Theta    | 439.1824 | 0.3038
 Zeta     | 417.8876 | 0.2891
```

## Development

```bash
$ just
just -l
Available recipes:
    bench             # Run benchmarks
    build             # Build (all features)
    build-release     # Build (release)
    build-release-all # Build (release, all features)
    ci                # Run CI pipeline
    clean             # Clean
    clippy            # Run clippy (all features)
    csv-example       # Run the csv example
    default           # Default (list of commands)
    fmt               # Check fmt
    simple-example    # Run the simple example
    simulated-example # Run the simulated example
    test              # Run tests (all features)
```
