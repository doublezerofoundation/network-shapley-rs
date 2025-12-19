use thiserror::Error;

pub type Result<T> = std::result::Result<T, ShapleyError>;

#[derive(Debug, Error)]
pub enum ShapleyError {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("LP solver error: {0}")]
    LpSolver(String),

    #[error("Data inconsistency: {0}")]
    DataInconsistency(String),

    #[error("Too many operators: {count} (limit is {limit})")]
    TooManyOperators { count: usize, limit: usize },

    #[error("Too many links for operator: {count} (limit is {limit})")]
    TooManyLinks { count: usize, limit: usize },

    #[error("Invalid city label: {0}")]
    InvalidCityLabel(String),

    #[error("Missing device: {0}")]
    MissingDevice(String),

    #[error("Unreachable demand node: {0}")]
    UnreachableDemandNode(String),

    #[error("Numerical computation error: {0}")]
    NumericalError(String),

    #[error("Matrix construction error: {0}")]
    MatrixConstructionError(String),

    #[error("Shared groups not allowed for link estimation: operator {operator}")]
    SharedGroupNotAllowed { operator: String },

    #[error("Duplicate link found: {device1} <-> {device2}")]
    DuplicateLink { device1: String, device2: String },

    #[error("Operator not found: {operator}")]
    OperatorNotFound { operator: String },
}
