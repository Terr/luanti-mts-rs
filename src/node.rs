use std::borrow::Cow;

use ndarray::ArrayView3;

use crate::error::Error;
use crate::vector::MapVector;

/// Trait for interacting with a 3D space of nodes.
pub trait NodeSpace<'nodes> {
    /// Iterator for the content names that nodes (can) use.
    fn content_names(&'nodes self) -> impl Iterator<Item = &'nodes str>;

    /// Returns the content ID for the given content `name`. Used by [RawNode]s to point to point
    /// to their contents.
    fn content_id_for_name(&'nodes self, name: &str) -> Option<u16>;

    /// Returns the content name for the given content `id`.
    fn content_name_for_id(&'nodes self, id: u16) -> Option<&'nodes str>;

    /// Returns the size of the node space in 3D dimensions.
    fn dimensions(&'nodes self) -> MapVector;

    /// The number of nodes contained in this node space.
    fn num_nodes(&'nodes self) -> usize;

    /// Iterator for all [RawNode] contained in this node space.
    fn nodes(&'nodes self) -> ArrayView3<'nodes, RawNode>;

    /// Returns the node at the specified `coordinates` as a [Node].
    fn node_at(&'nodes self, coordinates: MapVector) -> Option<Node<'nodes>>;
}

#[derive(Debug, PartialEq, Eq, Clone, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Node<'name> {
    /// Name that identifies the content (material, item) of this `Node`.
    ///
    /// Can be either a reference or an owned String.
    pub content_name: Cow<'name, str>,
    /// How likely it is (from 1 to 127) that the game actually spawns this node. Used to add some
    /// randomness to schematics. Older versions of the game used 255 to indicate "always spawn".
    pub spawn_probability: SpawnProbability,
    /// According to Luanti's documentation, when this is set to `false` this node should only be
    /// placed if it replaces an "air" or "ignore" node in the world. When true, it would replace
    /// any node.
    ///
    /// However, it seems that non-air nodes are always replaced, regardless of this setting.
    pub force_placement: bool,
    /// This value means different things for different kind of nodes, such as the rotation of
    /// doors and stairs.
    pub(crate) param2: u8,
}

impl<'name> Node<'name> {
    pub fn new(
        content_name: Cow<'name, str>,
        spawn_probability: SpawnProbability,
        force_placement: bool,
        param2: u8,
    ) -> Self {
        Node {
            content_name,
            spawn_probability,
            force_placement,
            param2,
        }
    }

    pub fn with_content_name(content_name: Cow<'name, str>) -> Self {
        Node::new(content_name, SpawnProbability::Always, true, 0)
    }

    /// Converts this `Node` into a `RawNode`.
    ///
    /// This can fail if the `Node`'s content name cannot be found in the `schematic`.
    ///
    /// If you want ensure that the content is present in the schematic so that the conversion
    /// never fails, use `Schematic::convert_node_to_raw_node()`.
    pub fn to_raw_node(&self, schematic: &'name impl NodeSpace<'name>) -> Result<RawNode, Error> {
        let content_id = schematic
            .content_id_for_name(&self.content_name)
            .ok_or_else(|| Error::InvalidContentName(self.content_name.clone().into_owned()))?;

        Ok(RawNode::new(
            content_id,
            self.spawn_probability,
            self.force_placement,
            self.param2,
        ))
    }
}

/// A memory-efficient representation of a node in Luanti, which owns all its values and is
/// copyable.
///
/// Public interfaces use `Node` for ease of use, because they contain the full name of their
/// content, instead of the vague `content_id` of `RawNode`, which can mean different contents
/// depending on the `Schematic` the `RawNode` is placed in.
///
/// `RawNode` follows how Luanti stores nodes in schematics files very closely, except that the
/// data in this struct is (naturally) stored per node, where in MTS files each field is stored as
/// sequence of arrays (e.g. first all node contents, then param1 of all nodes, etc.)
#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RawNode {
    /// Index to `content_names` array in the `Schematic`.
    pub(crate) content_id: u16,
    /// How likely it is (from 1 to 127) that the game actually spawns this node. Used to add some
    /// randomness to schematics. Older versions of the game used 255 to indicate "always spawn".
    pub(crate) spawn_probability: u8,
    /// According to Luanti's documentation, when this is set to `false` this node should only be
    /// placed if it replaces an "air" or "ignore" node in the world. When true, it would replace
    /// any node.
    ///
    /// However, it seems that non-air nodes are always replaced, regardless of this setting.
    pub(crate) force_placement: bool,
    /// This value means different things for different kind of nodes, such as the rotation of
    /// doors and stairs.
    pub(crate) param2: u8,
}

impl RawNode {
    pub fn new(
        content_id: u16,
        spawn_probability: SpawnProbability,
        force_placement: bool,
        param2: u8,
    ) -> Self {
        RawNode {
            content_id,
            spawn_probability: spawn_probability.into(),
            force_placement,
            param2,
        }
    }

    pub fn with_content_id(content_id: u16) -> Self {
        RawNode {
            content_id,
            spawn_probability: SpawnProbability::Always.into(),
            force_placement: false,
            param2: 0,
        }
    }

    pub fn content_id(&self) -> u16 {
        self.content_id
    }

    pub fn to_node<'schematic>(
        &'schematic self,
        schematic: &'schematic impl NodeSpace<'schematic>,
    ) -> Result<Node<'schematic>, Error> {
        let content_name = schematic
            .content_name_for_id(self.content_id)
            .ok_or_else(|| Error::InvalidContentIndex(self.content_id))?;

        Ok(Node::new(
            content_name.into(),
            self.spawn_probability.into(),
            self.force_placement,
            self.param2,
        ))
    }
}

/// Used by [AnnotatedNodeIterator], combines a [Node] with its `coordinates` inside the
/// [Schematic]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AnnotatedNode<'node> {
    pub coordinates: MapVector,
    pub node: Node<'node>,
}

#[derive(Debug, Default, PartialEq, Eq, Copy, Clone, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg(test)]
mod tests {
    use crate::Schematic;

    use super::*;

    #[test]
    fn test_node_to_raw_node() {
        let mut schematic = Schematic::with_raw_nodes(
            (1, 1, 1).try_into().unwrap(),
            vec![RawNode::new(0, SpawnProbability::Always, true, 0)],
        )
        .unwrap();
        schematic.register_content("default:cobble".into());

        let node = Node::new(
            "default:cobble".to_string().into(),
            SpawnProbability::Always,
            true,
            0,
        );

        let raw_node = node.to_raw_node(&schematic).unwrap();

        assert_eq!(raw_node.content_id, 1);
    }

    #[test]
    fn test_send() {
        fn assert_send<T: Send>() {}
        assert_send::<RawNode>();
        assert_send::<Node>();
    }

    #[test]
    fn test_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<RawNode>();
        assert_sync::<Node>();
    }
}
