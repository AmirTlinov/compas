use crate::api::InitPlan;
use std::collections::BTreeSet;

/// WASM init-plugin sandbox limits + capability allowlist.
///
/// Contract:
/// - input: JSON bytes (caller-defined)
/// - output: JSON bytes parsed as [`InitPlan`] only
///
/// Capability model (v1): deny-by-default imports.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Experimental + behind feature; keep API stable even when unused.
pub(crate) struct WasmInitPluginPolicy {
    /// Maximum input JSON bytes copied into guest memory.
    pub max_input_bytes: usize,
    /// Maximum output JSON bytes read from guest memory.
    pub max_output_bytes: usize,
    /// Maximum guest linear memory size (bytes).
    pub max_memory_bytes: usize,
    /// Fuel budget (rough instruction cap). Requires wasmtime fuel metering.
    pub max_fuel: u64,
    /// Allowed imports as `"<module>::<name>"`. Default: none.
    pub allowed_imports: BTreeSet<String>,
}

impl Default for WasmInitPluginPolicy {
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * 1024,
            max_output_bytes: 256 * 1024,
            max_memory_bytes: 64 * 1024 * 1024,
            max_fuel: 50_000_000,
            allowed_imports: BTreeSet::new(),
        }
    }
}

#[cfg(not(feature = "wasm"))]
#[allow(dead_code)] // Exposed as a fail-closed runtime error when wasm feature is off.
pub(crate) fn run_wasm_init_plan(
    _wasm_bytes: &[u8],
    _input_json: &[u8],
    _policy: &WasmInitPluginPolicy,
) -> Result<InitPlan, String> {
    Err("wasm feature is disabled; rebuild with --features wasm".to_string())
}

#[cfg(feature = "wasm")]
#[allow(dead_code)] // May be wired by init/code-plugins later.
pub(crate) fn run_wasm_init_plan(
    wasm_bytes: &[u8],
    input_json: &[u8],
    policy: &WasmInitPluginPolicy,
) -> Result<InitPlan, String> {
    use wasmtime::{Config, Engine, Instance, Linker, Module, Store};

    if input_json.len() > policy.max_input_bytes {
        return Err(format!(
            "wasm init input too large: bytes={} > max_input_bytes={}",
            input_json.len(),
            policy.max_input_bytes
        ));
    }

    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    let engine = Engine::new(&cfg).map_err(|e| format!("wasm.engine_create_failed: {e}"))?;

    let module = Module::from_binary(&engine, wasm_bytes)
        .map_err(|e| format!("wasm.module_invalid: {e}"))?;

    // Capabilities: deny-by-default imports.
    let mut denied: Vec<String> = vec![];
    for import in module.imports() {
        let key = format!("{}::{}", import.module(), import.name());
        if !policy.allowed_imports.contains(&key) {
            denied.push(key);
        }
    }
    denied.sort();
    if !denied.is_empty() {
        return Err(format!(
            "wasm.imports_denied: module imports are not allowed (denied={}): {:?}",
            denied.len(),
            denied
        ));
    }

    // Memory limiter (best-effort, fail-closed when limits API changes).
    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(policy.max_memory_bytes)
        .build();

    let mut store = Store::new(&engine, limits);
    store
        .set_fuel(policy.max_fuel)
        .map_err(|e| format!("wasm.fuel_set_failed: {e}"))?;
    store.limiter(|s| s);

    let linker: Linker<wasmtime::StoreLimits> = Linker::new(&engine);
    let instance: Instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("wasm.instantiate_failed: {e}"))?;

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| "wasm.missing_export: memory".to_string())?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "alloc")
        .map_err(|e| format!("wasm.missing_export: alloc: {e}"))?;
    let compas_init = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "compas_init")
        .map_err(|e| format!("wasm.missing_export: compas_init: {e}"))?;

    let in_ptr = alloc
        .call(&mut store, input_json.len() as i32)
        .map_err(|e| format!("wasm.call_failed: alloc: {e}"))?;
    memory
        .write(&mut store, in_ptr as usize, input_json)
        .map_err(|e| format!("wasm.memory_write_failed: {e}"))?;

    let out = compas_init
        .call(&mut store, (in_ptr, input_json.len() as i32))
        .map_err(|e| format!("wasm.call_failed: compas_init: {e}"))?;
    let out_ptr = (out & 0xffff_ffff) as u32 as usize;
    let out_len = (out >> 32) as u32 as usize;

    if out_len > policy.max_output_bytes {
        return Err(format!(
            "wasm init output too large: bytes={} > max_output_bytes={}",
            out_len, policy.max_output_bytes
        ));
    }

    let mut out_bytes = vec![0u8; out_len];
    memory
        .read(&mut store, out_ptr, &mut out_bytes)
        .map_err(|e| format!("wasm.memory_read_failed: {e}"))?;

    let plan: InitPlan = serde_json::from_slice(&out_bytes)
        .map_err(|e| format!("wasm.output_invalid_init_plan_json: {e}"))?;
    Ok(plan)
}

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::*;

    fn wasm_bytes_ok() -> Vec<u8> {
        // Minimal wasm guest:
        // - exports memory, alloc, compas_init
        // - compas_init ignores input and returns `{"writes":[],"deletes":[]}`
        let wat = r#"
(module
  (memory (export "memory") 1)
  (global $hp (mut i32) (i32.const 1024))
  (func (export "alloc") (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $hp))
    (global.set $hp (i32.add (global.get $hp) (local.get $n)))
    (local.get $p))

  ;; Writes a constant JSON into memory at 2048 and returns (ptr,len) packed into i64.
  (data (i32.const 2048) "{\"writes\":[],\"deletes\":[]}")
  (func (export "compas_init") (param i32 i32) (result i64)
    (i64.or
      (i64.shl (i64.const 26) (i64.const 32)) ;; len=26
      (i64.const 2048)))                      ;; ptr=2048
)
"#;
        wat::parse_str(wat).expect("wat parse")
    }

    #[test]
    fn wasm_runner_returns_init_plan_only() {
        let plan = run_wasm_init_plan(
            &wasm_bytes_ok(),
            br#"{"x":1}"#,
            &WasmInitPluginPolicy::default(),
        )
        .expect("run ok");
        assert!(plan.writes.is_empty());
        assert!(plan.deletes.is_empty());
    }

    #[test]
    fn wasm_imports_are_denied_by_default() {
        let wat = r#"
(module
  (import "env" "forbidden" (func $f))
  (memory (export "memory") 1)
  (func (export "alloc") (param i32) (result i32) (i32.const 0))
  (func (export "compas_init") (param i32 i32) (result i64) (i64.const 0))
)
"#;
        let bytes = wat::parse_str(wat).expect("wat parse");
        let err = run_wasm_init_plan(&bytes, b"{}", &WasmInitPluginPolicy::default()).unwrap_err();
        assert!(err.contains("imports_denied"), "{err}");
        assert!(err.contains("env::forbidden"), "{err}");
    }
}
