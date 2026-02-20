use crate::api::{
    ApiError, GateJobState, GateKind, GateOp, GateOutput, JobInfo, ValidateMode, ValidateOutput,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DEFAULT_JOB_TTL_SECS: u64 = 24 * 60 * 60;
const DEFAULT_JOB_RING_SIZE: usize = 200;
const DEFAULT_STATUS_WAIT_CAP_MS: u64 = 15_000;
const LOCK_ATTEMPTS: usize = 120;
const LOCK_SLEEP_MS: u64 = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GateJobRecord {
    job_id: String,
    repo_root: String,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
    gate_budget_ms: Option<u64>,
    state: GateJobState,
    created_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: i64,
    owner_pid: u32,
    result: Option<GateOutput>,
    job_error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JobIndex {
    entries: Vec<String>,
}

fn job_ttl_secs() -> u64 {
    std::env::var("AI_DX_JOB_TTL_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_JOB_TTL_SECS)
}

fn job_ring_size() -> usize {
    std::env::var("AI_DX_JOB_RING_SIZE")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_JOB_RING_SIZE)
}

fn status_wait_cap_ms() -> u64 {
    std::env::var("AI_DX_GATE_STATUS_WAIT_MAX_MS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_STATUS_WAIT_CAP_MS)
}

fn state_dir(repo_root: &str) -> PathBuf {
    Path::new(repo_root).join(".agents/mcp/compas/state/jobs")
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

fn lock_path(dir: &Path) -> PathBuf {
    dir.join(".lock")
}

fn job_path(dir: &Path, job_id: &str) -> PathBuf {
    dir.join(format!("{job_id}.json"))
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn ts_rfc3339(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

fn job_info(rec: &GateJobRecord) -> JobInfo {
    JobInfo {
        job_id: rec.job_id.clone(),
        state: rec.state,
        created_at: ts_rfc3339(rec.created_at_ms),
        updated_at: ts_rfc3339(rec.updated_at_ms),
        expires_at: ts_rfc3339(rec.expires_at_ms),
    }
}

fn with_lock<T, F>(repo_root: &str, f: F) -> Result<T, String>
where
    F: FnOnce(&Path) -> Result<T, String>,
{
    let dir = state_dir(repo_root);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create gate job state dir {:?}: {e}", dir))?;

    let lock = lock_path(&dir);
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..LOCK_ATTEMPTS {
        match OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_lock_handle) => {
                let res = f(&dir);
                let _ = std::fs::remove_file(&lock);
                return res;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(LOCK_SLEEP_MS));
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    Err(format!(
        "failed to acquire gate job lock {:?}: {}",
        lock,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "timeout".to_string())
    ))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let json =
        serde_json::to_string_pretty(value).map_err(|e| format!("serialize {:?}: {e}", path))?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, json).map_err(|e| format!("write tmp {:?}: {e}", tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename {:?} -> {:?}: {e}", tmp, path)
    })
}

fn read_record(dir: &Path, job_id: &str) -> Result<GateJobRecord, String> {
    let p = job_path(dir, job_id);
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("read {:?}: {e}", p))?;
    serde_json::from_str::<GateJobRecord>(&raw).map_err(|e| format!("parse {:?}: {e}", p))
}

fn write_record(dir: &Path, rec: &GateJobRecord) -> Result<(), String> {
    write_json_atomic(&job_path(dir, &rec.job_id), rec)
}

fn read_index(dir: &Path) -> Result<JobIndex, String> {
    let path = index_path(dir);
    if !path.is_file() {
        return Ok(JobIndex::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read {:?}: {e}", path))?;
    serde_json::from_str::<JobIndex>(&raw).map_err(|e| format!("parse {:?}: {e}", path))
}

fn write_index(dir: &Path, idx: &JobIndex) -> Result<(), String> {
    write_json_atomic(&index_path(dir), idx)
}

fn prune_index_and_expired(dir: &Path, idx: &mut JobIndex, now: i64) {
    idx.entries.retain(|job_id| {
        let p = job_path(dir, job_id);
        if !p.is_file() {
            return false;
        }
        let Ok(raw) = std::fs::read_to_string(&p) else {
            let _ = std::fs::remove_file(&p);
            return false;
        };
        let Ok(rec) = serde_json::from_str::<GateJobRecord>(&raw) else {
            let _ = std::fs::remove_file(&p);
            return false;
        };
        if rec.expires_at_ms <= now {
            let _ = std::fs::remove_file(&p);
            return false;
        }
        true
    });
}

fn enforce_ring_size(dir: &Path, idx: &mut JobIndex, max_entries: usize) {
    if idx.entries.len() <= max_entries {
        return;
    }
    let remove_count = idx.entries.len() - max_entries;
    let removed: Vec<String> = idx.entries.drain(0..remove_count).collect();
    for job_id in removed {
        let _ = std::fs::remove_file(job_path(dir, &job_id));
    }
}

fn placeholder_validate(repo_root: &str) -> ValidateOutput {
    ValidateOutput {
        ok: true,
        error: None,
        schema_version: "3".to_string(),
        repo_root: repo_root.to_string(),
        mode: ValidateMode::Ratchet,
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
        verdict: None,
        quality_posture: None,
        agent_digest: None,
        summary_md: None,
        payload_meta: None,
    }
}

pub(crate) fn empty_validate_output(repo_root: &str) -> ValidateOutput {
    placeholder_validate(repo_root)
}

fn response_from_record(rec: &GateJobRecord) -> GateOutput {
    if let Some(mut out) = rec.result.clone() {
        out.job = Some(job_info(rec));
        out.job_state = Some(rec.state);
        out.job_error = rec.job_error.clone();
        return out;
    }

    let (ok, error) = match rec.state {
        GateJobState::Pending | GateJobState::Running => (true, None),
        GateJobState::Expired => (
            false,
            Some(ApiError {
                code: "gate.job_expired".to_string(),
                message: format!("gate job {} expired", rec.job_id),
            }),
        ),
        GateJobState::Failed => (
            false,
            rec.job_error.clone().or_else(|| {
                Some(ApiError {
                    code: "gate.job_failed".to_string(),
                    message: format!("gate job {} failed", rec.job_id),
                })
            }),
        ),
        GateJobState::Succeeded => (
            false,
            Some(ApiError {
                code: "gate.job_missing_result".to_string(),
                message: format!("gate job {} has no result payload", rec.job_id),
            }),
        ),
    };

    GateOutput {
        ok,
        error,
        repo_root: rec.repo_root.clone(),
        kind: rec.kind,
        validate: placeholder_validate(&rec.repo_root),
        receipts: vec![],
        witness_path: None,
        witness: None,
        verdict: None,
        agent_digest: None,
        summary_md: None,
        payload_meta: None,
        job: Some(job_info(rec)),
        job_state: Some(rec.state),
        job_error: rec.job_error.clone(),
    }
}

fn job_not_found(repo_root: &str, kind: GateKind, job_id: &str) -> GateOutput {
    GateOutput {
        ok: false,
        error: Some(ApiError {
            code: "gate.job_not_found".to_string(),
            message: format!("gate job not found: {job_id}"),
        }),
        repo_root: repo_root.to_string(),
        kind,
        validate: placeholder_validate(repo_root),
        receipts: vec![],
        witness_path: None,
        witness: None,
        verdict: None,
        agent_digest: None,
        summary_md: None,
        payload_meta: None,
        job: None,
        job_state: None,
        job_error: None,
    }
}

fn next_job_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let now = Utc::now().timestamp_millis();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("gate-{now}-{}-{n}", std::process::id())
}

fn write_new_job(
    repo_root: &str,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
    gate_budget_ms: Option<u64>,
) -> Result<GateJobRecord, String> {
    with_lock(repo_root, |dir| {
        let now = now_ms();
        let ttl_ms = (job_ttl_secs() as i64) * 1000;
        let job_id = next_job_id();
        let record = GateJobRecord {
            job_id: job_id.clone(),
            repo_root: repo_root.to_string(),
            kind,
            dry_run,
            write_witness,
            gate_budget_ms,
            state: GateJobState::Running,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: now + ttl_ms,
            owner_pid: std::process::id(),
            result: None,
            job_error: None,
        };
        write_record(dir, &record)?;

        let mut idx = read_index(dir).unwrap_or_default();
        prune_index_and_expired(dir, &mut idx, now);
        idx.entries.retain(|id| id != &job_id);
        idx.entries.push(job_id);
        enforce_ring_size(dir, &mut idx, job_ring_size());
        write_index(dir, &idx)?;
        Ok(record)
    })
}

fn mark_job_result(repo_root: &str, job_id: &str, out: GateOutput) -> Result<(), String> {
    with_lock(repo_root, |dir| {
        let mut rec = read_record(dir, job_id)?;
        rec.updated_at_ms = now_ms();
        rec.state = GateJobState::Succeeded;
        rec.result = Some(out);
        rec.job_error = None;
        write_record(dir, &rec)
    })
}

fn mark_job_failed(repo_root: &str, job_id: &str, err: ApiError) -> Result<(), String> {
    with_lock(repo_root, |dir| {
        let mut rec = read_record(dir, job_id)?;
        rec.updated_at_ms = now_ms();
        rec.state = GateJobState::Failed;
        rec.job_error = Some(err);
        rec.result = None;
        write_record(dir, &rec)
    })
}

fn load_job(repo_root: &str, job_id: &str) -> Result<Option<GateJobRecord>, String> {
    with_lock(repo_root, |dir| {
        let now = now_ms();
        let mut idx = read_index(dir).unwrap_or_default();
        prune_index_and_expired(dir, &mut idx, now);
        write_index(dir, &idx)?;

        let path = job_path(dir, job_id);
        if !path.is_file() {
            return Ok(None);
        }
        let mut rec = read_record(dir, job_id)?;
        if rec.expires_at_ms <= now {
            let _ = std::fs::remove_file(&path);
            idx.entries.retain(|id| id != job_id);
            let _ = write_index(dir, &idx);
            rec.state = GateJobState::Expired;
            rec.result = None;
            rec.job_error = Some(ApiError {
                code: "gate.job_expired".to_string(),
                message: format!("gate job {} expired", rec.job_id),
            });
            return Ok(Some(rec));
        }

        if matches!(rec.state, GateJobState::Pending | GateJobState::Running)
            && rec.owner_pid != std::process::id()
        {
            rec.updated_at_ms = now;
            rec.state = GateJobState::Failed;
            rec.job_error = Some(ApiError {
                code: "gate.runner_interrupted".to_string(),
                message: format!(
                    "gate job {} was created in another session/process (pid={})",
                    rec.job_id, rec.owner_pid
                ),
            });
            rec.result = None;
            write_record(dir, &rec)?;
        }

        Ok(Some(rec))
    })
}

pub(crate) async fn start(
    repo_root: &str,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
    gate_budget_ms: Option<u64>,
) -> GateOutput {
    let rec = match write_new_job(repo_root, kind, dry_run, write_witness, gate_budget_ms) {
        Ok(r) => r,
        Err(msg) => {
            return GateOutput {
                ok: false,
                error: Some(ApiError {
                    code: "gate.job_start_failed".to_string(),
                    message: msg,
                }),
                repo_root: repo_root.to_string(),
                kind,
                validate: placeholder_validate(repo_root),
                receipts: vec![],
                witness_path: None,
                witness: None,
                verdict: None,
                agent_digest: None,
                summary_md: None,
                payload_meta: None,
                job: None,
                job_state: None,
                job_error: None,
            };
        }
    };

    let job_id = rec.job_id.clone();
    let repo_root_owned = repo_root.to_string();
    tokio::spawn(async move {
        let out = crate::app::gate_with_budget(
            &repo_root_owned,
            kind,
            dry_run,
            write_witness,
            gate_budget_ms,
        )
        .await;
        if let Err(msg) = mark_job_result(&repo_root_owned, &job_id, out) {
            let _ = mark_job_failed(
                &repo_root_owned,
                &job_id,
                ApiError {
                    code: "gate.job_persist_failed".to_string(),
                    message: msg,
                },
            );
        }
    });

    response_from_record(&rec)
}

pub(crate) async fn status(
    repo_root: &str,
    kind: GateKind,
    job_id: &str,
    wait_ms: Option<u64>,
) -> GateOutput {
    let wait_cap = status_wait_cap_ms();
    let wait_budget = wait_ms.unwrap_or(0).min(wait_cap);
    let started = Instant::now();

    loop {
        let rec = match load_job(repo_root, job_id) {
            Ok(Some(r)) => r,
            Ok(None) => return job_not_found(repo_root, kind, job_id),
            Err(msg) => {
                return GateOutput {
                    ok: false,
                    error: Some(ApiError {
                        code: "gate.job_corrupted_state".to_string(),
                        message: msg,
                    }),
                    repo_root: repo_root.to_string(),
                    kind,
                    validate: placeholder_validate(repo_root),
                    receipts: vec![],
                    witness_path: None,
                    witness: None,
                    verdict: None,
                    agent_digest: None,
                    summary_md: None,
                    payload_meta: None,
                    job: None,
                    job_state: None,
                    job_error: None,
                };
            }
        };

        let terminal = matches!(
            rec.state,
            GateJobState::Succeeded | GateJobState::Failed | GateJobState::Expired
        );
        if terminal || started.elapsed().as_millis() as u64 >= wait_budget {
            return response_from_record(&rec);
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub(crate) fn validate_gate_status_args(op: GateOp, job_id: Option<&str>) -> Result<(), ApiError> {
    if matches!(op, GateOp::Status) {
        let Some(job_id) = job_id else {
            return Err(ApiError {
                code: "gate.job_invalid_op".to_string(),
                message: "compas.gate op=status requires non-empty job_id".to_string(),
            });
        };
        if job_id.trim().is_empty() {
            return Err(ApiError {
                code: "gate.job_invalid_op".to_string(),
                message: "compas.gate op=status requires non-empty job_id".to_string(),
            });
        }
    }
    Ok(())
}
