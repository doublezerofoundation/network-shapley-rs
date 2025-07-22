use network_shapley::{
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

#[test]
fn test_simple_example_matches_python() {
    // This test replicates the simple_example.py from the Python reference implementation
    // Expected output:
    //   Alpha: 173.6756 (67.02%)
    //   Beta:   85.4756 (32.98%)

    // Private links data from simple_example.py
    let private_links = vec![
        PrivateLink::new(
            "SIN1".to_string(),
            "FRA1".to_string(),
            50.0, // Latency: 50
            10.0, // Bandwidth: 10
            1.0,  // Uptime: 1
            None, // Shared: None (pd.NA in Python)
        ),
        PrivateLink::new(
            "FRA1".to_string(),
            "AMS1".to_string(),
            3.0,  // Latency: 3
            10.0, // Bandwidth: 10
            1.0,  // Uptime: 1
            None, // Shared: None (pd.NA in Python)
        ),
        PrivateLink::new(
            "FRA1".to_string(),
            "LON1".to_string(),
            5.0,  // Latency: 5
            10.0, // Bandwidth: 10
            1.0,  // Uptime: 1
            None, // Shared: None (pd.NA in Python)
        ),
    ];

    // Devices data
    let devices = vec![
        Device::new("SIN1".to_string(), 1, "Alpha".to_string()),
        Device::new("FRA1".to_string(), 1, "Alpha".to_string()),
        Device::new("AMS1".to_string(), 1, "Beta".to_string()),
        Device::new("LON1".to_string(), 1, "Beta".to_string()),
    ];

    // Public links data
    let public_links = vec![
        PublicLink::new("SIN".to_string(), "FRA".to_string(), 100.0),
        PublicLink::new("SIN".to_string(), "AMS".to_string(), 102.0),
        PublicLink::new("FRA".to_string(), "LON".to_string(), 7.0),
        PublicLink::new("FRA".to_string(), "AMS".to_string(), 5.0),
    ];

    // Demands data
    let demands = vec![
        Demand::new(
            "SIN".to_string(),
            "AMS".to_string(),
            1,    // Receivers: 1
            1.0,  // Traffic: 1
            1.0,  // Priority: 1
            1,    // Type: 1
            true, // Multicast: true
        ),
        Demand::new(
            "SIN".to_string(),
            "LON".to_string(),
            5,    // Receivers: 5
            1.0,  // Traffic: 1
            2.0,  // Priority: 2
            1,    // Type: 1
            true, // Multicast: true
        ),
        Demand::new(
            "AMS".to_string(),
            "LON".to_string(),
            2,     // Receivers: 2
            3.0,   // Traffic: 3
            1.0,   // Priority: 1
            2,     // Type: 2
            false, // Multicast: false
        ),
        Demand::new(
            "AMS".to_string(),
            "FRA".to_string(),
            1,     // Receivers: 1
            3.0,   // Traffic: 3
            1.0,   // Priority: 1
            2,     // Type: 2
            false, // Multicast: false
        ),
    ];

    // Create input struct
    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 0.98,  // 0.98
        contiguity_bonus: 5.0,  // 5.0
        demand_multiplier: 1.0, // 1.0
    };

    // Compute Shapley values
    let result = input.compute().expect("Failed to compute Shapley values");

    // Check results
    assert_eq!(result.len(), 2, "Should have 2 operators");

    // Find Alpha and Beta values
    let alpha = result.get("Alpha").unwrap();
    let beta = result.get("Beta").unwrap();

    // Expected values from Python
    let expected_alpha = 173.6756;
    let expected_beta = 85.4756;

    // Allow small tolerance for floating point differences
    let tolerance = 0.01; // 0.01

    assert!(
        (alpha.value - expected_alpha).abs() < tolerance,
        "Alpha value mismatch: expected {}, got {}",
        expected_alpha,
        alpha.value
    );

    assert!(
        (beta.value - expected_beta).abs() < tolerance,
        "Beta value mismatch: expected {}, got {}",
        expected_beta,
        beta.value
    );

    // Check proportion
    let expected_alpha_proportion = 0.6702;
    let expected_beta_proportion = 0.3298;

    assert!(
        (alpha.proportion - expected_alpha_proportion).abs() < tolerance,
        "Alpha proportion mismatch: expected {}, got {}",
        expected_alpha_proportion,
        alpha.proportion
    );

    assert!(
        (beta.proportion - expected_beta_proportion).abs() < tolerance,
        "Beta proportion mismatch: expected {}, got {}",
        expected_beta_proportion,
        beta.proportion
    );
}
