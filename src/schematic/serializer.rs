use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::Schematic;
use super::parser::MTS_MAGIC_BYTES;

/// Converts the given `Schematic` into a byte format that Luanti can load.
pub(super) fn to_bytes(schematic: &Schematic) -> Vec<u8> {
    let mut output = Vec::new();

    output.extend(MTS_MAGIC_BYTES);
    output.extend(schematic.version.to_be_bytes());
    output.extend(schematic.dimensions.x.to_be_bytes());
    output.extend(schematic.dimensions.y.to_be_bytes());
    output.extend(schematic.dimensions.z.to_be_bytes());

    output.extend(
        schematic
            .layer_probabilities
            .iter()
            .map(|p| (u8::from(p)).to_be()),
    );

    output.extend((schematic.content_names.len() as u16).to_be_bytes());
    for content_name in &schematic.content_names {
        output.extend((content_name.len() as u16).to_be_bytes());
        output.extend(content_name.as_bytes());
    }

    // Node data is stored with zlib compression
    let mut node_data: Vec<u8> = Vec::with_capacity(schematic.num_nodes() * 4);
    node_data.extend(
        schematic
            .nodes
            .iter()
            .flat_map(|node| node.content_id.to_be_bytes()),
    );

    node_data.extend(
        schematic
            .nodes
            .iter()
            .map(|node| u8::from(node.force_placement) << 7 | u8::from(node.probability)),
    );

    node_data.extend(schematic.nodes.iter().map(|node| node.param2));

    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor
        .write_all(&node_data)
        .expect("node data should be compressed");
    let compressed_data = compressor.finish().expect("zlib compressed data");
    output.extend(&compressed_data);

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::schematic::parser::parse;

    #[test]
    fn test_to_bytes() {
        let original_data = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/3x3.mts"));
        let original_schematic = parse(original_data).unwrap();

        let serialized_schematic = to_bytes(&original_schematic);
        // The original data and serialized schematic don't always compare byte for byte because of
        // variation the zlib compression, so the best we can do here is re-parse the serialized
        // schematic and see if that comes out the same as the originally parsed schematic. The
        // game handles different zlib compression levels just fine.
        let reparsed_schematic = parse(&serialized_schematic).unwrap();

        assert_eq!(original_schematic, reparsed_schematic);
    }
}
