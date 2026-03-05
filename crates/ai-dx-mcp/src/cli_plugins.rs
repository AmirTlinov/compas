#[path = "cli_plugins/cache.rs"]
mod cache;

pub(crate) async fn run_plugins_cli(parsed: &super::PluginsCli) -> Result<i32, String> {
    cache::run_plugins_cli(parsed).await
}
