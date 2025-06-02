//! Network Shapley value computation library
//!
//! This library implements the Shapley value computation for network optimization problems,
//! determining fair allocation of value among network operators based on their contributions.

pub mod coalition_computation;
pub mod error;
pub mod link_preparation;
pub mod lp;
pub mod lp_construction;
pub mod network_shapley;
pub mod types;
pub mod validation;

// Re-export main types and functions
pub use error::{Result, ShapleyError};
pub use network_shapley::{NetworkShapley, NetworkShapleyBuilder};
pub use types::{
    Demand, DemandMatrix, Link, LinkBuilder, PrivateLinks, PublicLinks, ShapleyValue,
    decimal_to_f64, f64_to_decimal, round_decimal,
};
