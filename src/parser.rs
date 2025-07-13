//! Parser for Luanti (formerly Minetest) Schematic files
//!
//! MTS file format documentation:
//! * https://docs.luanti.org/for-creators/luanti-schematic-file-format/
//! * https://github.com/luanti-org/luanti/blob/5.1.0/src/mapgen/mg_schematic.h

use flate2::read::ZlibDecoder;
use std::io::Read;
use std::iter::zip;
use winnow::error::FromExternalError;

use winnow::BStr;
use winnow::Parser;
use winnow::binary::be_u8;
use winnow::binary::be_u16;
use winnow::binary::length_take;
use winnow::combinator::repeat;
use winnow::error::{ContextError, StrContext, StrContextValue};
use winnow::token::literal;

use crate::schematic::Dimensions;
use crate::schematic::Node;
use crate::schematic::Schematic;
use crate::schematic::SpawnProbability;

pub(crate) const MAGIC_BYTES: &[u8; 4] = b"MTSM";

pub fn from_bytes(input: &[u8]) -> Result<Schematic, ContextError> {
    let stream = &mut BStr::new(input);

    verify_magic_bytes(stream)?;

    let version = parse_version(stream)?;
    let dimensions = parse_dimensions(stream)?;
    let layer_probabilities: Vec<SpawnProbability> =
        parse_layer_probabilities(stream, dimensions.y)?;
    let name_ids = parse_name_ids(stream)?;

    // The rest of the data is zlib compressed
    let decompressed = decompress(stream)?;
    let node_stream = &mut BStr::new(&decompressed);

    let num_nodes = dimensions.volume();
    let nodes = parse_nodes(node_stream, num_nodes, name_ids.len())?;

    Ok(Schematic {
        version,
        dimensions,
        layer_probabilities,
        content_names: name_ids,
        nodes,
    })
}

fn parse_nodes(
    node_stream: &mut &BStr,
    num_nodes: usize,
    num_name_ids: usize,
) -> Result<Vec<Node>, ContextError> {
    let node_contents: Vec<u16> =
        repeat(num_nodes, be_u16.verify(|v| (*v as usize) < num_name_ids))
            .context(parser_expected("node contents to point to a valid name_id"))
            .parse_next(node_stream)?;

    let node_params1: Vec<(bool, u8)> = repeat(
        num_nodes,
        be_u8
            .map(|v| ((v & 0x80) > 0, v & 0x7f))
            .verify(|(_force_placement, probability)| is_valid_probability(*probability)),
    )
    .context(parser_expected("a probability value between 0-127"))
    .parse_next(node_stream)?;

    let node_params2: Vec<u8> = repeat(num_nodes, be_u8)
        .context(parser_expected("valid Param2 values for nodes"))
        .parse_next(node_stream)?;

    let nodes: Vec<Node> = zip(node_contents, zip(node_params1, node_params2))
        .map(|(content, ((force_placement, probability), param2))| {
            Node::new(
                content,
                SpawnProbability::from(probability),
                force_placement,
                param2,
            )
        })
        .collect();

    Ok(nodes)
}

fn verify_magic_bytes(stream: &mut &BStr) -> winnow::Result<()> {
    literal::<_, _, ContextError>(MAGIC_BYTES)
        .context(parser_expected("magic header bytes to be \"MTSM\""))
        .parse_next(stream)?;

    Ok(())
}

fn parse_version(stream: &mut &BStr) -> winnow::Result<u16> {
    be_u16
        .verify(|v| *v == 4)
        .context(parser_expected("version 4"))
        .parse_next(stream)
}

fn parse_dimensions(stream: &mut &BStr) -> winnow::Result<Dimensions> {
    let (size_x, size_y, size_z) = (be_u16, be_u16, be_u16).parse_next(stream)?;

    Ok((size_x, size_y, size_z).into())
}

fn parse_layer_probabilities(
    stream: &mut &BStr,
    size_y: u16,
) -> Result<Vec<SpawnProbability>, ContextError> {
    repeat(
        size_y as usize,
        be_u8
            .verify(|v| is_valid_probability(*v))
            .map(SpawnProbability::from),
    )
    .context(parser_expected("a probability value between 0-127"))
    .parse_next(stream)
}

fn parse_name_ids(stream: &mut &BStr) -> winnow::Result<Vec<String>> {
    let name_id_count = be_u16.parse_next(stream)?;

    repeat(
        name_id_count as usize,
        length_take(be_u16)
            .try_map(|bytes| str::from_utf8(bytes))
            .map(|name| name.to_string()),
    )
    .context(parser_expected(
        "a list of node names (items, materials) used in the schematic",
    ))
    .parse_next(stream)
}

fn decompress(stream: &mut &BStr) -> winnow::Result<Vec<u8>> {
    let compressed_size = stream.len();
    let mut decompressor = ZlibDecoder::new(stream.as_ref());

    // The data will be at least this amount of bytes big. How big exactly is not known ahead of
    // time.
    let mut decompressed = Vec::with_capacity(compressed_size);
    decompressor
        .read_to_end(&mut decompressed)
        .map_err(|err| ContextError::from_external_error(stream, err))?;

    Ok(decompressed)
}

/// To describe what was expected during parsing using `context()`, displayed when there are
/// parsing errors.
fn parser_expected(description: &'static str) -> StrContext {
    StrContext::Expected(StrContextValue::Description(description))
}

/// Probability values are between 0 and 127 (inclusive)
fn is_valid_probability(value: u8) -> bool {
    value <= 127
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let data = include_bytes!("../tests/3x3.mts");

        let schematic = from_bytes(data).unwrap();

        assert_eq!(schematic.version, 4);
        assert_eq!(schematic.dimensions.x, 3);
        assert_eq!(schematic.dimensions.y, 2);
        assert_eq!(schematic.dimensions.z, 3);

        assert_eq!(
            &schematic.layer_probabilities,
            &[SpawnProbability::Always, SpawnProbability::Always]
        );
        assert_eq!(schematic.content_names.len(), 7);
        assert_eq!(schematic.content_names[6], "default:pine_wood");
        assert_eq!(schematic.num_nodes(), 18);
    }
}
