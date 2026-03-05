use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir has parent")
        .parent()
        .expect("workspace root exists")
        .to_path_buf()
}

fn read_doc(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

#[test]
fn plugins_docs_use_canonical_sunset_terms() {
    let doc = read_doc("docs/PLUGINS.md");
    let old_flag = concat!("--allow-depre", "cated");
    let old_tier = concat!("tier=depre", "cated");
    assert!(
        doc.contains("--allow-sunset"),
        "docs/PLUGINS.md must mention --allow-sunset"
    );
    assert!(
        doc.contains("tier=sunset"),
        "docs/PLUGINS.md must mention tier=sunset"
    );
    assert!(
        !doc.contains(old_flag),
        "docs/PLUGINS.md must not mention old sunset flag alias"
    );
    assert!(
        !doc.contains(old_tier),
        "docs/PLUGINS.md must not mention old tier alias"
    );
    assert!(
        !doc.contains(".registry_state.json"),
        "docs/PLUGINS.md must not mention old .registry_state.json state path"
    );
}

#[test]
fn plugins_docs_admin_lane_examples_are_explicit() {
    let doc = read_doc("docs/PLUGINS.md");
    for line in doc.lines() {
        if line.contains("ai-dx-mcp plugins install")
            || line.contains("ai-dx-mcp plugins update")
            || line.contains("ai-dx-mcp plugins uninstall")
        {
            assert!(
                line.contains("--admin-lane"),
                "plugins mutating command example must include --admin-lane: {line}"
            );
        }
    }
}

#[test]
fn status_doc_uses_canonical_sunset_terms() {
    let doc = read_doc("docs/STATUS.md");
    let old_flag = concat!("--allow-depre", "cated");
    let old_tier = concat!("tier=depre", "cated");
    assert!(
        doc.contains("--allow-sunset"),
        "docs/STATUS.md must mention --allow-sunset"
    );
    assert!(
        doc.contains("tier=sunset"),
        "docs/STATUS.md must mention tier=sunset"
    );
    assert!(
        !doc.contains(old_flag),
        "docs/STATUS.md must not mention old sunset flag alias"
    );
    assert!(
        !doc.contains(old_tier),
        "docs/STATUS.md must not mention old tier alias"
    );
}
