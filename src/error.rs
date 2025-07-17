#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Schematic has too many or too few nodes: {found} instead of {expected}")]
    IncorrectNodeCount { found: usize, expected: usize },
    #[error("Number of layer probabilities doesn't match number of layers")]
    IncorrectNumberOfLayerProbabilities,
    #[error("Invalid content name index: {0}")]
    InvalidContentNameIndex(u16),
    #[error("Out of bounds")]
    OutOfBounds,
}
