use super::common::Handle;
use super::*;

/// A hatch boundary loop: closed polyline with bulges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchLoop {
    pub vertices: Vec<PolyVertex>,
    #[serde(default)]
    pub outer: bool,
    /// The objects the loop was made from (DXF group 97 count, 330 handles): an associative
    /// hatch re-fills the loop from them when they change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Handle>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hatch {
    pub pattern: String,
    pub solid: bool,
    pub loops: Vec<HatchLoop>,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub associative: bool,
    /// 0 normal (odd parity), 1 outer, 2 ignore.
    #[serde(default)]
    pub style: u8,
    #[serde(default)]
    pub elevation: f64,
    /// Gradient fill: name + second colour.
    #[serde(default)]
    pub gradient: Option<Gradient>,
    #[serde(default)]
    pub origin: Vec2,
    #[serde(default)]
    pub background: Option<Color>,
    /// The pattern's line families as a file defined them, for a pattern that isn't in the
    /// standard library (unit scale and angle, like the library's; empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_lines: Vec<crate::library::PatternLine>,
    /// Set for an MPOLYGON: a hatch that draws its own boundary (its loops, in the entity's
    /// colour) and fills in a colour of its own. It is never associative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpolygon: Option<MPolygon>,
}

/// What an MPOLYGON adds to a hatch (DXF Reference, MPOLYGON: the fill colour and the pattern's
/// X direction of its `AcDbMPolygon` record).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MPolygon {
    /// The fill's colour (pattern lines or solid fill); ByLayer follows the entity's layer.
    pub fill_color: Color,
    /// The pattern's X direction as a file gave it (kept for the round trip).
    #[serde(default)]
    pub x_dir: Vec2,
}

impl Default for MPolygon {
    fn default() -> Self {
        MPolygon { fill_color: Color::ByLayer, x_dir: Vec2::X }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gradient {
    pub name: String,
    pub color1: Color,
    pub color2: Color,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub centered: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wipeout {
    pub boundary: Vec<Vec2>,
}
