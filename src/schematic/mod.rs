mod editing;
mod parser;
mod serializer;

use std::borrow::Cow;

use ndarray::{Array3, ArrayView3, Axis, Dim};

use crate::error::Error;
use crate::node::{AnnotatedNode, Node, NodeSpace, RawNode, SpawnProbability};
use crate::vector::MapVector;

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Schematic {
    pub(crate) version: u16,
    pub dimensions: MapVector,
    pub(crate) layer_probabilities: Vec<SpawnProbability>,
    /// Called "name ids" in the file format documentation, it's an array of strings that identify
    /// the contents of a node, i.e. the type of block or items like torches.
    ///
    /// Examples of names are: "air", "default:cobble", "mcl_core:quartz"
    pub(crate) content_names: Vec<String>,
    pub(crate) nodes: Array3<RawNode>,
}

impl Schematic {
    pub fn new(dimensions: MapVector) -> Result<Self, Error> {
        let nodes = vec![
            RawNode {
                content_id: 0, // "air"
                spawn_probability: SpawnProbability::Always.into(),
                force_placement: false,
                param2: 0
            };
            dimensions.volume()
        ];

        Self::with_raw_nodes(dimensions, nodes)
    }

    /// Since [RawNode] does not contain the actual content names, using this constructor requires
    /// the caller to `register_content()` all the content names that the [RawNode]s content IDs
    /// point to and update those IDs, if necessary.
    pub fn with_raw_nodes<T: Into<Vec<RawNode>>>(
        dimensions: MapVector,
        nodes: T,
    ) -> Result<Self, Error> {
        let nodes = nodes.into();
        let num_nodes = nodes.len();
        let nodes = Array3::from_shape_vec(dimensions.as_shape(), nodes).map_err(|_| {
            Error::IncorrectNodeCount {
                found: num_nodes,
                expected: dimensions.volume(),
            }
        })?;

        Ok(Self::with_array3(dimensions, nodes))
    }

    fn with_array3(dimensions: MapVector, nodes: Array3<RawNode>) -> Self {
        Schematic {
            version: 4,
            // Dimensions could be created from `nodes.shape()`, but since creating a `MapVector`
            // is fallible this, and the other constructor methods, would become fallible as well.
            // Let the caller provide a correct `MapVector` instead.
            dimensions,
            layer_probabilities: vec![SpawnProbability::Always; dimensions.y as usize],
            content_names: vec!["air".to_string()],
            nodes,
        }
    }

    pub fn from_bytes<T: AsRef<[u8]>>(input: T) -> Result<Schematic, Error> {
        parser::parse(input.as_ref())
    }

    pub fn annotated_nodes<'schematic>(&'schematic self) -> AnnotatedNodeIterator<'schematic> {
        AnnotatedNodeIterator::from_schematic(self)
    }

    /// Registers a content name in the `Schematic`. Checks for duplicates.
    ///
    /// Returns the content ID that `Node`s in this Schematic can point to.
    ///
    /// # Panics
    ///
    /// Panics when exceeding the limit of 65536 unique content names
    pub fn register_content(&mut self, name: Cow<'_, str>) -> u16 {
        // TODO Convert this field to a HashMap? But that would not be good for
        // `AnnotatedNodeIterator`

        match self.content_id_for_name(&name) {
            None => {
                assert!(
                    self.content_names.len() - 1 != u16::MAX as usize,
                    "A Schematic can only contain 65536 kinds of content"
                );

                self.content_names.push(name.into_owned());
                (self.content_names.len() - 1) as u16
            }
            Some(content_id) => content_id,
        }
    }

    /// Checks if the `Schematic` has enough [Node]s to fill its entire space, that all
    /// [Node]s refer to a valid array index in `content_names`, and that there is a
    /// [SpawnProbability] for each Y-layer.
    pub fn validate(&self) -> Result<(), Error> {
        if self.layer_probabilities.len() != self.dimensions.y as usize {
            return Err(Error::IncorrectNumberOfLayerProbabilities);
        }

        if self.nodes.len() != self.dimensions.volume() {
            return Err(Error::IncorrectNodeCount {
                found: self.nodes.len(),
                expected: self.dimensions.volume(),
            });
        }

        for node in &self.nodes {
            if node.content_id as usize >= self.content_names.len() {
                return Err(Error::InvalidContentIndex(node.content_id));
            }
        }

        Ok(())
    }

    /// Places the provided `Node` at `coordinates` in the schematic, overwriting whatever is there
    /// now.
    pub fn place_node(&mut self, node: &Node, coordinates: MapVector) -> Result<(), Error> {
        if coordinates >= self.dimensions {
            return Err(Error::OutOfBounds);
        }

        let raw_node = self.convert_node_to_raw_node(node);
        self.nodes[coordinates.as_shape()] = raw_node;

        Ok(())
    }

    /// Converts a [Node] to a [RawNode], and registers the [Node]'s content in this `Schematic` if
    /// it isn't part of this schematic already.
    pub fn convert_node_to_raw_node(&mut self, node: &Node) -> RawNode {
        self.register_content(node.content_name.clone());

        node.to_raw_node(self)
            .expect("Node's content to be registered in this schematic")
    }

    /// Rotates the `Schematic` 90 degrees to the left along its Y-axis
    ///
    /// Does not copy the [Node] data, returns a reference that uses the original `Schematic`
    /// instead.
    pub fn rotate_left(&self) -> SchematicRef<'_> {
        // TODO Some blocks use param2 to change their rotation (e.g. stair pieces). It would be
        // impossible to create a comprehensive list of all param2 rotation values (especially with
        // all the available mods), but hopefully all the default game's stair pieces use the same
        // param2 values However, it would mean copying the complete schematic and its data because
        // we would be modifying the nodes
        //
        // Maybe it should be up the caller to add a map() or something to the nodes() output that
        // copies/modifes the nodes' param2 when needed, as the caller might know which blocks and
        // mods are being used.

        let mut rotated_nodes = self.nodes.t();
        rotated_nodes.invert_axis(Axis(2));

        SchematicRef {
            schematic: self,
            nodes_view: rotated_nodes,
        }
    }

    /// Rotates the `Schematic` 90 degrees to the right along its Y-axis
    ///
    /// Does not copy the [Node] data, returns a reference that uses the original `Schematic`
    /// instead.
    pub fn rotate_right(&self) -> SchematicRef<'_> {
        let mut rotated_nodes = self.nodes.t();
        rotated_nodes.invert_axis(Axis(0));

        SchematicRef {
            schematic: self,
            nodes_view: rotated_nodes,
        }
    }

    /// Rotates the `Schematic` 180 degrees its Y-axis
    ///
    /// Does not copy the [Node] data, returns a reference that uses the original `Schematic`
    /// instead.
    pub fn rotate_180(&self) -> SchematicRef<'_> {
        let mut rotated_nodes = self.nodes.view();
        rotated_nodes.invert_axis(Axis(2));
        rotated_nodes.invert_axis(Axis(0));

        SchematicRef {
            schematic: self,
            nodes_view: rotated_nodes,
        }
    }

    /// Starting at `from_position`, fills the given space with copies of the given `Node`
    /// (converted to a [RawNode])
    pub fn fill(
        &mut self,
        from_position: MapVector,
        fill_space: MapVector,
        node: &Node,
    ) -> Result<(), Error> {
        let raw_node = self.convert_node_to_raw_node(node);

        editing::fill(self, from_position, fill_space, raw_node)
    }

    /// Copies the current `Schematic` and adds a new layer with copies of `fill_with_node`
    /// (converted to a [RawNode]) inserted on given `y` axis.
    pub fn insert_layer(&self, y: u16, fill_with_node: &Node) -> Result<Schematic, Error> {
        editing::insert_layer(self, y, fill_with_node)
    }

    /// Modifies the current `Schematic` by merging the entire given `Schematic` into it, starting
    /// at the coordinates given in `merge_at`.
    ///
    /// If the source `Schematic` doesn't fit in the target space, an
    /// [OutOfBounds](Error::OutOfBounds) will be returned.
    pub fn merge<'schematic>(
        &mut self,
        source: &'schematic impl NodeSpace<'schematic>,
        merge_at: MapVector,
    ) -> Result<(), Error> {
        editing::merge(source, self, merge_at)
    }

    /// Splits the `Schematic` up in smaller `Schematic`s, each of of `chunk_dimensions` in size.`
    ///
    /// The order of the chunks goes like this: first X, then Y, then Z.
    ///
    /// Because it only uses chunks of exact `chunk_dimensions` in size, any nodes that fall outside the
    /// last chunk of that size won't be returned.
    pub fn split_into_chunks(
        &self,
        chunk_dimensions: MapVector,
    ) -> impl Iterator<Item = Schematic> {
        // TODO Would be nice to be able to add coordinates to each item, either offsets within the
        // original Schematic, or some position of the chunks relative to each other.
        self.nodes
            .exact_chunks(chunk_dimensions.as_shape())
            .into_iter()
            .map(move |chunk| {
                let mut schematic = Schematic::with_array3(chunk_dimensions, chunk.to_owned());
                // This is inaccurate, as not all content names of the original Schematic might be
                // present in the smaller chunk, but the alternative would be to go through all
                // nodes to gather the correct IDs, and adjust those IDs to their new position in
                // the Schematic chunk's content_names array. That would be slow.
                schematic.content_names.clone_from(&self.content_names);

                schematic
            })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serializer::to_bytes(self)
    }
}

impl<'schematic> NodeSpace<'schematic> for Schematic {
    fn content_names(&'schematic self) -> impl Iterator<Item = &'schematic str> {
        self.content_names.iter().map(String::as_str)
    }

    fn content_id_for_name(&'schematic self, name: &str) -> Option<u16> {
        // TODO This is not very efficient. A map between name and ID, or something else, would
        // speed this up.
        self.content_names
            .iter()
            .enumerate()
            .find(|(_index, content_name)| *content_name == name)
            .map(|(index, _content_name)| index as u16)
    }

    fn content_name_for_id(&'schematic self, id: u16) -> Option<&'schematic str> {
        self.content_names.get(id as usize).map(String::as_str)
    }

    fn dimensions(&'schematic self) -> MapVector {
        self.dimensions
    }

    fn num_nodes(&'schematic self) -> usize {
        self.nodes.len()
    }

    fn nodes(&'schematic self) -> ArrayView3<'schematic, RawNode> {
        self.nodes.view()
    }

    fn node_at(&'schematic self, coordinates: MapVector) -> Option<Node<'schematic>> {
        let raw_node = self.nodes.get(coordinates.as_shape())?;

        raw_node.to_node(self).ok()
    }
}

/// Contains a modified view of a `Schematic`'s nodes, e.g. they have been rotated, or cut up
/// somehow.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SchematicRef<'schematic> {
    schematic: &'schematic Schematic,
    nodes_view: ArrayView3<'schematic, RawNode>,
}

impl<'schematic> SchematicRef<'schematic> {
    pub fn from_schematic(schematic: &'schematic Schematic) -> Self {
        SchematicRef {
            schematic,
            nodes_view: schematic.nodes.view(),
        }
    }
}

impl<'schematic> NodeSpace<'schematic> for SchematicRef<'schematic> {
    fn content_names(&'schematic self) -> impl Iterator<Item = &'schematic str> {
        self.schematic.content_names()
    }

    fn content_id_for_name(&'schematic self, name: &str) -> Option<u16> {
        self.schematic.content_id_for_name(name)
    }

    fn content_name_for_id(&'schematic self, id: u16) -> Option<&'schematic str> {
        self.schematic.content_name_for_id(id)
    }

    fn dimensions(&'schematic self) -> MapVector {
        self.schematic.dimensions
    }

    fn num_nodes(&'schematic self) -> usize {
        self.nodes_view.len()
    }

    fn nodes(&'schematic self) -> ArrayView3<'schematic, RawNode> {
        self.nodes_view
    }

    fn node_at(&'schematic self, coordinates: MapVector) -> Option<Node<'schematic>> {
        let raw_node = self.nodes_view.get(coordinates.as_shape())?;

        raw_node.to_node(self).ok()
    }
}

/// Iterator for a collection of `Node` with some added metadata as how the `Node` relates to the
/// `Schematic` its in.
pub struct AnnotatedNodeIterator<'schematic> {
    current_x: u16,
    current_y: u16,
    current_z: u16,
    schematic: &'schematic Schematic,
    nodes_iter: ndarray::iter::Iter<'schematic, RawNode, Dim<[usize; 3]>>,
}

impl<'schematic> AnnotatedNodeIterator<'schematic> {
    fn from_schematic(schematic: &'schematic Schematic) -> AnnotatedNodeIterator<'schematic> {
        AnnotatedNodeIterator {
            current_x: 0,
            current_y: 0,
            current_z: 0,
            schematic,
            nodes_iter: schematic.nodes.iter(),
        }
    }
}

impl<'schematic> Iterator for AnnotatedNodeIterator<'schematic> {
    type Item = AnnotatedNode<'schematic>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = match self.nodes_iter.next() {
            Some(raw_node) => {
                let coordinates =
                    MapVector::new(self.current_x, self.current_y, self.current_z).ok()?;

                let node = raw_node.to_node(self.schematic).expect(
                    "Raw node's content ID should point to a content name in the schematic.",
                );

                AnnotatedNode { coordinates, node }
            }
            None => return None,
        };

        self.current_z += 1;
        if self.current_z == self.schematic.dimensions.z {
            self.current_z = 0;
            self.current_y += 1;
        }

        if self.current_y == self.schematic.dimensions.y {
            self.current_y = 0;
            self.current_x += 1;
        }

        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    use super::NodeSpace;

    #[rstest]
    fn test_node_iterator(schematic: Schematic) {
        let mut nodes_iter = schematic.annotated_nodes();

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 0).try_into().unwrap());
        assert_eq!(
            annotated_node.node,
            schematic.nodes[(0, 0, 0)].to_node(&schematic).unwrap()
        );

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 1).try_into().unwrap());
        assert_eq!(
            annotated_node.node,
            schematic.nodes[(0, 0, 1)].to_node(&schematic).unwrap()
        );

        let mut nodes_iter = nodes_iter.skip(1);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 1, 0).try_into().unwrap());
        assert_eq!(
            annotated_node.node,
            schematic.nodes[(0, 1, 0)].to_node(&schematic).unwrap()
        );

        let mut nodes_iter = nodes_iter.skip(2);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (1, 0, 0).try_into().unwrap());
        assert_eq!(
            annotated_node.node,
            schematic.nodes[(1, 0, 0)].to_node(&schematic).unwrap()
        );

        let mut nodes_iter = nodes_iter.skip(10);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (2, 1, 2).try_into().unwrap());
        assert_eq!(
            annotated_node.node,
            schematic.nodes[(2, 1, 2)].to_node(&schematic).unwrap()
        );
    }

    #[rstest]
    fn test_node_at(schematic: Schematic) {
        assert_eq!(
            schematic.node_at((0, 0, 0).try_into().unwrap()).unwrap(),
            schematic.nodes[(0, 0, 0)].to_node(&schematic).unwrap(),
        );

        assert_eq!(
            schematic.node_at((1, 1, 1).try_into().unwrap()).unwrap(),
            schematic.nodes[(1, 1, 1)].to_node(&schematic).unwrap(),
        );

        assert_eq!(schematic.node_at((999, 999, 999).try_into().unwrap()), None);
    }

    #[test]
    fn test_validate() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap()).unwrap();

        assert!(schematic.validate().is_ok());

        schematic.nodes.first_mut().unwrap().content_id = 999;
        assert!(schematic.validate().is_err());

        schematic.nodes.first_mut().unwrap().content_id = 0;
        assert!(schematic.validate().is_ok());
    }

    #[test]
    fn test_convert_node_to_raw_node() {
        let mut schematic = Schematic::with_raw_nodes(
            (1, 1, 1).try_into().unwrap(),
            vec![RawNode::new(0, SpawnProbability::Always, true, 0)],
        )
        .unwrap();
        schematic.register_content("default:cobble".into());

        let node = Node::new(
            "default:stone".to_string().into(),
            SpawnProbability::Always,
            true,
            0,
        );

        schematic.convert_node_to_raw_node(&node);

        assert_eq!(
            schematic.content_names.len(),
            3,
            "default:stone should have been registered as the 3rd content"
        );
    }

    #[rstest]
    fn test_place_node(mut schematic: Schematic) {
        let node = Node {
            content_name: "default:cobble".into(),
            ..Default::default()
        };
        let coordinates = (0, 1, 2).try_into().unwrap();

        schematic.place_node(&node, coordinates).unwrap();

        let found_node = schematic.node_at(coordinates).unwrap();

        assert_eq!(node, found_node);
    }

    #[test]
    fn test_place_node_out_of_bounds() {
        let mut schematic = Schematic::new((1, 1, 1).try_into().unwrap()).unwrap();
        let node = Node {
            content_name: "default:cobble".into(),
            ..Default::default()
        };
        let coordinates = (1, 1, 1).try_into().unwrap();

        schematic.place_node(&node, coordinates).unwrap_err();
    }

    #[rstest]
    fn test_split_into_chunks(schematic: Schematic) {
        let chunks = schematic
            .split_into_chunks((3, 2, 1).try_into().unwrap())
            .collect::<Vec<Schematic>>();

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| chunk.nodes.len() == 6));
    }

    #[rstest]
    fn test_rotate_left(schematic: Schematic) {
        // Sanity check
        assert_eq!(schematic.nodes.iter().next().unwrap().content_id, 1);

        let rotated_schematic = schematic.rotate_left();

        let nodes = rotated_schematic.nodes();
        let mut iter = nodes.iter();
        assert_eq!(iter.next().unwrap().content_id, 13);
        let mut iter = iter.skip(1);
        assert_eq!(iter.next().unwrap().content_id, 1);
    }

    #[rstest]
    fn test_rotate_180(schematic: Schematic) {
        // Sanity check
        assert_eq!(schematic.nodes.iter().next().unwrap().content_id, 1);

        let rotated_schematic = schematic.rotate_180();

        let nodes = rotated_schematic.nodes();
        let mut iter = nodes.iter();
        assert_eq!(iter.next().unwrap().content_id, 15);
        let mut iter = iter.skip(13);
        assert_eq!(iter.next().unwrap().content_id, 1);
    }

    #[rstest]
    fn test_rotate_right(schematic: Schematic) {
        // Sanity check
        assert_eq!(schematic.nodes.iter().next().unwrap().content_id, 1);

        let rotated_schematic = schematic.rotate_right();

        let nodes = rotated_schematic.nodes();
        let mut iter = nodes.iter();
        assert_eq!(iter.next().unwrap().content_id, 3);
        let mut iter = iter.skip(1);
        assert_eq!(iter.next().unwrap().content_id, 15);
    }

    #[fixture]
    fn schematic() -> Schematic {
        let mut schematic = Schematic::with_raw_nodes(
            (3, 2, 3).try_into().unwrap(),
            (1..=18)
                .map(|i| RawNode::new(i, SpawnProbability::Always, true, 0))
                .collect::<Vec<RawNode>>(),
        )
        .unwrap();
        schematic.register_content("default:cobble".into());
        (2..=schematic.num_nodes()).for_each(|i| {
            schematic.register_content(format!("content:{i}").into());
        });

        schematic
    }

    #[test]
    fn test_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Schematic>();
        assert_send::<SchematicRef>();
    }

    #[test]
    fn test_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<Schematic>();
        assert_sync::<SchematicRef>();
    }
}
