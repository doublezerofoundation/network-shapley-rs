use network_shapley::{
    error::ShapleyError,
    shapley::ShapleyInput,
    types::{Demand, Demands, Device, Devices, PrivateLink, PrivateLinks, PublicLink, PublicLinks},
};

fn create_basic_devices() -> Devices {
    vec![
        Device::new("NYC1".to_string(), 10, "Alpha".to_string()),
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ]
}

fn create_basic_private_links() -> PrivateLinks {
    vec![PrivateLink::new(
        "NYC1".to_string(),
        "LON1".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )]
}

fn create_basic_public_links() -> PublicLinks {
    vec![PublicLink::new("NYC".to_string(), "LON".to_string(), 100.0)]
}

fn create_basic_demands() -> Demands {
    vec![Demand::new(
        "NYC".to_string(),
        "LON".to_string(),
        1,
        1.0,
        1.0,
        1,
        false,
    )]
}

#[test]
fn test_public_operator_name_rejected() {
    let devices = vec![
        Device::new("NYC1".to_string(), 10, "Public".to_string()), // Invalid operator name
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ];
    let private_links = create_basic_private_links();
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::Validation(msg) => {
            assert!(msg.contains("Public is a protected keyword"));
        }
        _ => panic!("Expected validation error for Public operator name"),
    }
}

#[test]
fn test_too_many_operators_with_low_uptime() {
    let mut devices = vec![];
    // Create 16 operators (limit is 15 when uptime < 1.0)
    for i in 1..=16 {
        devices.push(Device::new(format!("NYC{}", i), 10, format!("Op{}", i)));
    }

    let private_links = vec![PrivateLink::new(
        "NYC1".to_string(),
        "NYC2".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )];
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 0.9, // Less than 1.0
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    let result = input.compute();
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::TooManyOperators { count, limit } => {
            assert_eq!(count, 16);
            assert_eq!(limit, 15);
        }
        _ => panic!("Expected TooManyOperators error"),
    }
}

#[test]
fn test_too_many_operators_with_full_uptime() {
    let mut devices = vec![];
    // Create 21 operators (limit is 20 when uptime = 1.0)
    for i in 1..=21 {
        devices.push(Device::new(format!("NYC{}", i), 10, format!("Op{}", i)));
    }

    let private_links = vec![PrivateLink::new(
        "NYC1".to_string(),
        "NYC2".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )];
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

    let input = ShapleyInput {
        private_links,
        devices,
        demands,
        public_links,
        operator_uptime: 1.0, // Full uptime
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    let result = input.compute();
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::TooManyOperators { count, limit } => {
            assert_eq!(count, 21);
            assert_eq!(limit, 20);
        }
        _ => panic!("Expected TooManyOperators error"),
    }
}

#[test]
fn test_empty_private_links_rejected() {
    let devices = create_basic_devices();
    let private_links = vec![]; // Empty private links
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::Validation(msg) => {
            assert!(msg.contains("at least one private link"));
        }
        _ => panic!("Expected validation error for empty private links"),
    }
}

#[test]
fn test_device_without_digit_rejected() {
    let devices = vec![
        Device::new("NYCX".to_string(), 10, "Alpha".to_string()), // No digit
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ];
    let private_links = vec![PrivateLink::new(
        "NYCX".to_string(), // Invalid device name
        "LON1".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )];
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::InvalidDeviceLabel(msg) => {
            assert!(msg.contains("NYCX"));
            assert!(msg.contains("should contain a digit"));
        }
        _ => panic!("Expected InvalidDeviceLabel error"),
    }
}

#[test]
fn test_device_with_00_code_rejected() {
    let devices = vec![
        Device::new("NYC00".to_string(), 10, "Alpha".to_string()), // Has 00 code
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ];
    let private_links = vec![PrivateLink::new(
        "NYC00".to_string(), // Invalid device name
        "LON1".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )];
    let demands = create_basic_demands();
    let public_links = create_basic_public_links();

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::InvalidDeviceLabel(msg) => {
            assert!(msg.contains("NYC00"));
            assert!(msg.contains("should not have a 00 code"));
        }
        _ => panic!("Expected InvalidDeviceLabel error"),
    }
}

// Note: 2-letter cities are allowed - removing this test

#[test]
fn test_city_with_digit_rejected() {
    let devices = create_basic_devices();
    let private_links = create_basic_private_links();
    let demands = create_basic_demands();
    let public_links = vec![
        PublicLink::new("NY1".to_string(), "LON".to_string(), 100.0), // City with digit
    ];

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::InvalidCityLabel(msg) => {
            assert!(msg.contains("NY1"));
            assert!(msg.contains("should not contain a digit"));
        }
        _ => panic!("Expected InvalidCityLabel error"),
    }
}

// Note: Empty public links are actually allowed - removing this test

#[test]
fn test_unreachable_demand_node() {
    let devices = vec![
        Device::new("NYC1".to_string(), 10, "Alpha".to_string()),
        Device::new("LON1".to_string(), 10, "Beta".to_string()),
    ];
    let private_links = vec![PrivateLink::new(
        "NYC1".to_string(),
        "LON1".to_string(),
        50.0,
        10.0,
        1.0,
        None,
    )];
    let demands = vec![Demand::new(
        "NYC".to_string(),
        "PAR".to_string(), // PAR is not connected
        1,
        1.0,
        1.0,
        1,
        false,
    )];
    let public_links = vec![PublicLink::new("NYC".to_string(), "LON".to_string(), 100.0)];

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
    assert!(result.is_err());
    match result.unwrap_err() {
        ShapleyError::UnreachableDemandNode(city) => {
            assert_eq!(city, "PAR");
        }
        _ => panic!("Expected UnreachableDemandNode error"),
    }
}
