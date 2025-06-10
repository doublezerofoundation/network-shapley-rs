use crate::{
    network_shapley::NetworkShapleyBuilderError,
    types::{DemandBuilderError, LinkBuilderError},
};
use thiserror::Error;

/// Error types for the Shapley computation system
#[derive(Debug, Error)]
pub enum ShapleyError {
    /// Missing required columns in link data
    #[error("Missing required columns for {link_type} links: {columns:?}")]
    MissingColumns {
        link_type: String,
        columns: Vec<String>,
    },

    /// Invalid switch naming (missing digits)
    #[error(
        "Switches are not labeled correctly in {link_type} links; they should be denoted with an integer."
    )]
    InvalidSwitchNaming { link_type: String },

    /// Invalid endpoint naming (has digits)
    #[error(
        "Endpoints are not labeled correctly in the demand matrix; they should not have an integer."
    )]
    InvalidEndpointNaming,

    /// Multiple sources for single traffic type
    #[error("All traffic of a single type must have a single source.")]
    MultipleTrafficSources,

    /// Incomplete public pathway coverage
    #[error("The public pathway is not fully specified for {location}. Missing: {missing:?}")]
    IncompletePublicPathway {
        location: String,
        missing: Vec<String>,
    },

    /// Reserved operator name
    #[error("0 is a protected keyword for operator names; choose another.")]
    ReservedOperatorName,

    /// Too many operators
    #[error("There are too many operators; we limit to 15 to prevent the program from crashing.")]
    TooManyOperators,

    /// Empty links
    #[error("There must be at least one {link_type} link.")]
    EmptyLinks { link_type: String },

    /// Linear programming solver failure
    #[error("Linear programming failed: {reason}")]
    LPSolveFailed { reason: String },

    /// Decimal conversion error
    #[error("Decimal conversion error: {0}")]
    DecimalError(#[from] rust_decimal::Error),

    #[error("NetworkShapley configuration build error: {0}")]
    NetworkShapleyBuild(#[from] NetworkShapleyBuilderError),

    #[error("Link configuration build error: {0}")]
    LinkBuild(#[from] LinkBuilderError),

    #[error("Demand configuration build error: {0}")]
    DemandBuild(#[from] DemandBuilderError),

    /// Generic computation error
    #[error("Computation error: {0}")]
    ComputationError(String),
}

/// Result type alias for Shapley operations
pub type Result<T> = std::result::Result<T, ShapleyError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        // Test MissingColumns
        let err = ShapleyError::MissingColumns {
            link_type: "private".to_string(),
            columns: vec!["Cost".to_string(), "Bandwidth".to_string()],
        };
        assert_eq!(
            err.to_string(),
            "Missing required columns for private links: [\"Cost\", \"Bandwidth\"]"
        );

        // Test InvalidSwitchNaming
        let err = ShapleyError::InvalidSwitchNaming {
            link_type: "public".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Switches are not labeled correctly in public links; they should be denoted with an integer."
        );

        // Test InvalidEndpointNaming
        let err = ShapleyError::InvalidEndpointNaming;
        assert_eq!(
            err.to_string(),
            "Endpoints are not labeled correctly in the demand matrix; they should not have an integer."
        );

        // Test MultipleTrafficSources
        let err = ShapleyError::MultipleTrafficSources;
        assert_eq!(
            err.to_string(),
            "All traffic of a single type must have a single source."
        );

        // Test IncompletePublicPathway
        let err = ShapleyError::IncompletePublicPathway {
            location: "all the switches".to_string(),
            missing: vec!["NYC1".to_string(), "LAX1".to_string()],
        };
        assert_eq!(
            err.to_string(),
            "The public pathway is not fully specified for all the switches. Missing: [\"NYC1\", \"LAX1\"]"
        );

        // Test ReservedOperatorName
        let err = ShapleyError::ReservedOperatorName;
        assert_eq!(
            err.to_string(),
            "0 is a protected keyword for operator names; choose another."
        );

        // Test TooManyOperators
        let err = ShapleyError::TooManyOperators;
        assert_eq!(
            err.to_string(),
            "There are too many operators; we limit to 15 to prevent the program from crashing."
        );

        // Test EmptyLinks
        let err = ShapleyError::EmptyLinks {
            link_type: "private".to_string(),
        };
        assert_eq!(err.to_string(), "There must be at least one private link.");

        // Test LPSolveFailed
        let err = ShapleyError::LPSolveFailed {
            reason: "infeasible".to_string(),
        };
        assert_eq!(err.to_string(), "Linear programming failed: infeasible");
    }
}
