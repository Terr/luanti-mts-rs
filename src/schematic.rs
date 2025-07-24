use std::collections::HashMap;

use ndarray::{Array3, Dim, s};

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
                content_index: 0,
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

        Schematic {
            version: 4,
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
            if node.content_index as usize >= self.content_names.len() {
                return Err(Error::InvalidContentNameIndex(node.content_index));
            }
        }

        Ok(())
    }

    /// Registers a content name in the `Schematic`. Checks for duplicates.
    ///
    /// Returns the content index that `Nodes` in this Schematic can point to.
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
        for (source_content_index, content_name) in
            source_schematic.content_names.iter().enumerate()
        {
            match current_content_positions.get(content_name) {
                // Content already exists in this Schematic, but might be at a different index than
                // at the source Schematic.
                Some(current_content_index) => {
                    if *current_content_index != source_content_index {
                        source_content_map
                            .insert(source_content_index as u16, *current_content_index as u16);
                    }
                }
                // Content isn't present in this Schematic yet
                None => {
                    self.content_names.push(content_name.clone());
                    let new_content_index = self.content_names.len() - 1;
                    source_content_map
                        .insert(source_content_index as u16, new_content_index as u16);
                }
            }
        }

        let from_shape = merge_at.as_shape();
        let to_shape = merge_end.as_shape();
        let target_space = self.nodes.slice_mut(s![
            from_shape.0..to_shape.0,
            from_shape.1..to_shape.1,
            from_shape.2..to_shape.2
        ]);

        // This does the actual merging
        ndarray::Zip::from(&source_schematic.nodes).map_assign_into(target_space, |node| {
            // Copies the Node
            let mut node = *node;

            // If the content ID of a copied Node is different in this Schematic, update it
            if let Some(new_content_index) = source_content_map.get(&node.content_index) {
                node.content_index = *new_content_index;
            }

            node
        });

        Ok(())
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
                    .get(node.content_index as usize)
                    .expect(
                        "node's content index should point to a content name in the schematic.",
                    );

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
    pub(crate) content_index: u16,
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
        content_index: u16,
        probability: SpawnProbability,
        force_placement: bool,
        param2: u8,
    ) -> Self {
        Node {
            content_index,
            probability,
            force_placement,
            param2,
        }
    }

    pub fn with_content_index(content_index: u16) -> Self {
        Node {
            content_index,
            probability: SpawnProbability::Always,
            force_placement: false,
            param2: 0,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct AnnotatedNode<'node> {
    pub coordinates: MapVector,
    pub content_name: &'node str,
    pub node: &'node Node,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum SpawnProbability {
    Never,
    Always,
    Custom(u8),
}

impl From<u8> for SpawnProbability {
    fn from(value: u8) -> Self {
        match value {
            0 => SpawnProbability::Never,
            // Nowadays '127' means "always spawn" but in the past this value used to be '255'.
            // Just accept any higher value to so we can use a From instead of a TryFrom for ease of use.
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

        schematic.nodes.first_mut().unwrap().content_index = 999;
        assert!(schematic.validate().is_err());

        schematic.nodes.first_mut().unwrap().content_index = 0;
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
        let node = Node::with_content_index(schematic.register_content("default:dirt".to_string()));

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
        let node = Node::with_content_index(0);

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
                &Node::with_content_index(default_dirt),
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
                .filter(|node| node.content_index == default_dirt)
                .count(),
            12,
            "Nodes missing or indexes of merged Nodes were not updated correctly"
        );

        // When seen as a 1D vector (as it will be saved in a MTS file), the following positions
        // should have been updated by the merge:
        for position in [0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14] {
            let node = schematic_1.nodes.iter().nth(position).unwrap();

            assert_eq!(node.content_index, default_dirt);
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
                &Node::with_content_index(1),
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
        let content_index = original_schematic.register_content("default:cobble".to_string());
        let node = Node::with_content_index(content_index);

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
                .all(|node| node.content_index == 0)
        );
        assert!(
            new_schematic
                .nodes
                .slice(s![.., 1, ..])
                .iter()
                .all(|node| node.content_index == 1)
        );
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
