//! Parity test: validates that Rust and Python implementations produce identical
//! Shapley values from the same CSV inputs.
//!
//! Requires Python 3 with pandas and scipy installed, plus the network-shapley-py
//! repo accessible (see tests/python_parity.py for search paths).
//!
//! Run with: cargo test --features serde --test python_parity_test
//! Skip with: cargo test --test python_parity_test -- --ignored (if Python unavailable)

use std::{collections::BTreeMap, fs::File, process::Command};

use network_shapley::{
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

/// Tolerance for value comparison. Values within this absolute difference
/// are considered identical (accounts for floating-point representation).
const VALUE_TOLERANCE: f64 = 0.01;

fn read_csv<T: serde::de::DeserializeOwned>(path: &str) -> Vec<T> {
    let file = File::open(path).unwrap_or_else(|e| panic!("Failed to open {path}: {e}"));
    csv::Reader::from_reader(file)
        .deserialize()
        .map(|r| r.unwrap())
        .collect()
}

fn run_rust_shapley(demand_file: &str, multiplier: f64) -> BTreeMap<String, f64> {
    let private_links: Vec<PrivateLink> = read_csv("tests/private_links.csv");
    let devices: Vec<Device> = read_csv("tests/devices.csv");
    let public_links: Vec<PublicLink> = read_csv("tests/public_links.csv");
    let demands: Vec<Demand> = read_csv(demand_file);

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 0.98,
        contiguity_bonus: 5.0,
        demand_multiplier: multiplier,
    };

    let result = input.compute().expect("Shapley computation failed");
    result.into_iter().map(|(op, sv)| (op, sv.value)).collect()
}

/// Returns None if Python or required deps (pandas, scipy) are not available.
fn run_python_shapley() -> Option<BTreeMap<String, BTreeMap<String, f64>>> {
    let output = match Command::new("python3")
        .arg("tests/python_parity.py")
        .output()
    {
        Ok(o) => o,
        Err(_) => {
            eprintln!("SKIP: python3 not found");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("ModuleNotFoundError") {
            eprintln!("SKIP: Python deps not installed (pandas/scipy)");
            return None;
        }
        panic!("Python parity script failed:\n{stderr}");
    }

    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8 from Python");
    Some(serde_json::from_str(&stdout).expect("Failed to parse Python JSON output"))
}

fn compare_results(scenario: &str, rust: &BTreeMap<String, f64>, python: &BTreeMap<String, f64>) {
    assert_eq!(
        rust.len(),
        python.len(),
        "{scenario}: operator count mismatch: Rust={}, Python={}",
        rust.len(),
        python.len()
    );

    let mut max_diff = 0.0f64;
    let mut max_diff_op = String::new();

    for (op, &rv) in rust {
        let &pv = python
            .get(op)
            .unwrap_or_else(|| panic!("{scenario}: operator {op} missing from Python output"));

        let diff = (rv - pv).abs();
        if diff > max_diff {
            max_diff = diff;
            max_diff_op = op.clone();
        }

        assert!(
            diff < VALUE_TOLERANCE,
            "{scenario}: value mismatch for {op}: Rust={rv:.6}, Python={pv:.6}, diff={diff:.6} (tolerance={VALUE_TOLERANCE})",
        );
    }

    eprintln!("{scenario}: PASS (max diff={max_diff:.8} on {max_diff_op})");
}

#[test]
fn test_rust_python_parity() {
    let python_results = match run_python_shapley() {
        Some(r) => r,
        None => {
            eprintln!("Skipping parity test — Python or deps not available");
            return;
        }
    };

    // Scenario: demand1 with multiplier 1.0
    let rust = run_rust_shapley("tests/demand1.csv", 1.0);
    let python = python_results
        .get("demand1_1x")
        .expect("Missing demand1_1x from Python");
    compare_results("demand1 (1.0x)", &rust, python);

    // Scenario: demand1 with multiplier 1.2
    let rust = run_rust_shapley("tests/demand1.csv", 1.2);
    let python = python_results
        .get("demand1_1.2x")
        .expect("Missing demand1_1.2x from Python");
    compare_results("demand1 (1.2x)", &rust, python);

    // Scenario: demand2 with multiplier 1.0
    let rust = run_rust_shapley("tests/demand2.csv", 1.0);
    let python = python_results
        .get("demand2_1x")
        .expect("Missing demand2_1x from Python");
    compare_results("demand2 (1.0x)", &rust, python);

    // Scenario: demand2 with multiplier 1.2
    let rust = run_rust_shapley("tests/demand2.csv", 1.2);
    let python = python_results
        .get("demand2_1.2x")
        .expect("Missing demand2_1.2x from Python");
    compare_results("demand2 (1.2x)", &rust, python);
}
