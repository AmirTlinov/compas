use super::*;
use crate::api::{InitPlan, InitWriteFile};
use tempfile::tempdir;

#[test]
fn apply_plan_writes_files_under_allowlist() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    let plan = InitPlan {
        writes: vec![InitWriteFile {
            path: "tools/custom/x/tool.toml".to_string(),
            content_utf8: "hello".to_string(),
        }],
        deletes: vec![],
    };

    apply_plan(repo, &plan).expect("apply ok");
    assert_eq!(
        fs::read_to_string(repo.join("tools/custom/x/tool.toml")).unwrap(),
        "hello"
    );
}

#[test]
fn apply_plan_allows_ai_first_scaffold_files() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    let plan = InitPlan {
        writes: vec![
            InitWriteFile {
                path: "AGENTS.md".to_string(),
                content_utf8: "router".to_string(),
            },
            InitWriteFile {
                path: "docs/index.md".to_string(),
                content_utf8: "docs".to_string(),
            },
        ],
        deletes: vec![],
    };

    apply_plan(repo, &plan).expect("apply ok");
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "router"
    );
    assert_eq!(
        fs::read_to_string(repo.join("docs/index.md")).unwrap(),
        "docs"
    );
}

#[test]
fn apply_plan_rejects_conflicting_ai_first_scaffold_file() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    fs::write(repo.join("AGENTS.md"), "existing\n").unwrap();

    let plan = InitPlan {
        writes: vec![InitWriteFile {
            path: "AGENTS.md".to_string(),
            content_utf8: "router".to_string(),
        }],
        deletes: vec![],
    };

    let err = apply_plan(repo, &plan).unwrap_err();
    assert_eq!(err.code, "init.write_conflict");
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "existing\n"
    );
}

#[test]
fn apply_plan_rejects_paths_outside_allowlist() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    let plan = InitPlan {
        writes: vec![InitWriteFile {
            path: "README.md".to_string(),
            content_utf8: "nope".to_string(),
        }],
        deletes: vec![],
    };

    let err = apply_plan(repo, &plan).unwrap_err();
    assert_eq!(err.code, "init.plan_path_forbidden");
}

#[cfg(unix)]
#[test]
fn apply_plan_rejects_symlink_path_component() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let repo = dir.path();
    let outside = tempdir().unwrap();

    symlink(outside.path(), repo.join(".agents")).expect("create symlink");

    let plan = InitPlan {
        writes: vec![InitWriteFile {
            path: ".agents/mcp/compas/plugins/default/plugin.toml".to_string(),
            content_utf8: "x".to_string(),
        }],
        deletes: vec![],
    };

    let err = apply_plan(repo, &plan).unwrap_err();
    assert_eq!(err.code, "init.plan_path_symlink", "{err:?}");
}
