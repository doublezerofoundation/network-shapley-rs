# Network Shapley

[![CI](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml)

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley).

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

## Example

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
    test              # Run tests (all features)
```
