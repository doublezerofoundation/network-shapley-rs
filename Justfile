# Default (list of commands)
default:
    just -l

# ---------------
# Examples
# ---------------

# Run the simple example
example-simple:
    cargo run --example simple --release --features serde

# Run the demand1.csv example
example-demand1:
    cargo run --example csv_demand1 --release --features serde

# Run the demand2.csv example
example-demand2:
    cargo run --example csv_demand2 --release --features serde

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

# Clean
clean:
    cargo clean

# Coverage
cov:
    cargo llvm-cov nextest --features serde --lcov --output-path lcov.info

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
