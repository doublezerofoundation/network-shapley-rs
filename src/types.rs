use derive_builder::Builder;
use faer::{Col, sparse::SparseColMat};
use rust_decimal::Decimal;

#[cfg(feature = "csv")]
use {
    rust_decimal::dec,
    serde::{Deserialize, Serialize},
    tabled::Tabled,
};

pub type Result<T> = std::result::Result<T, crate::error::ShapleyError>;

/// Represents a network link between two nodes
#[derive(Debug, Clone, PartialEq, Builder)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize))]
pub struct Link {
    pub start: String,
    pub end: String,
    #[builder(default = "Decimal::ZERO")]
    pub cost: Decimal,
    #[builder(default = "Decimal::ZERO")]
    pub bandwidth: Decimal,
    #[builder(default = "0.to_string()")]
    pub operator1: String,
    #[builder(default = "0.to_string()")]
    pub operator2: String,
    #[builder(default = "Decimal::ONE")]
    pub uptime: Decimal,
    #[builder(default = "0")]
    pub shared: usize,
    #[builder(default = "0")]
    pub link_type: usize,
}

/// Represents traffic demand between two endpoints
#[derive(Debug, Clone, PartialEq, Builder)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize))]
pub struct Demand {
    pub start: String,
    pub end: String,
    pub traffic: Decimal,
    pub demand_type: usize,
}

/// Represents a Shapley value result for an operator
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "csv", derive(Serialize, Deserialize, Tabled))]
pub struct ShapleyValue {
    pub operator: String,
    pub value: Decimal,
    #[cfg_attr(feature = "csv", tabled(display = "display_as_percent"))]
    pub percent: Decimal,
}

#[cfg(feature = "csv")]
fn display_as_percent(percent: &Decimal) -> String {
    format!("{:.2}%", percent * dec!(100))
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
#[derive(Default, Debug, Clone)]
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
#[derive(Default, Debug, Clone)]
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
#[derive(Default, Debug, Clone)]
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

                let link = LinkBuilder::default()
                    .start(record.start)
                    .end(record.end)
                    .cost(record.cost)
                    .bandwidth(record.bandwidth)
                    .operator1(record.operator1)
                    .operator2(record.operator2)
                    .uptime(record.uptime)
                    .shared(record.shared.unwrap_or(0))
                    .build()
                    .unwrap();

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

                let link = LinkBuilder::default()
                    .start(record.start)
                    .end(record.end)
                    .cost(record.cost)
                    .build()
                    .unwrap();

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

                let demand = DemandBuilder::default()
                    .start(record.start)
                    .end(record.end)
                    .traffic(record.traffic)
                    .demand_type(record.demand_type)
                    .build()?;

                demands.push(demand);
            }

            Ok(DemandMatrix::from_demands(demands))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{f64_to_decimal, round_decimal, utils::decimal_to_f64};
    use rust_decimal::dec;

    #[test]
    fn test_link_creation() {
        let link = LinkBuilder::default()
            .start("NYC1".to_string())
            .end("LAX1".to_string())
            .build()
            .unwrap();
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
        let demand = DemandBuilder::default()
            .start("NYC".to_string())
            .end("LAX".to_string())
            .traffic(dec!(100))
            .demand_type(1)
            .build()
            .unwrap();
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
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(dec!(10))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("CHI".to_string())
                .traffic(dec!(20))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("LAX".to_string())
                .end("CHI".to_string())
                .traffic(dec!(30))
                .demand_type(2)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("CHI".to_string())
                .end("NYC".to_string())
                .traffic(dec!(40))
                .demand_type(3)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("LAX".to_string())
                .end("NYC".to_string())
                .traffic(dec!(50))
                .demand_type(2)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);

        let types = matrix.unique_types();
        assert_eq!(types, vec![1, 2, 3]);
    }

    #[test]
    fn test_private_links_operations() {
        let links = vec![
            LinkBuilder::default()
                .start("A".to_string())
                .end("B".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("B".to_string())
                .end("C".to_string())
                .build()
                .unwrap(),
        ];
        let private_links = PrivateLinks::from_links(links);

        assert_eq!(private_links.len(), 2);
        assert!(!private_links.is_empty());

        let empty = PrivateLinks::from_links(vec![]);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }
}
