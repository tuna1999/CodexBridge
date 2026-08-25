use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEcosystem {
    Agents,
    Codex,
    Claude,
}

#[derive(Debug, Clone)]
pub struct AgentHome {
    pub ecosystem: AgentEcosystem,
    pub root: PathBuf,
}

impl AgentHome {
    pub fn discover() -> Option<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        [
            (".agents", AgentEcosystem::Agents),
            (".codex", AgentEcosystem::Codex),
            (".claude", AgentEcosystem::Claude),
        ]
        .into_iter()
        .map(|(name, ecosystem)| Self {
            ecosystem,
            root: home.join(name),
        })
        .find(|candidate| candidate.root.exists())
    }

    pub fn source_name(&self) -> &'static str {
        match self.ecosystem {
            AgentEcosystem::Agents => "agents",
            AgentEcosystem::Codex => "codex",
            AgentEcosystem::Claude => "claude",
        }
    }
}
