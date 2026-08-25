use rmcp::handler::server::router::tool::ToolRouter;

/// Central composition point for the fixed native MCP surface.
///
/// The public names below are the contract. Module routers may contain helper or
/// compatibility routes, but only routes explicitly named here are moved into
/// the returned router. This keeps the model-visible surface readable in one
/// place instead of composing every historical router and filtering afterward.
pub struct NativeToolRegistry;

pub const PUBLIC_TOOL_NAMES: &[&str] = &[
    "chatgpt_turn_init",
    "apply_patch",
    "read_file",
    "list_directory",
    "tree",
    "glob",
    "grep",
    "view_image",
    "exec_command",
    "write_stdin",
    "skills_list",
    "skills_read",
    "remember",
    "recall",
    "update_plan",
];

impl NativeToolRegistry {
    pub fn build<T: Send + Sync + 'static>(
        routers: impl IntoIterator<Item = ToolRouter<T>>,
    ) -> ToolRouter<T> {
        let mut available = ToolRouter::new();
        for router in routers {
            for route in router {
                let name = route.attr.name.clone();
                assert!(
                    !available.map.contains_key(&name),
                    "duplicate native tool route `{name}`"
                );
                available.add_route(route);
            }
        }

        let mut public = ToolRouter::new();
        for &name in PUBLIC_TOOL_NAMES {
            let route = available
                .map
                .remove(name)
                .unwrap_or_else(|| panic!("public native tool `{name}` is not registered"));
            public.add_route(route);
        }
        assert!(
            available.map.is_empty(),
            "native router contains unlisted routes: {:?}",
            available.map.keys().collect::<Vec<_>>()
        );
        public
    }
}
