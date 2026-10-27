//! `dimstyle.overrides` sets the current dimension style's overrides (what the Dimension Style
//! Manager's Override edits) in one undo step, and `dimstyle` redraws the dimensions of the
//! style it changes.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn style_overrides_are_set_in_one_step() {
    let mut s = Session::new();
    s.execute("dimstyle.overrides", &json!({ "textHeight": 0.3, "DIMCLRT": 1 })).unwrap();
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!(o.get("textHeight"), Some(&json!(0.3)));
    assert!(o.contains_key("textColor"), "{o:?}");
    s.undo().unwrap();
    assert!(s.doc().unwrap().dim_overrides().is_empty(), "one undo step");
    // Unusable values change nothing; names that aren't variables are refused.
    assert!(s.execute("dimstyle.overrides", &json!({ "textHeight": -1, "DIMDEC": 2 })).is_err());
    assert!(s.execute("dimstyle.overrides", &json!({ "nope": 1 })).is_err());
    assert!(s.doc().unwrap().dim_overrides().is_empty());
    s.execute("dimstyle.overrides", &json!({ "DIMDEC": 2 })).unwrap();
    // Clear drops them only when every value is usable.
    assert!(s.execute("dimstyle.overrides", &json!({ "clear": true, "textHeight": -1 })).is_err());
    assert!(s.execute("dimstyle.overrides", &json!({ "clear": true, "nope": 1 })).is_err());
    assert_eq!(s.doc().unwrap().dim_overrides().get("decimals"), Some(&json!(2)));
    s.execute("dimstyle.overrides", &json!({ "clear": true, "textGap": 0.05 })).unwrap();
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!((o.len(), o.get("textGap")), (1, Some(&json!(0.05))));
    s.execute("dimstyle.overrides", &json!({ "clear": true })).unwrap();
    assert!(s.doc().unwrap().dim_overrides().is_empty());
    // New dimensions take them.
    s.execute("dimstyle.overrides", &json!({ "DIMDEC": 1 })).unwrap();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let d = s.doc().unwrap();
    let Some(EntityKind::Dimension(dm)) = d.model.last().map(|e| &e.kind) else { panic!("no dimension") };
    assert_eq!(dm.overrides.get("decimals"), Some(&json!(1)), "{:?}", dm.overrides);
}

#[test]
fn a_modified_style_redraws_dimensions_that_kept_a_file_block() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle;
    // As a file read gives it: a `*D` block drawn with the old settings.
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.block = Some("*D1".into());
            }
        })
        .unwrap();
    s.execute("dimstyle", &json!({ "name": "Standard", "DIMDEC": 1 })).unwrap();
    let Some(EntityKind::Dimension(dm)) = s.doc().unwrap().entity(h).map(|e| e.kind.clone()) else { panic!("no dimension") };
    assert_eq!(dm.block, None, "drawn again from the style");
}

#[test]
fn hostile_overrides_never_panic() {
    let mut s = Session::new();
    for p in
        [json!({ "DIMTXT": "x" }), json!({ "DIMBLK": [1] }), json!({ "DIMCLRT": { "a": 1 } }), json!({ "clear": "yes" }), json!(null), json!([1])]
    {
        let _ = s.execute("dimstyle.overrides", &p);
    }
}
