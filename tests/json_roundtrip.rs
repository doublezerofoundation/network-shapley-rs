#![cfg(feature = "serde")]

use network_shapley::{
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

/// Round-trip: construct ShapleyInput -> serialize to JSON -> deserialize back -> assert fields match.
#[test]
fn json_round_trip() {
    let input = ShapleyInput {
        devices: vec![
            Device::new("SIN1".into(), 1, "OpA".into()),
            Device::new("FRA1".into(), 1, "OpB".into()),
        ],
        private_links: vec![
            PrivateLink::new("SIN1".into(), "FRA1".into(), 10.0, 100.0, 0.99, None),
            PrivateLink::new("SIN1".into(), "FRA1".into(), 10.0, 100.0, 0.99, Some(1)),
        ],
        public_links: vec![PublicLink::new("SIN".into(), "FRA".into(), 50.0)],
        demands: vec![
            Demand::new("SIN".into(), "FRA".into(), 1, 10.0, 1.0, 1, false),
            Demand::new("SIN".into(), "FRA".into(), 2, 20.0, 1.0, 1, true),
        ],
        operator_uptime: 1.0,
        contiguity_bonus: 0.0,
        demand_multiplier: 1.0,
    };

    let json = serde_json::to_string(&input).unwrap();
    let deserialized: ShapleyInput = serde_json::from_str(&json).unwrap();

    // Verify private links shared field round-trips
    assert_eq!(deserialized.private_links[0].shared, None);
    assert_eq!(deserialized.private_links[1].shared, Some(1));

    // Verify demands multicast field round-trips
    assert!(!deserialized.demands[0].multicast);
    assert!(deserialized.demands[1].multicast);

    // Verify scalar fields
    assert_eq!(deserialized.devices.len(), 2);
    assert_eq!(deserialized.public_links.len(), 1);
    assert!((deserialized.operator_uptime - 1.0).abs() < f64::EPSILON);
}

/// Parse a hand-written JSON string with native JSON types (null, integers, booleans).
#[test]
fn json_native_types() {
    let json = r#"{
        "devices": [
            {"device": "SIN1", "edge": 1, "operator": "OpA"},
            {"device": "FRA1", "edge": 1, "operator": "OpB"}
        ],
        "private_links": [
            {"device1": "SIN1", "device2": "FRA1", "latency": 10.0, "bandwidth": 100.0, "uptime": 0.99, "shared": null},
            {"device1": "SIN1", "device2": "FRA1", "latency": 10.0, "bandwidth": 100.0, "uptime": 0.99, "shared": 1}
        ],
        "public_links": [
            {"city1": "SIN", "city2": "FRA", "latency": 50.0}
        ],
        "demands": [
            {"start": "SIN", "end": "FRA", "receivers": 1, "traffic": 10.0, "priority": 1.0, "type": 1, "multicast": false},
            {"start": "SIN", "end": "FRA", "receivers": 2, "traffic": 20.0, "priority": 1.0, "type": 1, "multicast": true}
        ],
        "operator_uptime": 1.0,
        "contiguity_bonus": 0.0,
        "demand_multiplier": 1.0
    }"#;

    let input: ShapleyInput = serde_json::from_str(json).unwrap();

    assert_eq!(input.private_links[0].shared, None);
    assert_eq!(input.private_links[1].shared, Some(1));
    assert!(!input.demands[0].multicast);
    assert!(input.demands[1].multicast);
    assert_eq!(input.devices.len(), 2);
    assert_eq!(input.demands.len(), 2);
}
