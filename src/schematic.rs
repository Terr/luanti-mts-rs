use std::collections::HashMap;

use crate::Error;

/// Luanti's maximum map size is 62013 x 62013 x 62013, from -31006 to 31006 (inclusive), but the
/// schematics file format defines it as an unsigned 16-bit integer.
const MAX_MAP_DIMENSION: u16 = 62013;

#[derive(Debug, PartialEq, Eq)]
pub struct Schematic {
    pub(crate) version: u16,
    pub dimensions: MapVector,
    pub(crate) layer_probabilities: Vec<SpawnProbability>,
    /// Called "name ids" in the file format documentation, it's an array of strings that identify
    /// the contents of a node, i.e. the type of block or items like torches.
    ///
    /// Examples of names are: "air", "default:cobble", "mcl_core:quartz"
    pub(crate) content_names: Vec<String>,
    pub(crate) nodes: Vec<Node>,
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

    pub fn content_id_for_name(&self, name: &str) -> Option<u16> {
        self.content_names
            .iter()
            .enumerate()
            .find(|(_index, content_name)| *content_name == name)
            .map(|(index, _content_name)| index as u16)
    }

    pub fn node_at(&self, coordinates: MapVector) -> Option<&Node> {
        self.nodes.get(self.node_index_for_coordinates(coordinates))
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

    /// Registers a content name in the `Schematic`. Returns the content index that `Nodes` in this
    /// Schematic can point to.
    pub fn add_content(&mut self, name: String) -> u16 {
        self.content_names.push(name);

        (self.content_names.len() - 1) as u16
    }

    /// Starting at `from`, fills the given space with copies of the given `Node`.
    pub fn fill(
        &mut self,
        from: MapVector,
        fill_space: MapVector,
        node: &Node,
    ) -> Result<(), Error> {
        let bounds_check: MapVector = from.checked_add(fill_space).ok_or(Error::OutOfBounds)?;
        if bounds_check > self.dimensions {
            return Err(Error::OutOfBounds);
        }

        // TODO This should add the Node's content name to the schematic when necessary, like merge() does.
        // Perhaps add_content() can be modified for this and renamed to register_content_name() or
        // something

        for z in from.z..from.z + fill_space.z {
            for y in from.y..from.y + fill_space.y {
                for x in from.x..from.x + fill_space.x {
                    let coordinates = (x, y, z).try_into()?;
                    let index = self.node_index_for_coordinates(coordinates);
                    // This array access is safe because we checked the bounds above, and we can be
                    // sure we're not going out of bounds
                    self.nodes[index] = *node;
                }

                /*
                // fill() uses clone so is not as efficient
                let fill_from = self.node_index_for_coordinates((from.x, y, z).into());
                let fill_to = self.node_index_for_coordinates((from.x + dimensions.x, y, z).into());

                self.nodes[fill_from..fill_to].fill(*node);
                */
            }
        }

        Ok(())
    }

    pub fn insert_layer(&mut self, layer_num: u16, fill_with_node: &Node) -> Result<(), Error> {
        if layer_num > self.dimensions.y {
            return Err(Error::OutOfBounds);
        }

        self.layer_probabilities
            .insert(layer_num as usize, SpawnProbability::Always);

        let mut extended_nodes = Vec::with_capacity(
            self.dimensions
                .checked_add((0, 1, 0).try_into()?)
                .ok_or(Error::OutOfBounds)?
                .volume(),
        );

        // TODO This code is really complex with all the array index juggling. Is there a better
        // datastructure that could be used?
        // A Vec<Vec<Node>>, with the first Vec dimension being the Y-axis?
        // Or a Vec<Layer>, with Layer also keeping track of the SpawnProbability?

        let mut remaining_nodes = self.nodes.as_slice();
        let split_index = self.node_index_for_coordinates((0, layer_num, 0).try_into()?);
        let fill_x = [*fill_with_node].repeat(self.dimensions.x as usize);

        for _ in 0..self.dimensions.z {
            let (nodes_below, rest) = remaining_nodes.split_at(split_index);
            extended_nodes.extend(nodes_below);
            extended_nodes.extend(&fill_x);

            let until_next_insert = (self.dimensions.y - layer_num) * self.dimensions.x;
            extended_nodes.extend(&rest[..until_next_insert as usize]);

            remaining_nodes = &remaining_nodes[until_next_insert as usize..];
        }

        self.nodes = extended_nodes;

        // This needs to be done after the `node_index_for_coordinates()` calls, as the schematic's
        // dimensions influence its calculations
        self.dimensions = self
            .dimensions
            .checked_add((0, 1, 0).try_into()?)
            .ok_or(Error::OutOfBounds)?;

        Ok(())
    }

    /// Modifies the current `Schematic` by merging the given `Schematic` into it, starting at the
    /// coordinates given in `merge_at`.
    ///
    /// If any `Node` during the merge falls outside of the current schematic's dimensions, an
    /// `error::OutOfBounds` will be returned.
    pub fn merge(
        &mut self,
        source_schematic: &Schematic,
        merge_at: MapVector,
    ) -> Result<(), Error> {
        let bounds_check = merge_at
            .checked_add(source_schematic.dimensions)
            .ok_or(Error::OutOfBounds)?;
        if bounds_check > self.dimensions {
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

        for z in 0..source_schematic.dimensions.z {
            for y in 0..source_schematic.dimensions.y {
                let copy_start = source_schematic.node_index_for_coordinates((0, y, z).try_into()?);
                let copy_end = source_schematic
                    .node_index_for_coordinates((source_schematic.dimensions.x, y, z).try_into()?);

                let mut nodes_to_copy: Vec<Node> =
                    source_schematic.nodes[copy_start..copy_end].to_vec();
                for node in nodes_to_copy.iter_mut() {
                    // Check if this node's content got a new index position
                    if let Some(new_content_index) = source_content_map.get(&node.content_index) {
                        node.content_index = *new_content_index;
                    }
                }

                let write_start = self.node_index_for_coordinates(
                    (merge_at.x, merge_at.y + y, merge_at.z + z).try_into()?,
                );
                let write_end = self.node_index_for_coordinates(
                    (
                        merge_at.x + source_schematic.dimensions.x,
                        merge_at.y + y,
                        merge_at.z + z,
                    )
                        .try_into()?,
                );
                self.nodes[write_start..write_end].copy_from_slice(&nodes_to_copy[..]);
            }
        }

        Ok(())
    }

    fn node_index_for_coordinates(&self, coordinates: MapVector) -> usize {
        coordinates.z as usize * (self.dimensions.y as usize * self.dimensions.x as usize)
            + coordinates.y as usize * self.dimensions.x as usize
            + coordinates.x as usize
    }
}

/// Iterator for a collection of `Node` with some added metadata as how the `Node` relates to the
/// `Schematic` its in.
pub struct AnnotatedNodeIterator<'schematic> {
    current_x: u16,
    current_y: u16,
    current_z: u16,
    schematic: &'schematic Schematic,
    nodes_iter: std::slice::Iter<'schematic, Node>,
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
        assert_eq!(annotated_node.node, &schematic.nodes[0]);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (1, 0, 0).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[1]);

        let mut nodes_iter = nodes_iter.skip(1);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 1, 0).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[4]);

        let mut nodes_iter = nodes_iter.skip(2);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 1).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[6]);

        let mut nodes_iter = nodes_iter.skip(10);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (2, 1, 2).try_into().unwrap());
        assert_eq!(annotated_node.node, &schematic.nodes[17]);
    }

    #[rstest]
    fn test_node_at(schematic: Schematic) {
        assert_eq!(
            schematic.node_at((0, 0, 0).try_into().unwrap()).unwrap(),
            &schematic.nodes[0]
        );

        assert_eq!(
            schematic.node_at((1, 1, 1).try_into().unwrap()).unwrap(),
            &schematic.nodes[10]
        );

        assert_eq!(schematic.node_at((999, 999, 999).try_into().unwrap()), None);
    }

    #[test]
    fn test_validate() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap());

        assert!(schematic.validate().is_ok());

        schematic.nodes[0].content_index = 999;
        assert!(schematic.validate().is_err());

        schematic.nodes[0].content_index = 0;
        assert!(schematic.validate().is_ok());

        schematic.nodes.remove(6);
        assert!(schematic.validate().is_err());
    }

    #[rstest]
    #[case((0, 0, 0), 0)]
    #[case((1, 0, 0), 1)]
    #[case((0, 1, 0), 3)]
    #[case((0, 0, 1), 6)]
    #[case((0, 1, 1), 9)]
    #[case((1, 1, 1), 10)]
    fn test_node_index(
        schematic: Schematic,
        #[case] coordinates: (u16, u16, u16),
        #[case] expected: usize,
    ) {
        let coordinates = coordinates.try_into().unwrap();

        let result = schematic.node_index_for_coordinates(coordinates);

        assert_eq!(result, expected);
    }

    #[test]
    fn test_fill() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap());
        assert!(
            schematic
                .annotated_nodes()
                .all(|node| node.content_name == "air")
        );
        let node = Node::with_content_index(schematic.add_content("default:dirt".to_string()));

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
        let mut schematic_1 = Schematic::new((2, 2, 2).try_into().unwrap());
        schematic_1.add_content("something".to_string());

        let mut schematic_2 = Schematic::new((2, 2, 2).try_into().unwrap());
        schematic_2.add_content("default:dirt".to_string());
        schematic_2
            .fill(
                (0, 0, 0).try_into().unwrap(),
                (2, 2, 2).try_into().unwrap(),
                &Node::with_content_index(1),
            )
            .unwrap();

        schematic_1
            .merge(&schematic_2, (0, 0, 0).try_into().unwrap())
            .unwrap();

        assert!(schematic_1.validate().is_ok());
        assert_eq!(
            schematic_1.content_names,
            &["air", "something", "default:dirt"]
        );
        assert!(
            schematic_1.nodes.iter().all(|node| node.content_index == 2),
            "Content indexes of Nodes were not updated correctly"
        );
    }

    #[test]
    fn test_merge_small_schematic_into_larger() {
        let mut schematic_1 = Schematic::new((8, 8, 8).try_into().unwrap());
        schematic_1.add_content("something".to_string());

        let mut schematic_2 = Schematic::new((2, 2, 2).try_into().unwrap());
        schematic_2.add_content("default:dirt".to_string());
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
        let mut schematic = Schematic::new((2, 1, 2).try_into().unwrap());
        let content_index = schematic.add_content("default:cobble".to_string());
        let node = Node::with_content_index(content_index);

        schematic.insert_layer(1, &node).unwrap();

        assert_eq!(schematic.dimensions.y, 2);
        schematic.validate().unwrap();
        assert_eq!(
            schematic.node_at((0, 1, 0).try_into().unwrap()),
            Some(&node)
        );
        assert!(
            schematic.nodes[2..=3]
                .iter()
                .all(|node| node.content_index == 1)
        );
        assert!(
            schematic.nodes[6..=7]
                .iter()
                .all(|node| node.content_index == 1)
        );
    }

    #[test]
    fn test_insert_layer_bottom_layer() {
        let mut schematic = Schematic::new((3, 3, 3).try_into().unwrap());
        let content_index = schematic.add_content("default:cobble".to_string());
        let node = Node::with_content_index(content_index);
        schematic.validate().unwrap();

        schematic.insert_layer(0, &node).unwrap();

        assert_eq!(schematic.dimensions.y, 4);
        schematic.validate().unwrap();
        assert_eq!(
            schematic.node_at((0, 0, 0).try_into().unwrap()),
            Some(&node)
        );
        assert!(
            schematic.nodes[0..=2]
                .iter()
                .all(|node| node.content_index == 1)
        );
        assert!(
            schematic.nodes[12..=14]
                .iter()
                .all(|node| node.content_index == 1)
        );
        assert!(
            schematic.nodes[24..=26]
                .iter()
                .all(|node| node.content_index == 1)
        );
    }

    #[fixture]
    fn schematic() -> Schematic {
        Schematic {
            version: 4,
            dimensions: (3, 2, 3).try_into().unwrap(),
            layer_probabilities: vec![SpawnProbability::Always, SpawnProbability::Always],
            content_names: vec!["default:cobble".try_into().unwrap(), "air".into()],
            nodes: vec![Node::new(0, SpawnProbability::Always, true, 0); 18],
        }
    }
}
