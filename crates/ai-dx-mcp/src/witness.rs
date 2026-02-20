use crate::api::{ApiError, GateKind, GateOutput, WitnessMeta};
use crate::hash::sha256_hex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const WITNESS_MAX_FILES: usize = 20;
const WITNESS_MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024;

fn gate_kind_slug(kind: GateKind) -> &'static str {
    match kind {
        GateKind::CiFast => "ci-fast",
        GateKind::Ci => "ci",
        GateKind::Flagship => "flagship",
    }
}

#[derive(Debug)]
struct FileMeta {
    path: PathBuf,
    modified: std::time::SystemTime,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChainEntry {
    pub gate_kind: String,
    pub timestamp: String,
    pub witness_sha256: String,
    pub prev_hash: String,
    pub entry_hash: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChain {
    pub entries: Vec<WitnessChainEntry>,
}

fn rotate_witness_dir_with_limits(
    dir: &Path,
    keep_path: &Path,
    max_files: usize,
    max_total_bytes: u64,
) -> Result<usize, std::io::Error> {
    let mut files = Vec::<FileMeta>::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if !(file_name.starts_with("gate_") && file_name.ends_with(".json")) {
            continue;
        }
        let md = entry.metadata()?;
        files.push(FileMeta {
            path,
            modified: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            size: md.len(),
        });
    }

    files.sort_by_key(|f| f.modified);

    let mut count = files.len();
    let mut total: u64 = files.iter().map(|f| f.size).sum();
    let mut removed = 0usize;

    for f in files {
        if count <= max_files && total <= max_total_bytes {
            break;
        }
        if f.path == keep_path {
            continue;
        }

        std::fs::remove_file(&f.path)?;
        count = count.saturating_sub(1);
        total = total.saturating_sub(f.size);
        removed += 1;
    }

    Ok(removed)
}

fn rotate_witness_dir(dir: &Path, keep_path: &Path) -> Result<usize, std::io::Error> {
    rotate_witness_dir_with_limits(dir, keep_path, WITNESS_MAX_FILES, WITNESS_MAX_TOTAL_BYTES)
}

fn compute_entry_hash(
    prev_hash: &str,
    witness_sha256: &str,
    timestamp: &str,
    gate_kind: &str,
) -> String {
    let input = format!("{prev_hash}:{witness_sha256}:{timestamp}:{gate_kind}");
    sha256_hex(input.as_bytes())
}

pub(crate) fn load_witness_chain(path: &Path) -> Result<WitnessChain, std::io::Error> {
    if !path.is_file() {
        return Ok(WitnessChain { entries: vec![] });
    }
    let raw = std::fs::read_to_string(path)?;
    let chain: WitnessChain = serde_json::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(chain)
}

pub(crate) fn verify_chain_integrity(chain: &WitnessChain) -> bool {
    let mut expected_prev = "genesis".to_string();
    for entry in &chain.entries {
        if entry.prev_hash != expected_prev {
            return false;
        }
        let computed = compute_entry_hash(
            &entry.prev_hash,
            &entry.witness_sha256,
            &entry.timestamp,
            &entry.gate_kind,
        );
        if entry.entry_hash != computed {
            return false;
        }
        expected_prev = entry.entry_hash.clone();
    }
    true
}

pub(crate) fn append_chain_entry(
    chain_path: &Path,
    gate_kind: &str,
    witness_sha256: &str,
    ok: bool,
) -> Result<WitnessChainEntry, std::io::Error> {
    let mut chain = load_witness_chain(chain_path)?;
    if !verify_chain_integrity(&chain) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "witness chain integrity check failed",
        ));
    }

    let prev_hash = chain
        .entries
        .last()
        .map(|e| e.entry_hash.clone())
        .unwrap_or_else(|| "genesis".to_string());
    let timestamp = chrono::Utc::now().to_rfc3339();
    let entry_hash = compute_entry_hash(&prev_hash, witness_sha256, &timestamp, gate_kind);
    let entry = WitnessChainEntry {
        gate_kind: gate_kind.to_string(),
        timestamp,
        witness_sha256: witness_sha256.to_string(),
        prev_hash,
        entry_hash,
        ok,
    };

    chain.entries.push(entry.clone());
    if let Some(parent) = chain_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = chain_path.with_extension(format!("tmp.{}", std::process::id()));
    let json =
        serde_json::to_string_pretty(&chain).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, chain_path)?;
    Ok(entry)
}

pub(crate) fn maybe_write_gate_witness(
    repo_root: &Path,
    kind: GateKind,
    write_witness: bool,
    mut out: GateOutput,
) -> GateOutput {
    if !write_witness {
        return out;
    }

    let witness_rel = format!(
        ".agents/mcp/compas/witness/gate_{}.json",
        gate_kind_slug(kind)
    );
    let witness_path = repo_root.join(&witness_rel);

    if let Some(parent) = witness_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        out.ok = false;
        out.error = Some(ApiError {
            code: "witness.write_failed".to_string(),
            message: format!("failed to create witness dir {:?}: {e}", parent),
        });
        out.witness_path = None;
        out.witness = None;
        return out;
    }

    out.witness_path = Some(witness_rel.clone());
    let json = match serde_json::to_string_pretty(&out) {
        Ok(s) => s,
        Err(e) => {
            out.ok = false;
            out.error = Some(ApiError {
                code: "witness.write_failed".to_string(),
                message: format!("failed to serialize witness {witness_rel:?}: {e}"),
            });
            out.witness_path = None;
            out.witness = None;
            return out;
        }
    };

    let bytes = json.as_bytes();
    if let Err(e) = std::fs::write(&witness_path, bytes) {
        out.ok = false;
        out.error = Some(ApiError {
            code: "witness.write_failed".to_string(),
            message: format!("failed to write witness {witness_rel:?}: {e}"),
        });
        out.witness_path = None;
        out.witness = None;
        return out;
    }

    // Append to hash-chain (fail-closed).
    let chain_path = repo_root.join(".agents/mcp/compas/witness/chain.json");
    if let Err(e) = append_chain_entry(
        &chain_path,
        gate_kind_slug(kind),
        &sha256_hex(bytes),
        out.ok,
    ) {
        out.ok = false;
        out.error = Some(ApiError {
            code: "witness.chain_append_failed".to_string(),
            message: format!("failed to append witness chain: {e}"),
        });
        out.witness = None;
        return out;
    }

    let rotated_files = match witness_path.parent() {
        Some(parent) => match rotate_witness_dir(parent, &witness_path) {
            Ok(v) => v,
            Err(e) => {
                out.ok = false;
                out.error = Some(ApiError {
                    code: "witness.rotation_failed".to_string(),
                    message: format!("failed to rotate witness files in {:?}: {e}", parent),
                });
                out.witness = None;
                return out;
            }
        },
        None => 0,
    };

    out.witness = Some(WitnessMeta {
        path: witness_rel,
        size_bytes: bytes.len(),
        sha256: sha256_hex(bytes),
        rotated_files,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Decision, DecisionStatus, GateKind, ValidateMode, ValidateOutput, Verdict};

    #[test]
    fn rotation_keeps_latest_files() {
        let dir = tempfile::tempdir().unwrap();
        let wdir = dir.path().join("w");
        std::fs::create_dir_all(&wdir).unwrap();

        for i in 0..5 {
            let p = wdir.join(format!("gate_{i}.json"));
            std::fs::write(&p, format!("{i}")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let keep = wdir.join("gate_4.json");
        let removed = rotate_witness_dir_with_limits(&wdir, &keep, 2, 1024).unwrap();
        assert!(removed >= 3);
        assert!(keep.exists());
    }

    #[test]
    fn witness_chain_append_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let chain_path = dir.path().join("chain.json");

        let entry1 = append_chain_entry(&chain_path, "ci-fast", "abc123def456", true).unwrap();
        assert_eq!(entry1.prev_hash, "genesis");
        assert!(!entry1.entry_hash.is_empty());

        let entry2 = append_chain_entry(&chain_path, "ci-fast", "def456abc789", true).unwrap();
        assert_eq!(entry2.prev_hash, entry1.entry_hash);

        let chain = load_witness_chain(&chain_path).unwrap();
        assert_eq!(chain.entries.len(), 2);
        assert!(verify_chain_integrity(&chain));
    }

    #[test]
    fn witness_chain_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let chain_path = dir.path().join("chain.json");

        append_chain_entry(&chain_path, "ci-fast", "aaa", true).unwrap();
        append_chain_entry(&chain_path, "ci-fast", "bbb", true).unwrap();

        let mut chain = load_witness_chain(&chain_path).unwrap();
        chain.entries[0].entry_hash = "tampered".to_string();
        let json = serde_json::to_string_pretty(&chain).unwrap();
        std::fs::write(&chain_path, json).unwrap();

        let chain = load_witness_chain(&chain_path).unwrap();
        assert!(!verify_chain_integrity(&chain));
    }

    #[test]
    fn witness_meta_written() {
        let dir = tempfile::tempdir().unwrap();
        let out = GateOutput {
            ok: true,
            error: None,
            repo_root: ".".to_string(),
            kind: GateKind::CiFast,
            validate: ValidateOutput {
                ok: true,
                error: None,
                schema_version: "3".to_string(),
                repo_root: ".".to_string(),
                mode: ValidateMode::Warn,
                violations: vec![],
                findings_v2: vec![],
                suppressed: vec![],
                loc: None,
                boundary: None,
                public_surface: None,
                effective_config: None,
                risk_summary: None,
                coverage: None,
                trust_score: None,
                verdict: Some(Verdict {
                    decision: Decision {
                        status: DecisionStatus::Pass,
                        reasons: vec![],
                        blocking_count: 0,
                        observation_count: 0,
                    },
                    quality_posture: None,
                    suppressed_count: 0,
                    suppressed_codes: vec![],
                }),
                quality_posture: None,
                agent_digest: None,
                summary_md: None,
                payload_meta: None,
            },
            receipts: vec![],
            witness_path: None,
            witness: None,
            verdict: Some(Verdict {
                decision: Decision {
                    status: DecisionStatus::Pass,
                    reasons: vec![],
                    blocking_count: 0,
                    observation_count: 0,
                },
                quality_posture: None,
                suppressed_count: 0,
                suppressed_codes: vec![],
            }),
            agent_digest: None,
            summary_md: None,
            payload_meta: None,
            job: None,
            job_state: None,
            job_error: None,
        };

        let out = maybe_write_gate_witness(dir.path(), GateKind::CiFast, true, out);
        assert!(out.ok);
        assert!(out.witness_path.is_some());
        let meta = out.witness.expect("witness meta");
        assert!(meta.size_bytes > 0);
        assert_eq!(meta.sha256.len(), 64);
        assert!(
            dir.path()
                .join(".agents/mcp/compas/witness/chain.json")
                .is_file()
        );
    }
}
