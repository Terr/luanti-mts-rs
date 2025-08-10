#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Schematic has too many or too few nodes: {found} instead of {expected}")]
    IncorrectNodeCount { found: usize, expected: usize },
    #[error("Number of layer probabilities does not match number of layers")]
    IncorrectNumberOfLayerProbabilities,
    #[error("Invalid content index: {0}")]
    InvalidContentIndex(u16),
    #[error("Unregistered content name: {0}")]
    InvalidContentName(String),
    #[error("Out of bounds")]
    OutOfBounds,
    #[error("Parse error: {0}")]
    ParseError(winnow::error::ContextError),
}

impl From<winnow::error::ContextError> for Error {
    fn from(error: winnow::error::ContextError) -> Self {
        Error::ParseError(error)
    }
}

impl From<&winnow::error::ContextError> for Error {
    fn from(error: &winnow::error::ContextError) -> Self {
        Error::ParseError(error.clone())
    }
}
