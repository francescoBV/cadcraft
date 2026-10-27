//! The dimension style variables as a table the Dimension Style Manager (and agents) read: for
//! each [`DimStyle`] field, the tab of AutoCAD's Modify Dimension Style dialog it is edited on, a
//! label, and how it is entered (with the choices of the list ones). The UI builds its tabs from
//! this table, so a variable added here shows up in the dialog.

use cadcraft_doc::{DimStyle, Lineweight};
use serde_json::{Value, json};

/// How a dimension style variable is entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimPropKind {
    /// A length in drawing units.
    Distance,
    /// A plain number (a factor or a scale).
    Number,
    /// Degrees.
    Angle,
    Bool,
    /// Numbers with labels.
    Choice(&'static [(i64, &'static str)]),
    /// Decimal places 0–8.
    Precision,
    Color,
    TextStyle,
    /// A linetype name; "" is ByBlock.
    Linetype,
    /// Hundredths of a millimetre, or -1 ByLayer, -2 ByBlock, -3 Default.
    Lineweight,
    /// An arrowhead name ("" is closed filled).
    Arrow,
    /// The decimal separator.
    Separator,
    /// A DIMPOST / DIMAPOST template, edited as a prefix and a suffix.
    Post,
    /// Zero suppression bits: leading and trailing (DIMZIN 4 and 8, DIMAZIN 1 and 2).
    Zeros {
        leading: u64,
        trailing: u64,
    },
}

/// One dimension style variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimProp {
    /// The [`DimStyle`] field (camelCase), as `dimstyle` and `dimoverride` take it.
    pub field: &'static str,
    /// The tab of the Modify Dimension Style dialog ([`DIM_TABS`]).
    pub tab: usize,
    /// The group within the tab (a frame of the dialog).
    pub group: &'static str,
    pub label: &'static str,
    pub kind: DimPropKind,
}

/// The tabs of the Modify Dimension Style dialog, in AutoCAD's order.
pub const DIM_TABS: [&str; 7] = ["Lines", "Symbols and Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"];

const fn p(field: &'static str, tab: usize, group: &'static str, label: &'static str, kind: DimPropKind) -> DimProp {
    DimProp { field, tab, group, label, kind }
}

const TEXT_POS_HOR: &[(i64, &str)] = &[(0, "Centered"), (1, "At Ext Line 1"), (2, "At Ext Line 2"), (3, "Over Ext Line 1"), (4, "Over Ext Line 2")];
const TEXT_POS_VERT: &[(i64, &str)] = &[(0, "Centered"), (1, "Above"), (2, "Outside"), (3, "JIS"), (4, "Below")];
const FIT: &[(i64, &str)] = &[(0, "Both text and arrows"), (1, "Arrows"), (2, "Text"), (3, "Either text or arrows (best fit)")];
const TEXT_MOVE: &[(i64, &str)] =
    &[(0, "Beside the dimension line"), (1, "Over dimension line, with leader"), (2, "Over dimension line, without leader")];
const LINEAR_UNITS: &[(i64, &str)] =
    &[(1, "Scientific"), (2, "Decimal"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional"), (6, "Windows Desktop")];
const ALT_UNITS: &[(i64, &str)] = &[
    (1, "Scientific"),
    (2, "Decimal"),
    (3, "Engineering"),
    (4, "Architectural Stacked"),
    (5, "Fractional Stacked"),
    (6, "Architectural"),
    (7, "Fractional"),
    (8, "Windows Desktop"),
];
const ANGLE_FORMAT: &[(i64, &str)] = &[(0, "Decimal Degrees"), (1, "Degrees Minutes Seconds"), (2, "Gradians"), (3, "Radians")];
const FRACTION_FORMAT: &[(i64, &str)] = &[(0, "Horizontal"), (1, "Diagonal"), (2, "Not Stacked")];
const TOL_POS: &[(i64, &str)] = &[(0, "Bottom"), (1, "Middle"), (2, "Top")];
const ARC_SYMBOL: &[(i64, &str)] = &[(0, "Preceding dimension text"), (1, "Above dimension text"), (2, "None")];
const TEXT_FILL: &[(i64, &str)] = &[(0, "None"), (1, "Background"), (2, "Color")];
const TEXT_DIRECTION: &[(i64, &str)] = &[(0, "Left-to-Right"), (1, "Right-to-Left")];

use DimPropKind as K;

/// Every modelled dimension style variable, in the order the dialog shows them.
pub const DIM_PROPS: &[DimProp] = &[
    // Lines
    p("dimLineColor", 0, "Dimension lines", "Color", K::Color),
    p("dimLinetype", 0, "Dimension lines", "Linetype", K::Linetype),
    p("dimLineweight", 0, "Dimension lines", "Lineweight", K::Lineweight),
    p("dimLineExtend", 0, "Dimension lines", "Extend beyond ticks", K::Distance),
    p("baselineSpacing", 0, "Dimension lines", "Baseline spacing", K::Distance),
    p("suppressDim1", 0, "Dimension lines", "Suppress dim line 1", K::Bool),
    p("suppressDim2", 0, "Dimension lines", "Suppress dim line 2", K::Bool),
    p("extLineColor", 0, "Extension lines", "Color", K::Color),
    p("extLinetype1", 0, "Extension lines", "Linetype ext line 1", K::Linetype),
    p("extLinetype2", 0, "Extension lines", "Linetype ext line 2", K::Linetype),
    p("extLineweight", 0, "Extension lines", "Lineweight", K::Lineweight),
    p("suppressExt1", 0, "Extension lines", "Suppress ext line 1", K::Bool),
    p("suppressExt2", 0, "Extension lines", "Suppress ext line 2", K::Bool),
    p("extExtend", 0, "Extension lines", "Extend beyond dim lines", K::Distance),
    p("extOffset", 0, "Extension lines", "Offset from origin", K::Distance),
    p("fixedExt", 0, "Extension lines", "Fixed length extension lines", K::Bool),
    p("fixedExtLength", 0, "Extension lines", "Length", K::Distance),
    // Symbols and Arrows
    p("arrowBlock1", 1, "Arrowheads", "First", K::Arrow),
    p("arrowBlock2", 1, "Arrowheads", "Second", K::Arrow),
    p("leaderArrowBlock", 1, "Arrowheads", "Leader", K::Arrow),
    p("arrowSize", 1, "Arrowheads", "Arrow size", K::Distance),
    p("tickSize", 1, "Arrowheads", "Tick size", K::Distance),
    p("centerMark", 1, "Center marks", "Center mark size (negative: lines)", K::Distance),
    p("breakSize", 1, "Dimension Break", "Break size", K::Distance),
    p("arcSymbol", 1, "Arc length symbol", "Arc length symbol", K::Choice(ARC_SYMBOL)),
    p("jogAngle", 1, "Radius jog dimension", "Jog angle", K::Angle),
    p("jogHeight", 1, "Linear jog dimension", "Jog height factor (× text height)", K::Number),
    // Text
    p("textStyle", 2, "Text appearance", "Text style", K::TextStyle),
    p("textColor", 2, "Text appearance", "Text color", K::Color),
    p("textFill", 2, "Text appearance", "Fill", K::Choice(TEXT_FILL)),
    p("textFillColor", 2, "Text appearance", "Fill color", K::Color),
    p("textHeight", 2, "Text appearance", "Text height", K::Distance),
    p("tolScale", 2, "Text appearance", "Fraction height scale", K::Number),
    p("textAbove", 2, "Text placement", "Vertical", K::Choice(TEXT_POS_VERT)),
    p("textJust", 2, "Text placement", "Horizontal", K::Choice(TEXT_POS_HOR)),
    p("textDirection", 2, "Text placement", "View direction", K::Choice(TEXT_DIRECTION)),
    p("textGap", 2, "Text placement", "Offset from dim line", K::Distance),
    p("textVerticalPos", 2, "Text placement", "Vertical position (× text height)", K::Number),
    p("textInsideHorizontal", 2, "Text alignment", "Horizontal inside", K::Bool),
    p("textOutsideHorizontal", 2, "Text alignment", "Horizontal outside", K::Bool),
    // Fit
    p("fit", 3, "Fit options", "If there isn't room for text and arrows, move outside", K::Choice(FIT)),
    p("textInside", 3, "Fit options", "Always keep text between ext lines", K::Bool),
    p("suppressOutsideArrows", 3, "Fit options", "Suppress arrows if they don't fit", K::Bool),
    p("textMove", 3, "Text placement", "When text is not in the default position, place it", K::Choice(TEXT_MOVE)),
    p("scale", 3, "Scale for dimension features", "Use overall scale of", K::Number),
    p("userPositionedText", 3, "Fine tuning", "Place text manually", K::Bool),
    p("forceLineInside", 3, "Fine tuning", "Draw dim line between ext lines", K::Bool),
    // Primary Units
    p("linearUnit", 4, "Linear dimensions", "Unit format", K::Choice(LINEAR_UNITS)),
    p("decimals", 4, "Linear dimensions", "Precision", K::Precision),
    p("fractionFormat", 4, "Linear dimensions", "Fraction format", K::Choice(FRACTION_FORMAT)),
    p("decimalSeparator", 4, "Linear dimensions", "Decimal separator", K::Separator),
    p("round", 4, "Linear dimensions", "Round off", K::Distance),
    p("post", 4, "Linear dimensions", "Prefix and suffix", K::Post),
    p("linearFactor", 4, "Measurement scale", "Scale factor", K::Number),
    p("zeroSuppression", 4, "Zero suppression", "Zero suppression", K::Zeros { leading: 4, trailing: 8 }),
    p("angularUnit", 4, "Angular dimensions", "Units format", K::Choice(ANGLE_FORMAT)),
    p("angularDecimals", 4, "Angular dimensions", "Precision", K::Precision),
    p("angularZeroSuppression", 4, "Angular dimensions", "Zero suppression", K::Zeros { leading: 1, trailing: 2 }),
    // Alternate Units
    p("alt", 5, "Alternate units", "Display alternate units", K::Bool),
    p("altUnit", 5, "Alternate units", "Unit format", K::Choice(ALT_UNITS)),
    p("altDecimals", 5, "Alternate units", "Precision", K::Precision),
    p("altFactor", 5, "Alternate units", "Multiplier for alt units", K::Number),
    p("altRound", 5, "Alternate units", "Round distances to", K::Number),
    p("altPost", 5, "Alternate units", "Prefix and suffix", K::Post),
    p("altZeroSuppression", 5, "Zero suppression", "Zero suppression", K::Zeros { leading: 4, trailing: 8 }),
    // Tolerances
    p("tolerance", 6, "Tolerance format", "Show tolerances", K::Bool),
    p("limits", 6, "Tolerance format", "Limits", K::Bool),
    p("tolDecimals", 6, "Tolerance format", "Precision", K::Precision),
    p("tolPlus", 6, "Tolerance format", "Upper value", K::Number),
    p("tolMinus", 6, "Tolerance format", "Lower value", K::Number),
    p("tolJust", 6, "Tolerance format", "Vertical position", K::Choice(TOL_POS)),
    p("tolZeroSuppression", 6, "Tolerance format", "Zero suppression", K::Zeros { leading: 4, trailing: 8 }),
    p("altTolDecimals", 6, "Alternate unit tolerance", "Precision", K::Precision),
    p("altTolZeroSuppression", 6, "Alternate unit tolerance", "Zero suppression", K::Zeros { leading: 4, trailing: 8 }),
];

/// The arrowheads drawn in code, by the DIMBLK name stored for them and their label.
pub const ARROWHEADS: &[(&str, &str)] = &[
    ("", "Closed filled"),
    ("_CLOSEDBLANK", "Closed blank"),
    ("_CLOSED", "Closed"),
    ("_DOT", "Dot"),
    ("_ARCHTICK", "Architectural tick"),
    ("_OBLIQUE", "Oblique"),
    ("_OPEN", "Open"),
    ("_ORIGIN", "Origin indicator"),
    ("_ORIGIN2", "Origin indicator 2"),
    ("_OPEN90", "Right angle"),
    ("_OPEN30", "Open 30"),
    ("_DOTSMALL", "Dot small"),
    ("_DOTBLANK", "Dot blank"),
    ("_SMALL", "Dot small blank"),
    ("_BOXBLANK", "Box"),
    ("_BOXFILLED", "Box filled"),
    ("_DATUMBLANK", "Datum triangle"),
    ("_DATUMFILLED", "Datum triangle filled"),
    ("_INTEGRAL", "Integral"),
    ("_NONE", "None"),
];

/// A value a list variable can take, with its label.
#[derive(Clone, Debug, PartialEq)]
pub struct DimChoice {
    pub value: Value,
    pub label: String,
}

/// The variable for style field `field` (case-insensitive), if the table has it.
pub fn dim_prop(field: &str) -> Option<&'static DimProp> {
    DIM_PROPS.iter().find(|p| p.field.eq_ignore_ascii_case(field))
}

/// The variables of tab `tab` ([`DIM_TABS`]).
pub fn tab_props(tab: usize) -> impl Iterator<Item = &'static DimProp> {
    DIM_PROPS.iter().filter(move |p| p.tab == tab)
}

impl DimPropKind {
    /// The values a list kind takes, with their labels (none for the other kinds; text styles and
    /// linetypes are the drawing's).
    pub fn options(self) -> Vec<DimChoice> {
        let c = |value: Value, label: String| DimChoice { value, label };
        match self {
            K::Choice(list) => list.iter().map(|(v, l)| c(json!(v), (*l).to_string())).collect(),
            K::Precision => (0..=8u8).map(|n| c(json!(n), if n == 0 { "0".into() } else { format!("0.{}", "0".repeat(usize::from(n))) })).collect(),
            K::Lineweight => {
                let mut o = vec![c(json!(-2), "ByBlock".into()), c(json!(-1), "ByLayer".into()), c(json!(-3), "Default".into())];
                o.extend(Lineweight::STANDARD.iter().map(|w| c(json!(w), Lineweight::Mm100(*w).name())));
                o
            }
            K::Arrow => ARROWHEADS.iter().map(|(v, l)| c(json!(v), (*l).to_string())).collect(),
            K::Separator => {
                [(".", "'.' (Period)"), (",", "',' (Comma)"), (" ", "' ' (Space)")].iter().map(|(v, l)| c(json!(v), (*l).to_string())).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// `name` normalized as the renderer reads arrowhead names ("_DOT", "dot" and "Dot" alike).
fn arrow_key(name: &str) -> String {
    name.trim().trim_start_matches('_').chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// A style field's value as the dialog shows it: the arrowhead an empty per-end arrowhead draws
/// (DIMBLK's), a standard arrowhead by its table name; other fields as stored.
pub fn dim_value(p: &DimProp, st: &Value) -> Value {
    let v = st.get(p.field).cloned().unwrap_or(Value::Null);
    match p.kind {
        K::Arrow => {
            let name = v.as_str().filter(|s| !s.is_empty() || !matches!(p.field, "arrowBlock1" | "arrowBlock2")).map(str::to_string);
            let name = name.unwrap_or_else(|| st.get("arrowBlock").and_then(Value::as_str).unwrap_or_default().to_string());
            match ARROWHEADS.iter().find(|(n, _)| arrow_key(n) == arrow_key(&name) || (n.is_empty() && arrow_key(&name) == "closedfilled")) {
                Some((n, _)) => json!(n),
                None => json!(name),
            }
        }
        _ => v,
    }
}

/// The prefix and suffix of a DIMPOST/DIMAPOST template: around `<>`, or the whole template as a
/// suffix when it has no `<>` (as the renderer reads it).
pub fn split_post(template: &str) -> (String, String) {
    match template.split_once("<>") {
        Some((pre, post)) => (pre.to_string(), post.to_string()),
        None => (String::new(), template.to_string()),
    }
}

/// A DIMPOST/DIMAPOST template from a prefix and a suffix.
pub fn join_post(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() { suffix.to_string() } else { format!("{prefix}<>{suffix}") }
}

/// The DIM* system variable of a style field ("" for the settings that have none).
pub fn dim_var(field: &str) -> &'static str {
    cadcraft_doc::DIMVARS.iter().find(|(_, f)| *f == field).map_or("", |(v, _)| *v)
}

/// A style as the JSON object of its fields (camelCase).
pub fn style_fields(st: &DimStyle) -> serde_json::Map<String, Value> {
    match serde_json::to_value(st) {
        Ok(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_field_is_in_the_table_once() {
        let fields = style_fields(&DimStyle::default());
        for k in fields.keys().filter(|k| !matches!(k.as_str(), "name" | "annotative" | "arrowBlock")) {
            assert_eq!(DIM_PROPS.iter().filter(|p| p.field == k).count(), 1, "{k}");
        }
        for p in DIM_PROPS {
            assert!(fields.contains_key(p.field), "{}", p.field);
            assert!(p.tab < DIM_TABS.len());
        }
    }

    #[test]
    fn posts_and_arrows() {
        assert_eq!(split_post("Ø<> mm"), ("Ø".into(), " mm".into()));
        assert_eq!(join_post("", "mm"), "mm");
        assert_eq!(join_post("R", ""), "R<>");
        let st = json!({ "arrowBlock": "_Dot", "arrowBlock1": "" });
        assert_eq!(dim_value(dim_prop("arrowBlock1").unwrap(), &st), json!("_DOT"));
        assert_eq!(dim_var("textFill"), "DIMTFILL");
        assert_eq!(dim_var("breakSize"), "");
    }
}
