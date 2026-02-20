use super::RepoConfigError;
use regex::Regex;
use std::collections::BTreeMap;

pub(super) fn push_check_with_unique_id<T, F>(
    out: &mut Vec<T>,
    check: T,
    kind: &str,
    plugin_id: &str,
    id_re: &Regex,
    seen: &mut BTreeMap<String, String>,
    id_of: F,
) -> Result<(), RepoConfigError>
where
    F: Fn(&T) -> &str,
{
    let check_id = id_of(&check).to_string();
    if !id_re.is_match(&check_id) {
        return Err(RepoConfigError::InvalidCheckId {
            plugin_id: plugin_id.to_string(),
            kind: kind.to_string(),
            check_id,
        });
    }
    if let Some(prev) = seen.get(&check_id) {
        return Err(RepoConfigError::DuplicateCheckId {
            kind: kind.to_string(),
            check_id,
            plugin_id: plugin_id.to_string(),
            previous_plugin_id: prev.clone(),
        });
    }
    seen.insert(check_id, plugin_id.to_string());
    out.push(check);
    Ok(())
}
