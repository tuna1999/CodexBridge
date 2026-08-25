use serde::Serialize;

use crate::{config::Config, sandbox};

/// Identity-independent execution facts shared by MCP instructions, tool
/// descriptions, and startup diagnostics. Project paths and conversation
/// identifiers are intentionally excluded.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeEnvironment {
    pub os: &'static str,
    pub arch: &'static str,
    pub path_separator: char,
    pub executable_suffix: &'static str,
    pub shell: String,
    pub shell_kind: &'static str,
    pub shell_argv_prefix: Vec<String>,
    pub sandbox_backend: &'static str,
}

impl RuntimeEnvironment {
    pub fn detect(config: &Config) -> Self {
        let (shell, shell_kind, shell_argv_prefix) = sandbox::default_exec_shell(config);
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            path_separator: std::path::MAIN_SEPARATOR,
            executable_suffix: std::env::consts::EXE_SUFFIX,
            shell,
            shell_kind,
            shell_argv_prefix,
            sandbox_backend: sandbox::effective_default_sandbox_backend(config),
        }
    }

    pub fn render_agent_summary(&self) -> String {
        let shell_advice = match self.shell_kind {
            "powershell" => "Write PowerShell syntax, not POSIX shell syntax.",
            "cmd" => "Write cmd.exe syntax, not POSIX shell syntax.",
            _ => "Write POSIX shell syntax.",
        };
        format!(
            "Environment (identity-independent, secret-free): OS={}, architecture={}, path separator=`{}`, executable suffix=`{}`, exec shell=`{}` ({}), default exec backend={}. {} Structured project-tool paths remain relative to the active project and are disclosed only after chatgpt_turn_init; individual commands such as Podman may use a different effective backend when runtime capability probing requires it.",
            self.os,
            self.arch,
            self.path_separator,
            self.executable_suffix,
            self.shell,
            self.shell_kind,
            self.sandbox_backend,
            shell_advice,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigBuilder;
    use std::collections::BTreeMap;

    #[test]
    fn rendered_environment_is_secret_free_and_names_effective_shell() {
        let config = ConfigBuilder::from_map(BTreeMap::from([(
            "MCP_AUTH_TOKEN".to_owned(),
            "1234567890abcdef".to_owned(),
        )]))
        .build()
        .unwrap();
        let environment = RuntimeEnvironment::detect(&config);
        let rendered = environment.render_agent_summary();
        assert!(rendered.contains(&environment.shell));
        assert!(rendered.contains(environment.sandbox_backend));
        assert!(!rendered.contains(&config.auth_token));
        assert!(!rendered.contains(config.workspace_root.to_string_lossy().as_ref()));
    }
}
