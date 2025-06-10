# Default (list of commands)
default:
    just -l

# ---------------
# Examples
# ---------------

# Run the simple example
simple-example:
    cargo run --example simple --release

# Run the csv example
csv-example:
    cargo run --example csv --features csv --release

# Run the simulated example
simulated-example:
    cargo run --example simulated --features csv --release

# ---------------
# Dev
# ---------------

# Check fmt
fmt:
    cargo fmt --check

# Build (all features)
build:
    cargo build --all-features

# Run clippy (all features)
clippy:
    cargo clippy --all-features -- -Dclippy::all -D warnings

# Run tests (all features)
test:
    cargo nextest run --all-features

# Run benchmarks
bench:
    cargo bench

# Clean
clean:
    cargo clean

# Coverage
cov:
    cargo llvm-cov nextest --features csv --lcov --output-path lcov.info

# ---------------
# CI
# ---------------
# Run CI pipeline
ci:
    @just fmt
    @just build
    @just clippy
    @just test

# ---------------
# Release
# ---------------
# Build (release)
build-release:
    cargo build --release

# Build (release, all features)
build-release-all:
    cargo build --release --all-features
