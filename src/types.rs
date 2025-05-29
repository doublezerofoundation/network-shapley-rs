use faer::{Col, sparse::SparseColMat};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};

#[cfg(feature = "csv")]
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, crate::error::ShapleyError>;

/// Represents a network link between two nodes
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize))]
pub struct Link {
    pub start: String,
    pub end: String,
    pub cost: Decimal,
    pub bandwidth: Decimal,
    pub operator1: String,
    pub operator2: String,
    pub uptime: Decimal,
    pub shared: usize,
    pub link_type: usize,
}

impl Link {
    /// Create a new link with default values
    pub fn new(start: String, end: String) -> Self {
        Link {
            start,
            end,
            cost: Decimal::ZERO,
            bandwidth: Decimal::ZERO,
            operator1: String::from("0"),
            operator2: String::from("0"),
            uptime: Decimal::ONE,
            shared: 0,
            link_type: 0,
        }
    }
}

/// Represents traffic demand between two endpoints
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize))]
pub struct Demand {
    pub start: String,
    pub end: String,
    pub traffic: Decimal,
    pub demand_type: usize,
}

impl Demand {
    /// Create a new demand entry
    pub fn new(start: String, end: String, traffic: Decimal, demand_type: usize) -> Self {
        Demand {
            start,
            end,
            traffic,
            demand_type,
        }
    }
}

/// Represents a Shapley value result for an operator
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize))]
pub struct ShapleyValue {
    pub operator: String,
    pub value: Decimal,
    pub percent: Decimal,
}

impl ShapleyValue {
    /// Create a new Shapley value result
    pub fn new(operator: String, value: Decimal, percent: Decimal) -> Self {
        ShapleyValue {
            operator,
            value,
            percent,
        }
    }
}

/// Linear Programming primitives for the optimization problem
#[derive(Debug)]
pub struct LPPrimitives {
    pub a_eq: SparseColMat<usize, f64>,
    pub a_ub: SparseColMat<usize, f64>,
    pub b_eq: Col<f64>,
    pub b_ub: Col<f64>,
    pub cost: Col<f64>,
    pub row_index1: Vec<String>,
    pub row_index2: Vec<String>,
    pub col_index1: Vec<String>,
    pub col_index2: Vec<String>,
}

/// Input data for private links
#[derive(Debug, Clone)]
pub struct PrivateLinks {
    pub links: Vec<Link>,
}

impl PrivateLinks {
    /// Create from a vector of links
    pub fn from_links(links: Vec<Link>) -> Self {
        PrivateLinks { links }
    }

    /// Get the number of links
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

/// Input data for public links
#[derive(Debug, Clone)]
pub struct PublicLinks {
    pub links: Vec<Link>,
}

impl PublicLinks {
    /// Create from a vector of links
    pub fn from_links(links: Vec<Link>) -> Self {
        PublicLinks { links }
    }

    /// Get the number of links
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

/// Input data for demand matrix
#[derive(Debug, Clone)]
pub struct DemandMatrix {
    pub demands: Vec<Demand>,
}

impl DemandMatrix {
    /// Create from a vector of demands
    pub fn from_demands(demands: Vec<Demand>) -> Self {
        DemandMatrix { demands }
    }

    /// Get the number of demand entries
    pub fn len(&self) -> usize {
        self.demands.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.demands.is_empty()
    }

    /// Get unique traffic types
    pub fn unique_types(&self) -> Vec<usize> {
        let mut types: Vec<usize> = self
            .demands
            .iter()
            .map(|d| d.demand_type)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        types
    }
}

/// Utility functions for Decimal/f64 conversion
#[inline]
pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

#[inline]
pub fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64(f).unwrap_or(Decimal::ZERO)
}

/// Round a Decimal to 4 decimal places (matching Python behavior)
#[inline]
pub fn round_decimal(d: Decimal) -> Decimal {
    d.round_dp(4)
}

#[cfg(feature = "csv")]
mod csv_support {
    use super::*;
    use serde::{Deserialize, Deserializer};
    use std::path::Path;

    fn deserialize_na_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        let s = String::deserialize(deserializer)?;
        if s == "NA" || s.is_empty() {
            Ok(None)
        } else {
            s.parse::<T>().map(Some).map_err(serde::de::Error::custom)
        }
    }

    fn deserialize_na_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(if s == "NA" { "0".to_string() } else { s })
    }

    #[derive(Debug, Deserialize)]
    struct PrivateLinkRecord {
        #[serde(rename = "Start")]
        start: String,
        #[serde(rename = "End")]
        end: String,
        #[serde(rename = "Cost")]
        cost: Decimal,
        #[serde(rename = "Bandwidth")]
        bandwidth: Decimal,
        #[serde(rename = "Operator1")]
        operator1: String,
        #[serde(rename = "Operator2", deserialize_with = "deserialize_na_string")]
        operator2: String,
        #[serde(rename = "Uptime")]
        uptime: Decimal,
        #[serde(rename = "Shared", deserialize_with = "deserialize_na_option")]
        shared: Option<usize>,
    }

    #[derive(Debug, Deserialize)]
    struct PublicLinkRecord {
        #[serde(rename = "Start")]
        start: String,
        #[serde(rename = "End")]
        end: String,
        #[serde(rename = "Cost")]
        cost: Decimal,
    }

    #[derive(Debug, Deserialize)]
    struct DemandRecord {
        #[serde(rename = "Start")]
        start: String,
        #[serde(rename = "End")]
        end: String,
        #[serde(rename = "Traffic")]
        traffic: Decimal,
        #[serde(rename = "Type")]
        demand_type: usize,
    }

    impl PrivateLinks {
        /// Read private links from a CSV file
        pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self> {
            let mut reader = csv::Reader::from_path(path)
                .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

            let mut links = Vec::new();
            for result in reader.deserialize() {
                let record: PrivateLinkRecord = result
                    .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

                let mut link = Link::new(record.start, record.end);
                link.cost = record.cost;
                link.bandwidth = record.bandwidth;
                link.operator1 = record.operator1;
                link.operator2 = record.operator2;
                link.uptime = record.uptime;
                link.shared = record.shared.unwrap_or(0);

                links.push(link);
            }

            Ok(PrivateLinks::from_links(links))
        }
    }

    impl PublicLinks {
        /// Read public links from a CSV file
        pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self> {
            let mut reader = csv::Reader::from_path(path)
                .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

            let mut links = Vec::new();
            for result in reader.deserialize() {
                let record: PublicLinkRecord = result
                    .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

                let mut link = Link::new(record.start, record.end);
                link.cost = record.cost;

                links.push(link);
            }

            Ok(PublicLinks::from_links(links))
        }
    }

    impl DemandMatrix {
        /// Read demand matrix from a CSV file
        pub fn from_csv<P: AsRef<Path>>(path: P) -> Result<Self> {
            let mut reader = csv::Reader::from_path(path)
                .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

            let mut demands = Vec::new();
            for result in reader.deserialize() {
                let record: DemandRecord = result
                    .map_err(|e| crate::error::ShapleyError::ComputationError(e.to_string()))?;

                let demand =
                    Demand::new(record.start, record.end, record.traffic, record.demand_type);

                demands.push(demand);
            }

            Ok(DemandMatrix::from_demands(demands))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_link_creation() {
        let link = Link::new("NYC1".to_string(), "LAX1".to_string());
        assert_eq!(link.start, "NYC1");
        assert_eq!(link.end, "LAX1");
        assert_eq!(link.cost, Decimal::ZERO);
        assert_eq!(link.bandwidth, Decimal::ZERO);
        assert_eq!(link.operator1, "0");
        assert_eq!(link.operator2, "0");
        assert_eq!(link.uptime, Decimal::ONE);
        assert_eq!(link.shared, 0);
        assert_eq!(link.link_type, 0);
    }

    #[test]
    fn test_demand_creation() {
        let demand = Demand::new("NYC".to_string(), "LAX".to_string(), dec!(100), 1);
        assert_eq!(demand.start, "NYC");
        assert_eq!(demand.end, "LAX");
        assert_eq!(demand.traffic, dec!(100));
        assert_eq!(demand.demand_type, 1);
    }

    #[test]
    fn test_shapley_value_creation() {
        let sv = ShapleyValue::new("Alpha".to_string(), dec!(24.9704), dec!(0.0722));
        assert_eq!(sv.operator, "Alpha");
        assert_eq!(sv.value, dec!(24.9704));
        assert_eq!(sv.percent, dec!(0.0722));
    }

    #[test]
    fn test_decimal_conversions() {
        // Test decimal to f64
        assert_eq!(decimal_to_f64(dec!(10.5)), 10.5);
        assert_eq!(decimal_to_f64(dec!(0)), 0.0);
        assert_eq!(decimal_to_f64(dec!(-5.25)), -5.25);

        // Test f64 to decimal
        assert_eq!(f64_to_decimal(10.5), dec!(10.5));
        assert_eq!(f64_to_decimal(0.0), dec!(0));
        assert_eq!(f64_to_decimal(-5.25), dec!(-5.25));
    }

    #[test]
    fn test_round_decimal() {
        assert_eq!(round_decimal(dec!(3.14159)), dec!(3.1416));
        assert_eq!(round_decimal(dec!(2.99995)), dec!(3.0000));
        assert_eq!(round_decimal(dec!(0.00001)), dec!(0.0000));
        assert_eq!(round_decimal(dec!(-1.23456)), dec!(-1.2346));
    }

    #[test]
    fn test_demand_matrix_unique_types() {
        let demands = vec![
            Demand::new("NYC".to_string(), "LAX".to_string(), dec!(10), 1),
            Demand::new("NYC".to_string(), "CHI".to_string(), dec!(20), 1),
            Demand::new("LAX".to_string(), "CHI".to_string(), dec!(30), 2),
            Demand::new("CHI".to_string(), "NYC".to_string(), dec!(40), 3),
            Demand::new("LAX".to_string(), "NYC".to_string(), dec!(50), 2),
        ];
        let matrix = DemandMatrix::from_demands(demands);

        let types = matrix.unique_types();
        assert_eq!(types, vec![1, 2, 3]);
    }

    #[test]
    fn test_private_links_operations() {
        let links = vec![
            Link::new("A".to_string(), "B".to_string()),
            Link::new("B".to_string(), "C".to_string()),
        ];
        let private_links = PrivateLinks::from_links(links);

        assert_eq!(private_links.len(), 2);
        assert!(!private_links.is_empty());

        let empty = PrivateLinks::from_links(vec![]);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }
}
