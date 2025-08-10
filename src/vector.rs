use crate::error::Error;

/// Luanti's maximum map size is 62013 x 62013 x 62013, from -31006 to 31006 (inclusive), but the
/// schematics file format defines it as an unsigned 16-bit integer.
const MAX_MAP_DIMENSION: u16 = 62013;

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
    /// as a [Schematic](crate::schematic::Schematic)'s nodes.
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
