#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSerialize};

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
fn deser_shared<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let s = <String as serde::Deserialize>::deserialize(deserializer)?;
    if s == "NA" || s.is_empty() {
        Ok(None)
    } else {
        s.parse::<T>().map(Some).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "serde")]
fn deser_multicast<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = <String as serde::Deserialize>::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "invalid multicast boolean value: {other}",
        ))),
    }
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
