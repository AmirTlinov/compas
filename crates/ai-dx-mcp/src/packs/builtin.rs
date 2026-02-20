#![allow(dead_code)] // Wired by init (TASK-010); keep implementation in-place until used.

use crate::packs::schema::PackManifestV1;
use std::collections::BTreeMap;

const PACK_RUST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/rust/pack.toml"
));
const PACK_NODE_NPM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/node-npm/pack.toml"
));
const PACK_NODE_YARN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/node-yarn/pack.toml"
));
const PACK_NODE_PNPM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/node-pnpm/pack.toml"
));
const PACK_NODE_BUN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/node-bun/pack.toml"
));
const PACK_PYTHON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/python/pack.toml"
));
const PACK_PYTHON_PYTEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/python-pytest/pack.toml"
));
const PACK_GO: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/go/pack.toml"
));
const PACK_CMAKE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/cmake/pack.toml"
));
const PACK_DOTNET: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packs/builtin/dotnet/pack.toml"
));

pub(crate) fn load_builtin_pack_manifests() -> Result<BTreeMap<String, PackManifestV1>, String> {
    let mut out: BTreeMap<String, PackManifestV1> = BTreeMap::new();

    let sources = [
        PACK_RUST,
        PACK_NODE_NPM,
        PACK_NODE_YARN,
        PACK_NODE_PNPM,
        PACK_NODE_BUN,
        PACK_PYTHON,
        PACK_PYTHON_PYTEST,
        PACK_GO,
        PACK_CMAKE,
        PACK_DOTNET,
    ];
    for src in sources {
        let manifest: PackManifestV1 =
            toml::from_str(src).map_err(|e| format!("failed to parse builtin pack.toml: {e}"))?;
        let id = manifest.pack.id.clone();
        if out.insert(id.clone(), manifest).is_some() {
            return Err(format!("duplicate builtin pack id: {id}"));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn builtin_packs_parse_smoke() {
        let packs = load_builtin_pack_manifests().expect("load builtin packs");
        assert!(packs.contains_key("rust"));
        assert!(packs.contains_key("node-npm"));
        assert!(packs.contains_key("node-bun"));
        assert!(packs.contains_key("python"));
        assert!(packs.contains_key("python-pytest"));
        assert!(packs.contains_key("go"));
        assert!(packs.contains_key("cmake"));
        assert!(packs.contains_key("dotnet"));
    }

    #[test]
    fn builtin_packs_pass_packs_validator_smoke() {
        let packs = load_builtin_pack_manifests().expect("load builtin packs");

        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        let packs_dir = repo_root.join(".agents/mcp/compas/packs");
        fs::create_dir_all(&packs_dir).unwrap();
        fs::write(
            repo_root.join(".agents/mcp/compas/packs.lock"),
            "version = 1\npacks = []\n",
        )
        .unwrap();

        for (id, m) in &packs {
            let dst = packs_dir.join(id).join("pack.toml");
            fs::create_dir_all(dst.parent().unwrap()).unwrap();
            let raw = toml::to_string(m).expect("serialize manifest back to toml");
            fs::write(dst, raw).unwrap();
        }

        let violations = super::super::validate_packs(repo_root);
        assert!(violations.is_empty(), "violations: {:#?}", violations);
    }
}
