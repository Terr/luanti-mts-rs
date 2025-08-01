use std::collections::HashMap;

use ndarray::{Array3, AssignElem, Dim, s};

use crate::Error;

/// Luanti's maximum map size is 62013 x 62013 x 62013, from -31006 to 31006 (inclusive), but the
/// schematics file format defines it as an unsigned 16-bit integer.
const MAX_MAP_DIMENSION: u16 = 62013;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Schematic {
    pub(crate) version: u16,
    pub dimensions: MapVector,
    pub(crate) layer_probabilities: Vec<SpawnProbability>,
    /// Called "name ids" in the file format documentation, it's an array of strings that identify
    /// the contents of a node, i.e. the type of block or items like torches.
    ///
    /// Examples of names are: "air", "default:cobble", "mcl_core:quartz"
    pub(crate) content_names: Vec<String>,
    pub(crate) nodes: Array3<Node>,
}

impl Schematic {
    pub fn new(dimensions: MapVector) -> Self {
        let nodes = vec![
            Node {
                content_id: 0,
                probability: SpawnProbability::Always,
                force_placement: false,
                param2: 0
            };
            dimensions.volume()
        ];

        Self::with_nodes(dimensions, nodes)
    }

    pub fn with_nodes(dimensions: MapVector, nodes: Vec<Node>) -> Self {
        let nodes = Array3::from_shape_vec(dimensions.as_shape(), nodes).unwrap();

        Self::with_array3(dimensions, nodes)
    }

    pub fn with_array3(dimensions: MapVector, nodes: Array3<Node>) -> Self {
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

    pub fn annotated_nodes(&self) -> AnnotatedNodeIterator<'_> {
        AnnotatedNodeIterator::from_schematic(self)
    }

    pub fn node_at(&self, coordinates: MapVector) -> Option<&Node> {
        self.nodes.get(coordinates.as_shape())
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Checks if the `Schematic` has enough `Nodes` to fill its entire space, that all
    /// `Nodes` refer to a valid array index in `content_names`, and that there is a
    /// `SpawnProbability` for each Y-layer.
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

        for node in self.nodes.iter() {
            if node.content_id as usize >= self.content_names.len() {
                return Err(Error::InvalidContentNameIndex(node.content_id));
            }
        }

        Ok(())
    }

    /// Registers a content name in the `Schematic`. Checks for duplicates.
    ///
    /// Returns the content ID that `Nodes` in this Schematic can point to.
    pub fn register_content(&mut self, name: String) -> u16 {
        // TODO Convert this field to a HashMap? But that would not be good for
        // `AnnotatedNodeIterator`

        match self.content_id_for_name(&name) {
            None => {
                self.content_names.push(name);
                (self.content_names.len() - 1) as u16
            }
            Some(content_id) => content_id,
        }
    }

    pub fn content_id_for_name(&self, name: &str) -> Option<u16> {
        self.content_names
            .iter()
            .enumerate()
            .find(|(_index, content_name)| *content_name == name)
            .map(|(index, _content_name)| index as u16)
    }

    pub fn content_name_for_id(&self, id: u16) -> Option<&String> {
        self.content_names.get(id as usize)
    }

    /// Starting at `from`, fills the given space with copies of the given `Node`.
    pub fn fill(
        &mut self,
        from: MapVector,
        fill_space: MapVector,
        node: &Node,
    ) -> Result<(), Error> {
        let to: MapVector = from.checked_add(fill_space).ok_or(Error::OutOfBounds)?;
        if to > self.dimensions {
            return Err(Error::OutOfBounds);
        }

        let from_shape = from.as_shape();
        let to_shape = to.as_shape();

        self.nodes
            .slice_mut(s![
                from_shape.0..to_shape.0,
                from_shape.1..to_shape.1,
                from_shape.2..to_shape.2
            ])
            .fill(*node);

        Ok(())
    }

    /// Builds a new `Schematic` with a new layer of `fill_with_node` inserted on given `y` axis.
    pub fn insert_layer(&mut self, y: u16, fill_with_node: &Node) -> Result<Schematic, Error> {
        if y > self.dimensions.y {
            return Err(Error::OutOfBounds);
        }

        let new_dimensions = self
            .dimensions
            .checked_add((0, 1, 0).try_into()?)
            .ok_or(Error::OutOfBounds)?;

        let mut extended_nodes = Array3::from_elem(new_dimensions.as_shape(), *fill_with_node);

        // Copy all nodes above the new layer
        let y = y as usize;
        self.nodes
            .slice(s![.., 0..y, ..])
            .assign_to(&mut extended_nodes.slice_mut(s![.., 0..y, ..]));

        // Copy all nodes below the new layer
        self.nodes
            .slice(s![.., y.., ..])
            .assign_to(&mut extended_nodes.slice_mut(s![.., y + 1.., ..]));

        // TODO Like with from_bytes(), this could do with a better constructor
        let mut new_schematic = Schematic {
            version: self.version,
            dimensions: new_dimensions,
            layer_probabilities: self.layer_probabilities.clone(),
            content_names: self.content_names.clone(),
            nodes: extended_nodes,
        };

        new_schematic
            .layer_probabilities
            .insert(y, SpawnProbability::Always);

        Ok(new_schematic)
    }

    /// Modifies the current `Schematic` by merging the entire given `Schematic` into it, starting
    /// at the coordinates given in `merge_at`.
    ///
    /// If the source `Schematic` doesn't fit in the target space, an `error::OutOfBounds` will be
    /// returned.
    pub fn merge(
        &mut self,
        source_schematic: &Schematic,
        merge_at: MapVector,
    ) -> Result<(), Error> {
        let merge_end = merge_at
            .checked_add(source_schematic.dimensions)
            .ok_or(Error::OutOfBounds)?;
        if merge_end > self.dimensions {
            return Err(Error::OutOfBounds);
        }

        let current_content_positions: HashMap<String, usize> = self
            .content_names
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, name)| (name, index))
            .collect();
        // A mapping between the content name IDs of the source Schematic, and those at this
        // Schematic
        let mut source_content_map: HashMap<u16, u16> = HashMap::new();

        // Register the content IDs of the source Schematic into at this Schematic, and keep track
        // of their updated IDs (i.e. index positions)
        for (source_content_id, content_name) in source_schematic.content_names.iter().enumerate() {
            match current_content_positions.get(content_name) {
                // Content already exists in this Schematic, but might be at a different index than
                // at the source Schematic.
                Some(current_content_id) => {
                    if *current_content_id != source_content_id {
                        source_content_map
                            .insert(source_content_id as u16, *current_content_id as u16);
                    }
                }
                // Content isn't present in this Schematic yet
                None => {
                    self.content_names.push(content_name.clone());
                    let new_content_id = self.content_names.len() - 1;
                    source_content_map.insert(source_content_id as u16, new_content_id as u16);
                }
            }
        }

        // These two content IDs are for blocks that are considered by Luanti as "nothing" when
        // it comes to deciding if a node should overwrite the existing position, and the new node
        // is marked as "force_placement = false"
        let content_air = self.content_id_for_name("air");
        let content_ignore = self.content_id_for_name("ignore");

        let from_shape = merge_at.as_shape();
        let to_shape = merge_end.as_shape();
        let slice = s![
            from_shape.0..to_shape.0,
            from_shape.1..to_shape.1,
            from_shape.2..to_shape.2
        ];

        let target_space = self.nodes.slice_mut(slice);

        // This does the actual merging
        ndarray::Zip::from(&source_schematic.nodes)
            // The reason for not using `map_assign_into()` here is that that function doesn't pass
            // the target `into` slice into the closure, so we aren't able to make any comparisons
            // to the original node.
            .and(target_space)
            .for_each(move |merge_node, target_node| {
                // This doesn't take any SpawnProbability::Custom() probability into account, such
                // nodes will just overwrite the current node. The game will then decide whether to
                // spawn the node or not.
                if merge_node.probability == SpawnProbability::Never && !merge_node.force_placement
                {
                    let place_merge_node = if let Some(air) = content_air
                        && target_node.content_id == air
                    {
                        true
                    } else if let Some(ignore) = content_ignore
                        && target_node.content_id == ignore
                    {
                        true
                    } else {
                        false
                    };

                    if !place_merge_node {
                        // Leave the current node alone
                        return;
                    }
                }

                // Copies the Node
                let mut node = *merge_node;

                // If the content ID of a copied Node is different in this Schematic, update it
                if let Some(new_content_id) = source_content_map.get(&node.content_id) {
                    node.content_id = *new_content_id;
                }

                target_node.assign_elem(node);
            });

        Ok(())
    }

    /// Splits the `Schematic`` up in smaller `Schematic`s, each of of `chunk_dimensions` in size.`
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
                schematic.content_names = self.content_names.clone();

                schematic
            })
    }
}

/// Iterator for a collection of `Node` with some added metadata as how the `Node` relates to the
/// `Schematic` its in.
pub struct AnnotatedNodeIterator<'schematic> {
    current_x: u16,
    current_y: u16,
    current_z: u16,
    schematic: &'schematic Schematic,
    nodes_iter: ndarray::iter::Iter<'schematic, Node, Dim<[usize; 3]>>,
}

impl<'schematic> AnnotatedNodeIterator<'_> {
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
            Some(node) => {
                let coordinates =
                    MapVector::new(self.current_x, self.current_y, self.current_z).ok()?;

                let content_name = self
                    .schematic
                    .content_names
                    .get(node.content_id as usize)
                    .expect("node's content ID should point to a content name in the schematic.");

                AnnotatedNode {
                    coordinates,
                    content_name,
                    node,
                }
            }
            None => return None,
        };

        self.current_x += 1;
        if self.current_x == self.schematic.dimensions.x {
            self.current_x = 0;
            self.current_y += 1;
        }

        if self.current_y == self.schematic.dimensions.y {
            self.current_y = 0;
            self.current_z += 1;
        }

        Some(item)
    }
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

/// A map-aware, three-dimensional vector.
///
/// "Map-aware" as it checks its values against the maximum map/schematic size of Luanti (see `MAX_MAP_DIMENSION`)
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd)]
pub struct MapVector {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}

impl MapVector {
    pub fn new(x: u16, y: u16, z: u16) -> Result<Self, Error> {
        if x >= MAX_MAP_DIMENSION || y >= MAX_MAP_DIMENSION || z >= MAX_MAP_DIMENSION {
            return Err(Error::OutOfBounds);
        }

        Ok(MapVector { x, y, z })
    }

    pub fn volume(&self) -> usize {
        self.x as usize * self.y as usize * self.z as usize
    }

    pub fn checked_add(&self, other: MapVector) -> Option<Self> {
        let x = self.x.checked_add(other.x)?;
        let y = self.y.checked_add(other.y)?;
        let z = self.z.checked_add(other.z)?;

        MapVector::new(x, y, z).ok()
    }

    /// Converts the `MapVector` into a shape that can be used to access a row-major ndarray, such
    /// as a `Schematic`'s `nodes`.
    pub fn as_shape(self) -> (usize, usize, usize) {
        (self.z as usize, self.y as usize, self.x as usize)
    }
}

impl TryFrom<(u16, u16, u16)> for MapVector {
    type Error = Error;

    fn try_from(value: (u16, u16, u16)) -> Result<Self, Self::Error> {
        MapVector::new(value.0, value.1, value.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    fn test_node_iterator(schematic: Schematic) {
        let mut nodes_iter = schematic.annotated_nodes();

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 0).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[(0, 0, 0)]);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (1, 0, 0).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[(1, 0, 0)]);

        let mut nodes_iter = nodes_iter.skip(1);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 1, 0).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[(0, 1, 0)]);

        let mut nodes_iter = nodes_iter.skip(2);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 1).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[(0, 0, 1)]);

        let mut nodes_iter = nodes_iter.skip(10);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (2, 1, 2).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[(2, 1, 2)]);
    }

    #[rstest]
    fn test_node_at(schematic: Schematic) {
        assert_eq!(
            schematic.node_at((0, 0, 0).try_into().unwrap()).unwrap(),
            &schematic.nodes[(0, 0, 0)]
        );

        assert_eq!(
            schematic.node_at((1, 1, 1).try_into().unwrap()).unwrap(),
            &schematic.nodes[(1, 1, 1)]
        );

        assert_eq!(schematic.node_at((999, 999, 999).try_into().unwrap()), None);
    }

    #[test]
    fn test_validate() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap());

        assert!(schematic.validate().is_ok());

        schematic.nodes.first_mut().unwrap().content_id = 999;
        assert!(schematic.validate().is_err());

        schematic.nodes.first_mut().unwrap().content_id = 0;
        assert!(schematic.validate().is_ok());
    }

    #[test]
    fn test_fill() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap());
        assert!(
            schematic
                .annotated_nodes()
                .all(|node| node.content_name == "air")
        );
        let node = Node::with_content_id(schematic.register_content("default:dirt".to_string()));

        schematic
            .fill(
                (0, 0, 0).try_into().unwrap(),
                (2, 2, 2).try_into().unwrap(),
                &node,
            )
            .unwrap();

        assert!(
            schematic
                .annotated_nodes()
                .all(|node| node.content_name == "default:dirt"),
            "not all nodes in the schematic were replaced"
        );
    }

    #[test]
    fn test_fill_out_of_bounds() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap());
        let node = Node::with_content_id(0);

        schematic
            .fill(
                (0, 0, 0).try_into().unwrap(),
                (3, 3, 3).try_into().unwrap(),
                &node,
            )
            .unwrap_err();
    }

    #[test]
    fn test_merge() {
        let mut schematic_1 = Schematic::new((3, 3, 3).try_into().unwrap());
        schematic_1.register_content("something".to_string());

        let mut schematic_2 = Schematic::new((3, 2, 2).try_into().unwrap());
        let default_dirt = schematic_2.register_content("default:dirt".to_string());
        schematic_2
            .fill(
                (0, 0, 0).try_into().unwrap(),
                schematic_2.dimensions,
                &Node::with_content_id(default_dirt),
            )
            .unwrap();

        schematic_1
            .merge(&schematic_2, (0, 0, 0).try_into().unwrap())
            .unwrap();

        let default_dirt = schematic_1.content_id_for_name("default:dirt").unwrap();

        assert!(schematic_1.validate().is_ok());
        assert_eq!(
            schematic_1.content_names,
            &["air", "something", "default:dirt"]
        );
        assert_eq!(
            schematic_1
                .nodes
                .iter()
                .filter(|node| node.content_id == default_dirt)
                .count(),
            12,
            "Nodes missing or indexes of merged Nodes were not updated correctly"
        );

        // When seen as a 1D vector (as it will be saved in a MTS file), the following positions
        // should have been updated by the merge:
        for position in [0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14] {
            let node = schematic_1.nodes.iter().nth(position).unwrap();

            assert_eq!(node.content_id, default_dirt);
        }
    }

    #[test]
    fn test_merge_small_schematic_into_larger() {
        let mut schematic_1 = Schematic::new((8, 8, 8).try_into().unwrap());
        schematic_1.register_content("something".to_string());

        let mut schematic_2 = Schematic::new((2, 2, 2).try_into().unwrap());
        schematic_2.register_content("default:dirt".to_string());
        schematic_2
            .fill(
                (0, 0, 0).try_into().unwrap(),
                (2, 2, 2).try_into().unwrap(),
                &Node::with_content_id(1),
            )
            .unwrap();

        schematic_1
            .merge(&schematic_2, (1, 1, 1).try_into().unwrap())
            .unwrap();

        assert!(schematic_1.validate().is_ok());
        assert_eq!(
            schematic_1.content_names,
            &["air", "something", "default:dirt"]
        );
    }

    #[rstest]
    fn test_merge_optional_node_doesnt_overwrite_existing(mut schematic: Schematic) {
        let content_id = schematic.register_content("default:dry_dirt".to_string());
        let mut optional_node = Node::with_content_id(content_id);
        optional_node.probability = SpawnProbability::Never;
        let optional_schematic =
            Schematic::with_nodes((1, 1, 1).try_into().unwrap(), vec![optional_node]);

        schematic
            .merge(&optional_schematic, (0, 0, 0).try_into().unwrap())
            .unwrap();

        let node = schematic.node_at((0, 0, 0).try_into().unwrap()).unwrap();
        assert_eq!(
            schematic.content_name_for_id(node.content_id).unwrap(),
            "default:cobble",
            "The original node should not have been overwritten by this default:dry_dirt node"
        );
    }

    #[test]
    fn test_dimensions_checked_add() {
        let dimensions = MapVector::new(1000, 1000, 1000).unwrap();

        assert_eq!(
            dimensions.checked_add((1000, 1000, 1000).try_into().unwrap()),
            Some((2000, 2000, 2000).try_into().unwrap())
        );
    }

    #[test]
    fn test_insert_layer() {
        let mut original_schematic = Schematic::new((2, 1, 2).try_into().unwrap());
        let content_id = original_schematic.register_content("default:cobble".to_string());
        let node = Node::with_content_id(content_id);

        let new_schematic = original_schematic.insert_layer(1, &node).unwrap();

        assert_eq!(new_schematic.dimensions.y, 2);
        new_schematic.validate().unwrap();
        assert_eq!(
            new_schematic.node_at((0, 1, 0).try_into().unwrap()),
            Some(&node)
        );
        assert!(
            new_schematic
                .nodes
                .slice(s![.., 0, ..])
                .iter()
                .all(|node| node.content_id == 0)
        );
        assert!(
            new_schematic
                .nodes
                .slice(s![.., 1, ..])
                .iter()
                .all(|node| node.content_id == 1)
        );
    }

    #[rstest]
    fn test_split_into_chunks(schematic: Schematic) {
        let chunks = schematic
            .split_into_chunks((3, 2, 1).try_into().unwrap())
            .collect::<Vec<Schematic>>();

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| chunk.nodes.len() == 6));
    }

    #[fixture]
    fn schematic() -> Schematic {
        let mut schematic = Schematic::with_nodes(
            (3, 2, 3).try_into().unwrap(),
            vec![Node::new(1, SpawnProbability::Always, true, 0); 18],
        );
        schematic.register_content("default:cobble".to_string());

        schematic
    }
}
