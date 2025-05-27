# Network Shapley

Rust implementation to match Python [network-shapley](https://github.com/doublezerofoundation/network-shapley) 1:1.

## Prerequisites

- Rust (stable, tested with 1.87.0)
- [Just](https://github.com/casey/just) (alternative to `make`)

## Run

```bash
$ just
just --list --unsorted
Available recipes:
    default
    simple-example    # Run the simple example
    csv-example       # Run the csv example
    fmt               # Check fmt
    build             # Build (all features)
    clippy            # Run clippy (all features)
    test              # Run tests (all features)
    ci                # Run CI pipeline
    build-release     # Build (release)
    build-release-all # Build (release, all features)
```
