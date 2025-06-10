# Network Shapley

[![CI](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml)

[![codecov](https://codecov.io/github/doublezerofoundation/network-shapley-rs/graph/badge.svg?token=S3QVQV7CFJ)](https://codecov.io/github/doublezerofoundation/network-shapley-rs)

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley).

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

## Usage

```rust
use rust_decimal::dec;
use shapley::{
    DemandBuilder, DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks,
    error::Result,
};

fn main() -> Result<()> {
    // Private links
    let private_links = PrivateLinks::from_links(vec![
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(40))
                .bandwidth(dec!(10))
                .operator1("Alpha".to_string())
                .build()?
        },
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("SIN1".to_string())
                .cost(dec!(50))
                .bandwidth(dec!(10))
                .operator1("Beta".to_string())
                .build()?
        },
        {
            LinkBuilder::default()
                .start("SIN1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(80))
                .bandwidth(dec!(10))
                .operator1("Gamma".to_string())
                .build()?
        },
    ]);

    // Public links
    let public_links = PublicLinks::from_links(vec![
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(70))
                .build()?
        },
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("SIN1".to_string())
                .cost(dec!(80))
                .build()?
        },
        {
            LinkBuilder::default()
                .start("SIN1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(120))
                .build()?
        },
    ]);

    // Demand
    let demand = DemandMatrix::from_demands(vec![
        DemandBuilder::default()
            .start("SIN".to_string())
            .end("NYC".to_string())
            .traffic(dec!(5))
            .demand_type(1)
            .build()?,
        DemandBuilder::default()
            .start("SIN".to_string())
            .end("FRA".to_string())
            .traffic(dec!(5))
            .demand_type(1)
            .build()?,
    ]);

    // Compute shapley values
    let result = NetworkShapleyBuilder::default()
        .private_links(private_links)
        .public_links(public_links)
        .demand(demand)
        .operator_uptime(dec!(0.98))
        .hybrid_penalty(dec!(5.0))
        .demand_multiplier(dec!(1.0))
        .build()?
        .compute()?;

    for sv in result {
        println!(
            "Operator: {}, Shapley: {}, Percent: {}%",
            sv.operator,
            sv.value,
            sv.percent * dec!(100)
        );
    }

    Ok(())
}
```

## Examples

```bash
$ just simple-example
cargo run --example simple --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `target/release/examples/simple`
 Operator      Value    Percent
    Alpha    24.9704       7.22%
     Beta   171.9704      49.72%
    Gamma   148.9404      43.06%

$ just csv-example
cargo run --example csv --features csv --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `target/release/examples/csv`
result1:
 operator | value    | percent
 Alpha    | 36.0066  | 2.05%
 Beta     | 29.6241  | 1.68%
 Delta    | 48.4246  | 2.75%
 Epsilon  | 1.4944   | 0.08%
 Gamma    | 874.2343 | 49.72%
 Kappa    | 241.6951 | 13.75%
 Theta    | 526.7844 | 29.96%
 Zeta     | 0.0506   | 0.00%
result2:
 operator | value    | percent
 Alpha    | 27.0093  | 1.87%
 Beta     | 298.6748 | 20.66%
 Delta    | 160.0684 | 11.07%
 Epsilon  | 0.0000   | 0.00%
 Gamma    | 71.6712  | 4.96%
 Kappa    | 30.9147  | 2.14%
 Theta    | 439.1824 | 30.38%
 Zeta     | 417.8876 | 28.91%

$ just simulated-example
cargo run --example simulated --features csv --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `target/release/examples/simulated`
 operator | value   | percent
 a        | 0.0082  | 0.03%
 b        | 0.1259  | 0.49%
 c        | 13.0194 | 50.41%
 d        | 7.1835  | 27.81%
 e        | 0.0727  | 0.28%
 f        | 0.0224  | 0.09%
 g        | 2.2136  | 8.57%
 h        | 0.3989  | 1.54%
 i        | 0.8118  | 3.14%
 j        | 1.9552  | 7.57%
 k        | 0.0177  | 0.07%
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
