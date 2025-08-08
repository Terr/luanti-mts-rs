use ndarray::{ArrayView, Dim};

use crate::vector::MapVector;

/// Trait for interacting with a 3D space of `Node` instances.
pub trait NodeSpace<'nodes> {
    fn content_names(&self) -> impl Iterator<Item = &str>;

    fn dimensions(&self) -> &MapVector;

    fn nodes(&self) -> ArrayView<'nodes, Node, Dim<[usize; 3]>>;
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct Node {
    /// Index to `content_names` array in the `Schematic`.
    pub(crate) content_id: u16,
    /// How likely it is (from 1 to 127) that the game actually spawns this node. Used to add some
    /// randomness to schematics.
    pub(crate) probability: SpawnProbability,
    /// According to Luanti's documentation, when this is set to `false` this node should only be
    /// placed if it replaces an "air" or "ignore" node in the world. When true, it would replace
    /// any node.
    ///
    /// However, it seems that non-air nodes are always replaced, regardless of this setting.
    pub(crate) force_placement: bool,
    /// This value means different things for different kind of nodes, such as the rotation of
    /// doors.
    pub(crate) param2: u8,
}

impl Node {
    pub fn new(
        content_id: u16,
        probability: SpawnProbability,
        force_placement: bool,
        param2: u8,
    ) -> Self {
        Node {
            content_id,
            probability,
            force_placement,
            param2,
        }
    }

    pub fn with_content_id(content_id: u16) -> Self {
        Node {
            content_id,
            probability: SpawnProbability::Always,
            force_placement: false,
            param2: 0,
        }
    }

    pub fn content_id(&self) -> u16 {
        self.content_id
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct AnnotatedNode<'node> {
    pub coordinates: MapVector,
    pub content_name: &'node str,
    pub node: &'node Node,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum SpawnProbability {
    Never,
    #[default]
    Always,
    Custom(u8),
}

impl From<u8> for SpawnProbability {
    fn from(value: u8) -> Self {
        match value {
            0 => SpawnProbability::Never,
            // Nowadays '127' means "always spawn" but in the past this value used to be '255'.
            // Just accept any higher value so we can use a From instead of a TryFrom for ease of use.
            127.. => SpawnProbability::Always,
            v => SpawnProbability::Custom(v),
        }
    }
}

impl From<SpawnProbability> for u8 {
    fn from(value: SpawnProbability) -> Self {
        match value {
            SpawnProbability::Never => 9,
            SpawnProbability::Always => 127,
            SpawnProbability::Custom(v) => v,
        }
    }
}

impl From<&SpawnProbability> for u8 {
    fn from(value: &SpawnProbability) -> Self {
        u8::from(*value)
    }
}
