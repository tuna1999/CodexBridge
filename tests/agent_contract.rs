use std::collections::BTreeMap;

use codex_bridge::{
    config::ConfigBuilder,
    project::ProjectResolver,
    request_context::{RequestIdentity, TransportMode},
    runtime_environment::RuntimeEnvironment,
    storage::Storage,
};

fn identity(subject: &str, conversation: &str) -> RequestIdentity {
    RequestIdentity {
        openai_subject: subject.to_owned(),
        openai_conversation_id: conversation.to_owned(),
        mcp_session_id: None,
        transport_mode: TransportMode::Stateless,
    }
}

#[test]
fn runtime_environment_is_identity_independent_and_secret_free() {
    let directory = tempfile::tempdir().unwrap();
    let token = "1234567890abcdef";
    let config = ConfigBuilder::from_map(BTreeMap::from([
        ("MCP_AUTH_TOKEN".to_owned(), token.to_owned()),
        (
            "WORKSPACE_ROOT".to_owned(),
            directory.path().display().to_string(),
        ),
    ]))
    .build()
    .unwrap();

    let environment = RuntimeEnvironment::detect(&config);
    let rendered = environment.render_agent_summary();
    assert!(rendered.contains(&environment.shell));
    assert!(rendered.contains(environment.sandbox_backend));
    assert!(!rendered.contains(token));
    assert!(!rendered.contains(directory.path().to_string_lossy().as_ref()));
}

#[test]
fn conversation_init_is_new_then_existing_after_storage_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state.sqlite3");
    let workspace = directory.path().join("workspace");
    let request = identity("user", "conversation");

    let effective = {
        let storage = Storage::open(&database).unwrap();
        let resolver = ProjectResolver::new(workspace.clone(), storage).unwrap();
        let prepared = resolver
            .prepare_initialize(&request, Some("shared-name"))
            .unwrap();
        assert!(!prepared.reused_existing_binding);
        assert!(!prepared.joined);
        let effective = prepared.project.effective_project_key.clone();
        resolver.commit_initialize(&prepared).unwrap();
        effective
    };

    let storage = Storage::open(&database).unwrap();
    let resolver = ProjectResolver::new(workspace, storage).unwrap();
    let prepared = resolver
        .prepare_initialize(&request, Some("shared-name"))
        .unwrap();
    assert!(prepared.reused_existing_binding);
    assert!(prepared.joined);
    assert_eq!(prepared.project.effective_project_key, effective);
}

#[test]
fn fresh_conversation_can_join_existing_alias_without_becoming_same_identity() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(&directory.path().join("state.sqlite3")).unwrap();
    let resolver = ProjectResolver::new(directory.path().join("workspace"), storage).unwrap();
    let owner = identity("user-a", "conversation-a");
    let joiner = identity("user-b", "conversation-b");

    let owner_prepared = resolver
        .prepare_initialize(&owner, Some("shared-name"))
        .unwrap();
    resolver.commit_initialize(&owner_prepared).unwrap();

    let joined = resolver
        .prepare_initialize(&joiner, Some("shared-name"))
        .unwrap();
    assert!(joined.joined);
    assert!(!joined.reused_existing_binding);
    assert_eq!(
        joined.project.effective_project_key,
        owner_prepared.project.effective_project_key
    );
    assert_ne!(
        joined.project.native_project_key,
        owner_prepared.project.native_project_key
    );
}
