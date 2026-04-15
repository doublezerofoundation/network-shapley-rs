#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSerialize};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

pub type Demands = Vec<Demand>;
pub type Devices = Vec<Device>;
pub type PrivateLinks = Vec<PrivateLink>;
pub type PublicLinks = Vec<PublicLink>;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[derive(Debug, Clone)]
pub struct Device {
    pub device: String,
    pub edge: u32,
    pub operator: String,
}

impl Device {
    pub fn new(device: String, edge: u32, operator: String) -> Self {
        Self {
            device,
            edge,
            operator,
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[derive(Debug, Clone)]
pub struct PrivateLink {
    pub device1: String,
    pub device2: String,
    pub latency: f64,
    pub bandwidth: f64,
    pub uptime: f64,
    #[cfg_attr(feature = "serde", serde(deserialize_with = "deser_shared"))]
    pub shared: Option<u32>,
}

#[cfg(feature = "serde")]
fn deser_shared<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SharedVisitor;

    impl<'de> serde::de::Visitor<'de> for SharedVisitor {
        type Value = Option<u32>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("null, an integer, or a string (\"NA\"/empty for None)")
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(self)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            u32::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("shared value {v} out of u32 range")))
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            u32::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("shared value {v} out of u32 range")))
        }

        fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
            if s == "NA" || s.is_empty() {
                Ok(None)
            } else {
                s.parse::<u32>().map(Some).map_err(E::custom)
            }
        }
    }

    deserializer.deserialize_any(SharedVisitor)
}

#[cfg(feature = "serde")]
fn deser_multicast<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    struct MulticastVisitor;

    impl<'de> serde::de::Visitor<'de> for MulticastVisitor {
        type Value = bool;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a boolean or a string (\"true\"/\"false\")")
        }

        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
            match s.to_lowercase().as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                other => Err(E::custom(format!(
                    "invalid multicast boolean value: {other}",
                ))),
            }
        }
    }

    deserializer.deserialize_any(MulticastVisitor)
}

impl PrivateLink {
    pub fn new(
        device1: String,
        device2: String,
        latency: f64,
        bandwidth: f64,
        uptime: f64,
        shared: Option<u32>,
    ) -> Self {
        Self {
            device1,
            device2,
            latency,
            bandwidth,
            uptime,
            shared,
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[derive(Debug, Clone)]
pub struct PublicLink {
    pub city1: String,
    pub city2: String,
    pub latency: f64,
}

impl PublicLink {
    pub fn new(city1: String, city2: String, latency: f64) -> Self {
        Self {
            city1,
            city2,
            latency,
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[derive(Debug, Clone)]
pub struct Demand {
    pub start: String,
    pub end: String,
    pub receivers: u32,
    pub traffic: f64,
    pub priority: f64,
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    pub kind: u32, // aka type
    #[cfg_attr(feature = "serde", serde(deserialize_with = "deser_multicast"))]
    pub multicast: bool,
}

impl Demand {
    pub fn new(
        start: String,
        end: String,
        receivers: u32,
        traffic: f64,
        priority: f64,
        kind: u32,
        multicast: bool,
    ) -> Self {
        Self {
            start,
            end,
            receivers,
            traffic,
            priority,
            kind,
            multicast,
        }
    }
}

// Consolidated types for internal processing
#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedDemand {
    pub start: String,
    pub end: String,
    pub receivers: u32,
    pub traffic: f64,
    pub priority: f64,
    pub kind: u32,
    pub multicast: bool,
    pub original: u32, // Original type before adjustment
}

#[derive(Debug, Clone)]
pub(crate) struct ConsolidatedLink {
    pub device1: String,
    pub device2: String,
    pub latency: f64,
    pub bandwidth: f64,
    pub operator1: String,
    pub operator2: String,
    pub shared: u32,
    pub link_type: u32, // 0 for all traffic types, specific type otherwise
}
