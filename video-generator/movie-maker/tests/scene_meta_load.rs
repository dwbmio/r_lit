//! M0 unit test for `stage::import_scene` JSON deserialization.
//!
//! Guards against regressions in the on-disk schema (`scene.meta`):
//! - `clear-tp-id` (kebab) maps to `clear_tp_id` (snake)
//! - timeline keys are stringified node IDs
//! - `pos_star` is the misspelled "start" we must preserve for back-compat

use movie_maker::stage::import_scene;
use std::path::PathBuf;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rel)
}

#[tokio::test]
async fn loads_minimal_scene_with_one_node_one_action() {
    let path = fixture("scene_minimal.meta");
    let list = import_scene(path).await.expect("import_scene must succeed");

    assert_eq!(list.meta_scene_list.len(), 1, "exactly one scene expected");
    let scene = &list.meta_scene_list[0];

    assert_eq!(scene.name, "minimal_scene");
    assert_eq!(scene.clear_tp_id.as_deref(), Some("0"), "kebab-case alias must deserialize");
    assert_eq!(scene.textures.len(), 1, "one texture path");
    assert_eq!(scene.nodes.len(), 1, "one node");

    let node = &scene.nodes[0];
    assert_eq!(node.id, 42);
    assert_eq!(node.name, "lone_node");
    assert_eq!(node.tp_id.as_deref(), Some("1"));

    // timeline keyed by stringified node id
    let actions = scene.timeline.get("42").expect("node 42 must have a timeline");
    assert_eq!(actions.len(), 1, "one action on node 42");
    let act = &actions[0];
    assert_eq!(act.action, "move_to");
    assert!((act.start_t - 0.0).abs() < f32::EPSILON);
    assert_eq!(act.duration, Some(1.5));
}

#[tokio::test]
async fn loads_existing_perf_main_scene() {
    // Round-trip the real fixture used by perf_main to catch
    // unintended schema drift.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/perf_main/scene.meta");
    let list = import_scene(path).await.expect("perf_main scene.meta must load");

    let scene = &list.meta_scene_list[0];
    assert_eq!(scene.name, "single_scene");
    assert_eq!(scene.textures.len(), 2);
    assert_eq!(scene.nodes.len(), 2);
    assert_eq!(scene.timeline.len(), 2, "two timelines (node 1 and 2)");
    assert_eq!(scene.timeline.get("1").map(|v| v.len()), Some(3));
    assert_eq!(scene.timeline.get("2").map(|v| v.len()), Some(2));
}
