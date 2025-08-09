use std::collections::HashMap;

use ndarray::{Array3, AssignElem, s};

use crate::error::Error;
use crate::node::{Node, NodeSpace, SpawnProbability};
use crate::vector::MapVector;

use super::Schematic;

pub(super) fn fill(
    destination: &mut Schematic,
    from_position: MapVector,
    fill_space: MapVector,
    node: &Node,
) -> Result<(), Error> {
    let to: MapVector = from_position
        .checked_add(fill_space)
        .ok_or(Error::OutOfBounds)?;
    if to > destination.dimensions {
        return Err(Error::OutOfBounds);
    }

    let from_shape = from_position.as_shape();
    let to_shape = to.as_shape();

    destination
        .nodes
        .slice_mut(s![
            from_shape.0..to_shape.0,
            from_shape.1..to_shape.1,
            from_shape.2..to_shape.2
        ])
        .fill(*node);

    Ok(())
}

pub(super) fn insert_layer(
    schematic: &Schematic,
    y: u16,
    fill_with_node: &Node,
) -> Result<Schematic, Error> {
    if y > schematic.dimensions.y {
        return Err(Error::OutOfBounds);
    }

    let new_dimensions = schematic
        .dimensions
        .checked_add((0, 1, 0).try_into()?)
        .ok_or(Error::OutOfBounds)?;

    let mut extended_nodes = Array3::from_elem(new_dimensions.as_shape(), *fill_with_node);

    // Copy all nodes above the new layer
    let y = y as usize;
    schematic
        .nodes
        .slice(s![.., 0..y, ..])
        .assign_to(&mut extended_nodes.slice_mut(s![.., 0..y, ..]));

    // Copy all nodes below the new layer
    schematic
        .nodes
        .slice(s![.., y.., ..])
        .assign_to(&mut extended_nodes.slice_mut(s![.., y + 1.., ..]));

    // TODO Like with from_bytes(), this could do with a better constructor
    let mut new_schematic = Schematic {
        version: schematic.version,
        dimensions: new_dimensions,
        layer_probabilities: schematic.layer_probabilities.clone(),
        content_names: schematic.content_names.clone(),
        nodes: extended_nodes,
    };

    new_schematic
        .layer_probabilities
        .insert(y, SpawnProbability::Always);

    Ok(new_schematic)
}

pub(super) fn merge<'schematic>(
    source: &'schematic impl NodeSpace<'schematic>,
    destination: &mut Schematic,
    merge_at: MapVector,
) -> Result<(), Error> {
    let merge_end = merge_at
        .checked_add(source.dimensions())
        .ok_or(Error::OutOfBounds)?;
    if merge_end > destination.dimensions {
        return Err(Error::OutOfBounds);
    }

    let current_content_positions: HashMap<String, usize> = destination
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
    for (source_content_id, content_name) in source.content_names().enumerate() {
        match current_content_positions.get(content_name) {
            // Content already exists in this Schematic, but might be at a different index than
            // at the source Schematic.
            Some(current_content_id) => {
                if *current_content_id != source_content_id {
                    source_content_map.insert(source_content_id as u16, *current_content_id as u16);
                }
            }
            // Content isn't present in this Schematic yet
            None => {
                destination.content_names.push(content_name.to_string());
                let new_content_id = destination.content_names.len() - 1;
                source_content_map.insert(source_content_id as u16, new_content_id as u16);
            }
        }
    }

    // These two content IDs are for blocks that are considered by Luanti as "nothing" when it
    // comes to deciding whether a node should overwrite the existing position, and the new node is
    // marked as "force_placement = false"
    let content_air = destination.content_id_for_name("air");
    let content_ignore = destination.content_id_for_name("ignore");

    let from_shape = merge_at.as_shape();
    let to_shape = merge_end.as_shape();
    let slice = s![
        from_shape.0..to_shape.0,
        from_shape.1..to_shape.1,
        from_shape.2..to_shape.2
    ];

    let target_space = destination.nodes.slice_mut(slice);

    // This does the actual merging
    ndarray::Zip::from(&source.nodes())
        // The reason for not using `map_assign_into()` here is that that function doesn't pass
        // the target `into` slice into the closure, so we aren't able to make any comparisons
        // to the original node.
        .and(target_space)
        .for_each(move |merge_node, target_node| {
            // This doesn't take any SpawnProbability::Custom() probability into account, such
            // nodes will just overwrite the current node. The game will then decide whether to
            // spawn the node or not.
            if merge_node.probability == SpawnProbability::Never && !merge_node.force_placement {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    use crate::node::Node;

    #[test]
    fn test_fill() {
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap()).unwrap();
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
        let mut schematic = Schematic::new((2, 2, 2).try_into().unwrap()).unwrap();
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
    fn test_dimensions_checked_add() {
        let dimensions = MapVector::new(1000, 1000, 1000).unwrap();

        assert_eq!(
            dimensions.checked_add((1000, 1000, 1000).try_into().unwrap()),
            Some((2000, 2000, 2000).try_into().unwrap())
        );
    }

    #[test]
    fn test_insert_layer() {
        let mut original_schematic = Schematic::new((2, 1, 2).try_into().unwrap()).unwrap();
        let content_id = original_schematic.register_content("default:cobble".to_string());
        let node = Node::with_content_id(content_id);

        let new_schematic = original_schematic.insert_layer(1, node).unwrap();

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

    #[test]
    fn test_merge() {
        let mut schematic_1 = Schematic::new((3, 3, 3).try_into().unwrap()).unwrap();
        schematic_1.register_content("something".to_string());

        let mut schematic_2 = Schematic::new((3, 2, 2).try_into().unwrap()).unwrap();
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
        let mut schematic_1 = Schematic::new((8, 8, 8).try_into().unwrap()).unwrap();
        schematic_1.register_content("something".to_string());

        let mut schematic_2 = Schematic::new((2, 2, 2).try_into().unwrap()).unwrap();
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
            Schematic::with_nodes((1, 1, 1).try_into().unwrap(), vec![optional_node]).unwrap();

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

    #[fixture]
    fn schematic() -> Schematic {
        let mut schematic = Schematic::with_nodes(
            (3, 2, 3).try_into().unwrap(),
            (1..=18)
                .map(|i| Node::new(i, SpawnProbability::Always, true, 0))
                .collect(),
        )
        .unwrap();
        schematic.register_content("default:cobble".to_string());
        (2..=schematic.num_nodes()).for_each(|i| {
            schematic.register_content(format!("content:{i}"));
        });

        schematic
    }
}
