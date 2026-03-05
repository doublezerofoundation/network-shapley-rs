#![cfg(feature = "serde")]

use network_shapley::{
    shapley::ShapleyInput,
    types::{Demand, Device, PrivateLink, PublicLink},
};

fn create_basic_devices() -> Vec<Device> {
    vec![
        Device::new("SIN1".into(), 1, "OpA".into()),
        Device::new("FRA1".into(), 1, "OpB".into()),
    ]
}

fn create_basic_private_links() -> Vec<PrivateLink> {
    vec![
        PrivateLink::new("SIN1".into(), "FRA1".into(), 10.0, 100.0, 0.99, None),
        PrivateLink::new("SIN1".into(), "FRA1".into(), 10.0, 100.0, 0.99, Some(1)),
    ]
}

fn create_basic_public_links() -> Vec<PublicLink> {
    vec![PublicLink::new("SIN".into(), "FRA".into(), 50.0)]
}

fn create_basic_demands() -> Vec<Demand> {
    vec![
        Demand::new("SIN".into(), "FRA".into(), 1, 10.0, 1.0, 1, false),
        Demand::new("SIN".into(), "FRA".into(), 2, 20.0, 1.0, 1, true),
    ]
}

/// Builds a minimal JSON input string with the given raw `shared` and `multicast` values.
/// Use this to test specific JSON representations of those fields without repeating boilerplate.
fn make_json(shared: &str, multicast: &str) -> String {
    format!(
        r#"{{
        "devices": [{{"device": "SIN1", "edge": 1, "operator": "OpA"}}],
        "private_links": [{{"device1": "SIN1", "device2": "FRA1", "latency": 10.0, "bandwidth": 100.0, "uptime": 0.99, "shared": {shared}}}],
        "public_links": [{{"city1": "SIN", "city2": "FRA", "latency": 50.0}}],
        "demands": [{{"start": "SIN", "end": "FRA", "receivers": 1, "traffic": 10.0, "priority": 1.0, "type": 1, "multicast": {multicast}}}],
        "operator_uptime": 1.0,
        "contiguity_bonus": 0.0,
        "demand_multiplier": 1.0
    }}"#
    )
}

/// Round-trip: construct ShapleyInput -> serialize to JSON -> deserialize back -> assert fields match.
#[test]
fn json_round_trip() {
    let input = ShapleyInput {
        devices: create_basic_devices(),
        private_links: create_basic_private_links(),
        public_links: create_basic_public_links(),
        demands: create_basic_demands(),
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

/// deser_shared: string "NA" maps to None.
#[test]
fn shared_string_na() {
    let input: ShapleyInput = serde_json::from_str(&make_json(r#""NA""#, "false")).unwrap();
    assert_eq!(input.private_links[0].shared, None);
}

/// deser_shared: empty string maps to None.
#[test]
fn shared_string_empty() {
    let input: ShapleyInput = serde_json::from_str(&make_json(r#""""#, "false")).unwrap();
    assert_eq!(input.private_links[0].shared, None);
}

/// deser_shared: numeric string parses to Some(n).
#[test]
fn shared_string_numeric() {
    let input: ShapleyInput = serde_json::from_str(&make_json(r#""7""#, "false")).unwrap();
    assert_eq!(input.private_links[0].shared, Some(7));
}

/// deser_shared: invalid string returns an error.
#[test]
fn shared_string_invalid() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json(r#""bad""#, "false")).is_err());
}

/// deser_shared: negative integer returns an error.
#[test]
fn shared_negative_int() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json("-1", "false")).is_err());
}

/// deser_shared: u64 value out of u32 range returns an error.
#[test]
fn shared_out_of_range() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json("4294967296", "false")).is_err());
}

/// deser_shared: unexpected type (float) returns an error via the visitor's expecting() message.
#[test]
fn shared_unexpected_type() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json("1.5", "false")).is_err());
}

/// deser_multicast: string "true" and "False" (case-insensitive) parse correctly.
#[test]
fn multicast_string_true_false() {
    let input: ShapleyInput = serde_json::from_str(&make_json("null", r#""true""#)).unwrap();
    assert!(input.demands[0].multicast);

    let input: ShapleyInput = serde_json::from_str(&make_json("null", r#""False""#)).unwrap();
    assert!(!input.demands[0].multicast);
}

/// deser_multicast: invalid string returns an error.
#[test]
fn multicast_string_invalid() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json("null", r#""yes""#)).is_err());
}

/// deser_multicast: unexpected type (integer) returns an error via the visitor's expecting() message.
#[test]
fn multicast_unexpected_type() {
    assert!(serde_json::from_str::<ShapleyInput>(&make_json("null", "1")).is_err());
}
