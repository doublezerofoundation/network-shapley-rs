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

    #[error("Invalid device label: {0}")]
    InvalidDeviceLabel(String),

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
}
