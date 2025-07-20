# Network Shapley

[![CI](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/doublezerofoundation/network-shapley-rs/actions/workflows/ci.yml)

[![codecov](https://codecov.io/github/doublezerofoundation/network-shapley-rs/graph/badge.svg?token=S3QVQV7CFJ)](https://codecov.io/github/doublezerofoundation/network-shapley-rs)

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley).

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

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
