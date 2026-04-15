#!/usr/bin/env python3
"""Run the Python network_shapley on test fixtures and output JSON for comparison.

Used by the Rust parity test (python_parity_test.rs) to validate that
both implementations produce identical results from the same inputs.

Requires: pip install pandas scipy
"""
import json
import sys
import os

# Find the Python network_shapley module — check common locations
SEARCH_PATHS = [
    os.path.join(os.path.dirname(__file__), "..", "..", "network-shapley-py"),
    os.path.join(os.path.dirname(__file__), "..", "..", "network-shapley"),
    os.environ.get("NETWORK_SHAPLEY_PY_PATH", ""),
]

for path in SEARCH_PATHS:
    if path and os.path.isfile(os.path.join(path, "network_shapley.py")):
        sys.path.insert(0, path)
        break

import warnings
warnings.filterwarnings("ignore")

import pandas as pd
from network_shapley import network_shapley

TEST_DIR = os.path.join(os.path.dirname(__file__))


def run_scenario(demand_file: str, multiplier: float) -> dict:
    devices = pd.read_csv(os.path.join(TEST_DIR, "devices.csv"))
    devices.columns = ["Device", "Edge", "Operator"]

    private_links = pd.read_csv(os.path.join(TEST_DIR, "private_links.csv"))
    private_links.columns = ["Device1", "Device2", "Latency", "Bandwidth", "Uptime", "Shared"]

    public_links = pd.read_csv(os.path.join(TEST_DIR, "public_links.csv"))
    public_links.columns = ["City1", "City2", "Latency"]

    demand = pd.read_csv(os.path.join(TEST_DIR, demand_file))
    demand.columns = ["Start", "End", "Receivers", "Traffic", "Priority", "Type", "Multicast"]

    result = network_shapley(
        private_links=private_links,
        devices=devices,
        demand=demand,
        public_links=public_links,
        operator_uptime=0.98,
        contiguity_bonus=5.0,
        demand_multiplier=multiplier,
    )

    return {row["Operator"]: row["Value"] for _, row in result.iterrows()}


if __name__ == "__main__":
    output = {
        "demand1_1x": run_scenario("demand1.csv", 1.0),
        "demand1_1.2x": run_scenario("demand1.csv", 1.2),
        "demand2_1x": run_scenario("demand2.csv", 1.0),
        "demand2_1.2x": run_scenario("demand2.csv", 1.2),
    }
    print(json.dumps(output))
