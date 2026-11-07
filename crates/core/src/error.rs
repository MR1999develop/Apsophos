use thiserror::Error;

/// Core errors. Messages are written for the end user.
#[derive(Debug, Error)]
pub enum DenoiseError {
    #[error("processing cancelled")]
    Cancelled,
    #[error("invalid size: {0}")]
    InvalidSize(String),
    #[error("value out of range: {0}")]
    OutOfRange(f32),
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Inference engine error, already worded for the user.
#[derive(Debug, Error)]
#[error("the neural network could not process the image: {0}")]
pub struct ModelError(pub String);
