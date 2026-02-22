use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

fn sha256_file(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read file");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn run_compas(args: &[String]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let cache = tempfile::tempdir().expect("temp cache");
    std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache.path())
        .args(args)
        .output()
        .expect("run compas")
}

fn build_registry_archive(
    root: &Path,
    add_traversal_entry: bool,
    add_symlink_entry: bool,
) -> PathBuf {
    // The `tar` crate builder refuses to create entries with `..` in the path.
    // For traversal fixtures we intentionally hand-craft the tar stream.
    if add_traversal_entry {
        return build_registry_archive_with_traversal_entry(root);
    }

    // We build a minimal archive that still looks like a registry:
    // - exactly one top-level dir: compas_plugins-fixture/
    // - one plugin payload at plugins/spec-adr-gate/
    let payload_root = root.join("payload_root");
    let plugin_dir = payload_root.join("plugins/spec-adr-gate");
    write_file(&plugin_dir.join("README.md"), "spec-adr fixture\n");
    write_file(
        &plugin_dir.join("plugin.toml"),
        "[plugin]\nid='spec-adr-gate'\n",
    );

    let archive_name = "compas_plugins-fixture.tar.gz";
    let archive_path = root.join(archive_name);
    let tar_gz = std::fs::File::create(&archive_path).expect("create archive");
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Add malicious entries first (fail-fast).
    if add_symlink_entry {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        header.set_link_name("target").expect("set link name");
        tar.append_data(
            &mut header,
            "compas_plugins-fixture/plugins/spec-adr-gate/symlink",
            std::io::empty(),
        )
        .expect("append symlink entry");
    }

    if add_traversal_entry {
        let data = b"evil\n";
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "compas_plugins-fixture/../evil.txt", &data[..])
            .expect("append traversal entry");
    }

    // Add the normal payload.
    tar.append_dir_all("compas_plugins-fixture", &payload_root)
        .expect("append payload");
    let enc = tar.into_inner().expect("finalize tar");
    let _ = enc.finish().expect("finalize gzip");
    archive_path
}

fn build_registry_archive_with_traversal_entry(root: &Path) -> PathBuf {
    fn write_octal(buf: &mut [u8], value: u64) {
        // Write as NUL-terminated octal, right-aligned, padded with '0'.
        for b in buf.iter_mut() {
            *b = b'0';
        }
        let s = format!("{:o}", value);
        let end = buf.len().saturating_sub(1);
        let start = end.saturating_sub(s.len());
        buf[start..start + s.len()].copy_from_slice(s.as_bytes());
        buf[buf.len() - 1] = 0;
    }

    fn header(name: &str, size: u64, typeflag: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        let name_bytes = name.as_bytes();
        assert!(name_bytes.len() <= 100, "tar name too long");
        h[0..name_bytes.len()].copy_from_slice(name_bytes);

        // mode / uid / gid / size / mtime
        write_octal(&mut h[100..108], 0o644);
        write_octal(&mut h[108..116], 0);
        write_octal(&mut h[116..124], 0);
        write_octal(&mut h[124..136], size);
        write_octal(&mut h[136..148], 0);

        // checksum field: spaces for computation
        for b in &mut h[148..156] {
            *b = b' ';
        }

        h[156] = typeflag;

        // ustar magic + version
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");

        let checksum: u32 = h.iter().map(|b| *b as u32).sum();
        let chk = format!("{:06o}\0 ", checksum);
        h[148..156].copy_from_slice(chk.as_bytes());
        h
    }

    fn append_block(out: &mut Vec<u8>, block: &[u8]) {
        assert_eq!(block.len(), 512);
        out.extend_from_slice(block);
    }

    fn append_dir(out: &mut Vec<u8>, name: &str) {
        let mut dir_name = name.to_string();
        if !dir_name.ends_with('/') {
            dir_name.push('/');
        }
        append_block(out, &header(&dir_name, 0, b'5'));
    }

    fn append_file(out: &mut Vec<u8>, name: &str, data: &[u8]) {
        append_block(out, &header(name, data.len() as u64, b'0'));
        out.extend_from_slice(data);
        let pad = (512 - (data.len() % 512)) % 512;
        if pad != 0 {
            out.extend(std::iter::repeat(0u8).take(pad));
        }
    }

    let mut tar_bytes: Vec<u8> = vec![];

    // Single top-level directory.
    append_dir(&mut tar_bytes, "compas_plugins-fixture");
    append_dir(&mut tar_bytes, "compas_plugins-fixture/plugins");
    append_dir(
        &mut tar_bytes,
        "compas_plugins-fixture/plugins/spec-adr-gate",
    );

    append_file(
        &mut tar_bytes,
        "compas_plugins-fixture/plugins/spec-adr-gate/README.md",
        b"spec-adr fixture\n",
    );
    append_file(
        &mut tar_bytes,
        "compas_plugins-fixture/plugins/spec-adr-gate/plugin.toml",
        b"[plugin]\nid='spec-adr-gate'\n",
    );

    // Malicious traversal entry: should be rejected by compas extractor.
    append_file(
        &mut tar_bytes,
        "compas_plugins-fixture/../evil.txt",
        b"evil\n",
    );

    // End-of-archive markers.
    tar_bytes.extend(std::iter::repeat(0u8).take(1024));

    let archive_name = "compas_plugins-fixture.tar.gz";
    let archive_path = root.join(archive_name);
    let tar_gz = std::fs::File::create(&archive_path).expect("create archive");
    let mut enc = GzEncoder::new(tar_gz, Compression::default());
    enc.write_all(&tar_bytes).expect("write gz");
    enc.finish().expect("finalize gz");
    archive_path
}

fn write_manifest(root: &Path, archive_path: &Path, override_sha256: Option<&str>) -> PathBuf {
    let archive_name = archive_path
        .file_name()
        .expect("archive name")
        .to_string_lossy()
        .to_string();
    let sha = override_sha256
        .map(str::to_string)
        .unwrap_or_else(|| sha256_file(archive_path));

    let manifest_path = root.join("registry.manifest.v1.json");
    let manifest = serde_json::json!({
        "schema": "compas.registry.manifest.v1",
        "registry_version": "fixture-1",
        "archive": { "name": archive_name, "sha256": sha },
        "plugins": [
            {
                "id": "spec-adr-gate",
                "aliases": ["spec-gate"],
                "path": "plugins/spec-adr-gate",
                "description": "Fixture plugin for archive security tests",
                "package": {
                    "version": "0.1.0",
                    "type": "script",
                    "maturity": "stable",
                    "runtime": "python3",
                    "portable": true,
                    "languages": ["agnostic"],
                    "entrypoint": "README.md",
                    "license": "MIT"
                }
            }
        ],
        "packs": [
            { "id": "core", "description": "Fixture pack", "plugins": ["spec-adr-gate"] }
        ]
    });
    write_file(
        &manifest_path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).expect("serialize manifest")
        ),
    );
    manifest_path
}

fn run_manifest_install(repo_root: &Path, manifest_path: &Path) -> std::process::Output {
    let args = vec![
        "plugins".to_string(),
        "install".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--".to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
        "--force".to_string(),
    ];
    run_compas(&args)
}

#[test]
fn manifest_install_rejects_archive_sha_mismatch() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path(), false, false);
    let manifest_path = write_manifest(
        workspace.path(),
        &archive_path,
        Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
    );

    let out = run_manifest_install(&repo_root, &manifest_path);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("archive sha256 mismatch"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn manifest_install_rejects_symlink_entries_in_archive() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path(), false, true);
    let manifest_path = write_manifest(workspace.path(), &archive_path, None);

    let out = run_manifest_install(&repo_root, &manifest_path);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unsupported tar entry type"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn manifest_install_rejects_path_traversal_entries_in_archive() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path(), true, false);
    let manifest_path = write_manifest(workspace.path(), &archive_path, None);

    let out = run_manifest_install(&repo_root, &manifest_path);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unsafe tar path component"),
        "unexpected stderr: {stderr}"
    );
}
