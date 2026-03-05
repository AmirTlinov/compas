use rmcp::{
    ErrorData,
    handler::server::tool::{ToolCallContext, ToolRouter},
    model::{CallToolResult, Tool},
};

/// Wrapper around rmcp `ToolRouter` that makes `list_all()` deterministic.
///
/// Tool ordering is a protocol-level contract for prompt caching: the MCP tool list must not
/// fluctuate between runs.
pub(crate) struct OrderedToolRouter<'a, S> {
    inner: &'a ToolRouter<S>,
}

impl<'a, S> OrderedToolRouter<'a, S>
where
    S: Send + Sync + 'static,
{
    pub(crate) fn new(inner: &'a ToolRouter<S>) -> Self {
        Self { inner }
    }

    pub(crate) async fn call<'b>(
        self,
        context: ToolCallContext<'b, S>,
    ) -> Result<CallToolResult, ErrorData> {
        self.inner.call(context).await
    }

    pub(crate) fn list_all(self) -> Vec<Tool> {
        let mut tools = self.inner.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    pub(crate) fn get(self, name: &str) -> Option<&'a Tool> {
        self.inner.get(name)
    }
}
