use network_shapley::{
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

/// Test that device names from real telemetry data are accepted by validation
///
/// Naming conventions:
/// - Testnet: {city}-dz{number} format (e.g., lax-dz001, nyc-dz001)
/// - Devnet: {city}-dn-dzd{number} format (e.g., chi-dn-dzd1, chi-dn-dzd2)
/// - Links: {device1}:{device2} format (e.g., lax-dz001:nyc-dz001)

#[test]
fn test_testnet_device_names_accepted() {
    // Real device names from testnet telemetry
    let devices = vec![
        Device::new("lax-dz001".to_string(), 10, "Alpha".to_string()),
        Device::new("nyc-dz001".to_string(), 10, "Beta".to_string()),
        Device::new("lon-dz001".to_string(), 10, "Gamma".to_string()),
        Device::new("tyo-dz001".to_string(), 10, "Delta".to_string()),
    ];

    // Create links between these devices
    let private_links = vec![
        PrivateLink::new(
            "lax-dz001".to_string(),
            "nyc-dz001".to_string(),
            68.4,
            100.0,
            1.0,
            None,
        ),
        PrivateLink::new(
            "nyc-dz001".to_string(),
            "lon-dz001".to_string(),
            70.0,
            100.0,
            1.0,
            None,
        ),
        PrivateLink::new(
            "lax-dz001".to_string(),
            "tyo-dz001".to_string(),
            100.0,
            100.0,
            1.0,
            None,
        ),
    ];

    // Cities for public links (no digits)
    let public_links = vec![
        PublicLink::new("LAX".to_string(), "NYC".to_string(), 100.0),
        PublicLink::new("NYC".to_string(), "LON".to_string(), 100.0),
        PublicLink::new("LAX".to_string(), "TYO".to_string(), 100.0),
    ];

    let demands = vec![Demand::new(
        "LAX".to_string(),
        "NYC".to_string(),
        1,     // receivers
        100.0, // traffic
        1.0,   // priority
        1,     // kind
        false, // multicast
    )];

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 1.0,
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    // Should pass validation
    let result = input.compute();
    if let Err(e) = &result {
        eprintln!("Validation error: {e:?}");
    }
    assert!(result.is_ok(), "Testnet device names should be valid");
}

#[test]
fn test_devnet_device_names_accepted() {
    // Real device names from devnet telemetry
    let devices = vec![
        Device::new("chi-dn-dzd1".to_string(), 10, "Alpha".to_string()),
        Device::new("chi-dn-dzd2".to_string(), 10, "Beta".to_string()),
        Device::new("chi-dn-dzd3".to_string(), 10, "Gamma".to_string()),
        Device::new("chi-dn-dzd4".to_string(), 10, "Delta".to_string()),
    ];

    // Create links between these devices
    let private_links = vec![
        PrivateLink::new(
            "chi-dn-dzd1".to_string(),
            "chi-dn-dzd2".to_string(),
            178.0,
            100.0,
            1.0,
            None,
        ),
        PrivateLink::new(
            "chi-dn-dzd1".to_string(),
            "chi-dn-dzd3".to_string(),
            180.0,
            100.0,
            1.0,
            None,
        ),
        PrivateLink::new(
            "chi-dn-dzd2".to_string(),
            "chi-dn-dzd4".to_string(),
            171.0,
            100.0,
            1.0,
            None,
        ),
        PrivateLink::new(
            "chi-dn-dzd3".to_string(),
            "chi-dn-dzd4".to_string(),
            170.0,
            100.0,
            1.0,
            None,
        ),
    ];

    // Cities for public links (no digits)
    let public_links = vec![PublicLink::new("CHI".to_string(), "NYC".to_string(), 100.0)];

    let demands = vec![Demand::new(
        "CHI".to_string(),
        "NYC".to_string(),
        1,     // receivers
        100.0, // traffic
        1.0,   // priority
        1,     // kind
        false, // multicast
    )];

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 1.0,
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    // Should pass validation
    let result = input.compute();
    assert!(result.is_ok(), "Devnet device names should be valid");
}

#[test]
fn test_mixed_format_device_names() {
    // Test that various hyphenated formats with digits are accepted
    let test_cases = vec![
        "nyc-dz001",    // testnet format
        "chi-dn-dzd1",  // devnet format
        "sin-prod-001", // hypothetical prod format
        "fra-test-x1",  // mixed letters and numbers
        "ams-v2-node3", // version format
    ];

    for device_name in test_cases {
        let devices = vec![
            Device::new(device_name.to_string(), 10, "TestOp".to_string()),
            Device::new("NYC1".to_string(), 10, "TestOp".to_string()), // standard format
        ];

        let private_links = vec![PrivateLink::new(
            device_name.to_string(),
            "NYC1".to_string(),
            50.0,
            100.0,
            1.0,
            None,
        )];

        let public_links = vec![PublicLink::new("NYC".to_string(), "LON".to_string(), 100.0)];

        let demands = vec![Demand::new(
            "NYC".to_string(),
            "LON".to_string(),
            1,     // receivers
            100.0, // traffic
            1.0,   // priority
            1,     // kind
            false, // multicast
        )];

        let input = ShapleyInput {
            private_links,
            devices,
            demands,
            public_links,
            operator_uptime: 1.0,
            contiguity_bonus: 0.0,
            demand_multiplier: 1.0,
        };

        let result = input.compute();
        assert!(
            result.is_ok(),
            "Device name '{device_name}' should be valid",
        );
    }
}

#[test]
fn test_device_names_without_digits_are_allowed() {
    // Device names may omit digits; ensure they pass validation
    let devices = vec![
        Device::new("nyc-dz-abc".to_string(), 10, "Alpha".to_string()), // no digits
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ];

    let private_links = vec![PrivateLink::new(
        "nyc-dz-abc".to_string(),
        "LON1".to_string(),
        50.0,
        100.0,
        1.0,
        None,
    )];

    let public_links = vec![PublicLink::new("NYC".to_string(), "LON".to_string(), 100.0)];

    let demands = vec![Demand::new(
        "NYC".to_string(),
        "LON".to_string(),
        1,     // receivers
        100.0, // traffic
        1.0,   // priority
        1,     // kind
        false, // multicast
    )];

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 1.0,
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    let result = input.compute();
    assert!(result.is_ok());
}
