//! REFEDIT on xrefs and nested references (gap F2): the source drawing written back on REFCLOSE
//! Save (with a backup; refused for read-only, locked or changed files) and nesting levels.
//! Files go in a folder under Cargo's temporary directory for integration tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::{Vec2, Vec3};
use cadcraft_engine::doc::{Block, Circle, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, XrefStatus};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A fresh folder for one test.
fn folder(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("refedit-xref").join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(d: &Drawing, path: &Path) {
    std::fs::write(path, cadcraft_io::write(d, &path.to_string_lossy()).unwrap()).unwrap();
}

fn read(path: &Path) -> Drawing {
    cadcraft_io::read(&std::fs::read(path).unwrap(), &path.to_string_lossy()).unwrap()
}

fn line(d: &mut Drawing, layer: &str, a: (f64, f64), b: (f64, f64)) -> Handle {
    let h = d.new_handle();
    let mut e = Entity::new(h, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
    e.common.layer = layer.into();
    d.model.push(e);
    h
}

fn insert(block: &str, at: Vec3) -> Insert {
    Insert {
        block: block.into(),
        insert: at,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

/// A site plan: a line on layer Walls and a reference to its block Tree (a circle).
fn site() -> (Drawing, Handle) {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), ..Layer::default() });
    let wall = line(&mut d, "Walls", (0.0, 0.0), (10.0, 0.0));
    let mut tree = Block::new("Tree");
    let c = d.new_handle();
    tree.entities.push(Entity::new(c, EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert("Tree".into(), Arc::new(tree));
    let h = d.new_handle();
    d.model.push(Entity::new(h, EntityKind::Insert(insert("Tree", Vec3::new(5.0, 0.0, 0.0)))));
    (d, wall)
}

/// A host drawing (saved as host.dxf) with site.dxf attached at (100, 0).
fn attached(tag: &str) -> (Session, PathBuf, Handle, String) {
    hooks();
    let dir = folder(tag);
    let (d, wall) = site();
    save(&d, &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    let r = s.execute("xattach", &json!({ "path": "site.dxf", "at": [100, 0] })).unwrap();
    let insert = r["handle"].as_str().unwrap().to_string();
    (s, dir, wall, insert)
}

fn hx(v: Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn working(r: &Value) -> Vec<String> {
    r["working"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

fn find_line(s: &Session, hs: &[String]) -> String {
    let d = s.doc().unwrap();
    hs.iter().find(|h| matches!(d.entity(Handle::parse_hex(h).unwrap()).unwrap().kind, EntityKind::Line(_))).unwrap().clone()
}

#[test]
fn editing_an_xref_writes_its_drawing_back_with_a_backup() {
    let (mut s, dir, wall, insert) = attached("save");
    let file = dir.join("site.dxf");
    let r = s.execute("refedit", &json!({ "handle": insert })).unwrap();
    assert_eq!(r["xref"]["name"], "site");
    let work = working(&r);
    assert_eq!(work.len(), 2);
    // Everything but the working set is locked while editing, and drawn faded.
    let l = find_line(&s, &work);
    s.execute("move", &json!({ "handles": [l], "delta": [0, 5] })).unwrap();
    s.execute("layer", &json!({ "option": "make", "name": "Added" })).ok();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let out = s.execute("refclose", &json!({ "option": "save" })).unwrap();
    assert_eq!(out["objects"], 3);
    assert!(out["file"].as_str().unwrap().ends_with("site.dxf"));
    assert!(dir.join("site.bak").is_file(), "the previous file is kept");
    // The file holds the edit, in its own coordinates, with the line's own handle.
    let back = read(&file);
    assert_eq!(back.model.len(), 3);
    let moved = back.entity(wall).expect("the edited line keeps its handle");
    let EntityKind::Line(ln) = &moved.kind else { panic!() };
    assert!((ln.a.y - 5.0).abs() < 1e-9 && (ln.a.x).abs() < 1e-9, "{ln:?}");
    assert_eq!(moved.common.layer, "Walls", "the xref's layer loses its prefix");
    let new_line = back.model.iter().find(|e| matches!(&e.kind, EntityKind::Line(l) if (l.a.y - 20.0).abs() < 1e-9)).unwrap();
    assert!(back.layer(&new_line.common.layer).is_some(), "a host layer the new line uses comes along");
    // The host shows the xref as written (loaded again), and nothing of the edit is left.
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.block("site").unwrap().entities.len(), 3);
    assert_eq!(d.block("site").unwrap().xref_status, XrefStatus::Loaded);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_block_of_the_xref_is_written_into_that_block() {
    let (mut s, dir, _, insert) = attached("nested");
    let levels = s.execute("refedit.levels", &json!({ "handle": insert })).unwrap();
    let names: Vec<&str> = levels["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["site", "site|Tree"]);
    let r = s.execute("refedit", &json!({ "handle": insert, "nested": "site|Tree" })).unwrap();
    assert_eq!(r["block"], "site|Tree");
    assert_eq!(r["path"].as_array().unwrap().len(), 1);
    assert!(r["faded"].as_u64().unwrap() >= 1, "the site's other objects stand faded around it");
    let c = working(&r)[0].clone();
    // The circle shows at the tree's place in the drawing: (100 + 5, 0).
    let EntityKind::Circle(circle) = &s.doc().unwrap().entity(Handle::parse_hex(&c).unwrap()).unwrap().kind else { panic!() };
    assert!((circle.center.x - 105.0).abs() < 1e-9);
    s.execute("scale", &json!({ "handles": [c], "base": [105, 0], "factor": 2 })).unwrap();
    s.execute("refclose", &json!({})).unwrap();
    let back = read(&dir.join("site.dxf"));
    let EntityKind::Circle(c) = &back.block("Tree").unwrap().entities.iter().next().unwrap().kind else { panic!() };
    assert!((c.radius - 2.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn locked_read_only_or_changed_files_are_not_overwritten() {
    // A lock file of another program: refused at the start.
    let (mut s, dir, _, insert) = attached("locked");
    let file = dir.join("site.dxf");
    std::fs::write(dir.join("site.dwl"), b"lock").unwrap();
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("open in another program"), "{e}");
    std::fs::remove_file(dir.join("site.dwl")).unwrap();

    // Changed on disk while editing: the save is refused, the edit stays, Discard ends it.
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    let mut other = read(&file);
    line(&mut other, "0", (0.0, 50.0), (1.0, 50.0));
    std::thread::sleep(std::time::Duration::from_millis(20));
    save(&other, &file);
    let e = s.execute("refclose", &json!({ "option": "save" })).unwrap_err().to_string();
    assert!(e.contains("changed since"), "{e}");
    assert!(s.doc().unwrap().refedit.is_some(), "the edit stays open");
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    assert_eq!(read(&file).model.len(), 3, "the other program's change is kept");

    // Changed since it was loaded: reload first.
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("since") && e.contains("reload"), "{e}");
    s.execute("-xref", &json!({ "option": "reload", "names": "site" })).unwrap();

    // Read-only: refused.
    let mut perm = std::fs::metadata(&file).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&file, perm.clone()).unwrap();
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("read-only"), "{e}");
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&file, perm).unwrap();

    // Open in another drawing tab of this session: refused.
    s.execute("qsave", &json!({})).unwrap();
    let host = s.state().unwrap().path.clone().unwrap();
    s.execute("open", &json!({ "path": file.to_string_lossy() })).unwrap();
    s.execute("open", &json!({ "path": host })).unwrap();
    let ih = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().handle;
    let e = s.execute("refedit", &json!({ "handle": ih.hex() })).unwrap_err().to_string();
    assert!(e.contains("another drawing tab"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nested_block_references_are_picked_through_their_parents() {
    let mut s = Session::new();
    let c = hx(s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Inner", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    let i = hx(s.execute("insert", &json!({ "name": "Inner", "at": [10, 0] })).unwrap());
    let l = hx(s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap());
    s.execute("block", &json!({ "name": "Outer", "base": [0, 0], "handles": [i, l], "keep": "delete" })).unwrap();
    let o = hx(s.execute("insert", &json!({ "name": "Outer", "at": [100, 100] })).unwrap());

    // At the command line: pick, Next to the nested level, OK.
    s.start("refedit").unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&o).unwrap()])).unwrap();
    assert!(s.prompt_text().contains("Select nesting level"), "{}", s.prompt_text());
    assert!(s.log.iter().any(|m| m.contains("Reference name: Outer")));
    s.input(Input::Enter).unwrap(); // Next
    assert!(s.log.iter().any(|m| m.contains("Reference name: Inner")));
    s.cmdline("o").unwrap();
    let d = s.doc().unwrap();
    let r = d.refedit.clone().unwrap();
    assert_eq!(r.block, "Inner");
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.ghosts.len(), 1, "Outer's line stands faded");
    let EntityKind::Circle(circle) = &d.entity(r.items[0].copy).unwrap().kind else { panic!() };
    assert!(circle.center.xy().near(Vec2::new(110.0, 100.0), 1e-9));
    // Only the working set can change: Outer's faded line is locked.
    s.execute("move", &json!({ "handles": [r.items[0].copy.hex()], "delta": [0, 3] })).unwrap();
    s.execute("refclose", &json!({ "option": "save" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Circle(c) = &d.block("Inner").unwrap().entities.iter().next().unwrap().kind else { panic!() };
    assert!(c.center.xy().near(Vec2::new(0.0, 3.0), 1e-9), "{c:?}");
    assert_eq!(d.block("Outer").unwrap().entities.len(), 2, "the parent is untouched");
    // Bad paths are refused.
    assert!(s.execute("refedit", &json!({ "handle": o, "path": ["FFFFF"] })).is_err());
    assert!(s.execute("refedit", &json!({ "handle": o, "path": "x" })).is_err());
    assert!(s.execute("refedit", &json!({ "handle": o, "nested": "Nope" })).is_err());
    let _ = Vec2::ZERO;
}
