#[derive(Debug, PartialEq, Eq)]
pub struct Schematic {
    pub(crate) version: u16,
    pub(crate) dimensions: Dimensions,
    pub(crate) layer_probabilities: Vec<SpawnProbability>,
    /// Called "name ids" in the file format documentation, it's an array of strings that identify
    /// the contents of a node, i.e. the type of block or items like torches.
    ///
    /// Examples of names are: "air", "default:cobble", "mcl_core:quartz"
    pub(crate) content_names: Vec<String>,
    pub(crate) nodes: Vec<Node>,
}

impl Schematic {
    pub fn new(dimensions: Dimensions) -> Self {
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

    pub fn nodes(&self) -> NodeIterator<'_> {
        NodeIterator::from_schematic(self)
    }

    pub fn node_at(&self, coordinates: Point3) -> Option<&Node> {
        if coordinates.x >= self.dimensions.x
            || coordinates.y >= self.dimensions.y
            || coordinates.z >= self.dimensions.z
        {
            return None;
        }

        let index = (coordinates.z * (self.dimensions.y * self.dimensions.x)
            + coordinates.y * self.dimensions.x
            + coordinates.x) as usize;

        self.nodes.get(index)
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }
}

pub struct NodeIterator<'schematic> {
    current_x: u16,
    current_y: u16,
    current_z: u16,
    schematic: &'schematic Schematic,
    nodes_iter: std::slice::Iter<'schematic, Node>,
}

impl<'schematic> NodeIterator<'_> {
    fn from_schematic(schematic: &'schematic Schematic) -> NodeIterator<'schematic> {
        NodeIterator {
            current_x: 0,
            current_y: 0,
            current_z: 0,
            schematic,
            nodes_iter: schematic.nodes.iter(),
        }
    }
}

impl<'schematic> Iterator for NodeIterator<'schematic> {
    type Item = AnnotatedNode<'schematic>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = match self.nodes_iter.next() {
            Some(node) => {
                let coordinates = Point3 {
                    x: self.current_x,
                    y: self.current_y,
                    z: self.current_z,
                };

                AnnotatedNode { coordinates, node }
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

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct AnnotatedNode<'node> {
    pub coordinates: Point3,
    pub node: &'node Node,
}

impl Node {
    pub fn new(
        content: u16,
        probability: SpawnProbability,
        force_placement: bool,
        param2: u8,
    ) -> Self {
        Node {
            content_index: content,
            probability,
            force_placement,
            param2,
        }
    }
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
            127 => SpawnProbability::Always,
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Point3 {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}

impl From<(u16, u16, u16)> for Point3 {
    fn from(value: (u16, u16, u16)) -> Self {
        Point3 {
            x: value.0,
            y: value.1,
            z: value.2,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Dimensions {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}

impl From<(u16, u16, u16)> for Dimensions {
    fn from(value: (u16, u16, u16)) -> Self {
        Dimensions {
            x: value.0,
            y: value.1,
            z: value.2,
        }
    }
}

impl Dimensions {
    pub fn volume(&self) -> usize {
        self.x as usize * self.y as usize * self.z as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_iterator() {
        let schematic = schematic_fixture();

        let mut nodes_iter = schematic.nodes();

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 0).into());
        assert_eq!(annotated_node.node, &schematic.nodes[0]);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (1, 0, 0).into());
        assert_eq!(annotated_node.node, &schematic.nodes[1]);

        let mut nodes_iter = nodes_iter.skip(1);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 1, 0).into());
        assert_eq!(annotated_node.node, &schematic.nodes[4]);

        let mut nodes_iter = nodes_iter.skip(2);
        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (0, 0, 1).into());
        assert_eq!(annotated_node.node, &schematic.nodes[6]);

        let mut nodes_iter = nodes_iter.skip(10);

        let annotated_node = nodes_iter.next().unwrap();
        assert_eq!(annotated_node.coordinates, (2, 1, 2).into());
        assert_eq!(annotated_node.node, &schematic.nodes[17]);
    }

    #[test]
    fn test_node_at() {
        let schematic = schematic_fixture();

        assert_eq!(
            schematic.node_at((0, 0, 0).into()).unwrap(),
            &schematic.nodes[0]
        );

        assert_eq!(
            schematic.node_at((1, 1, 1).into()).unwrap(),
            &schematic.nodes[10]
        );
    }

    #[test]
    fn test_merge_small_schematic_into_larger() {
        let mut schematic_1 = Schematic::new((8, 8, 8).into());
        schematic_1.add_content("something".to_string());

        let mut schematic_2 = Schematic::new((2, 2, 2).into());
        schematic_2.add_content("default:dirt".to_string());
        schematic_2
            .fill(
                (0, 0, 0).into(),
                (2, 2, 2).into(),
                &Node::with_content_index(1),
            )
            .unwrap();

        schematic_1.merge(&schematic_2, (1, 1, 1).into()).unwrap();

        assert!(schematic_1.validate().is_ok());
        assert_eq!(
            schematic_1.content_names,
            &["air", "something", "default:dirt"]
        );
    }

    fn schematic_fixture() -> Schematic {
        Schematic {
            version: 4,
            dimensions: (3, 2, 3).into(),
            layer_probabilities: vec![SpawnProbability::Always, SpawnProbability::Always],
            content_names: vec!["default:cobble".into(), "air".into()],
            nodes: vec![Node::new(0, SpawnProbability::Always, true, 0); 18],
        }
    }
}
