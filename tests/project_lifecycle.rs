use codex_bridge::{
    project::ProjectResolver,
    request_context::{RequestIdentity, TransportMode},
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
fn prepared_initialization_is_not_visible_until_commit() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(&temp.path().join("state.sqlite3")).unwrap();
    let resolver = ProjectResolver::new(temp.path().join("workspace"), storage).unwrap();
    let request = identity("subject", "conversation");

    let prepared = resolver
        .prepare_initialize(&request, Some("shared-project"))
        .unwrap();
    assert_eq!(
        resolver.resolve_initialized(&request).unwrap_err().code(),
        "TURN_NOT_INITIALIZED"
    );

    resolver.commit_initialize(&prepared).unwrap();
    let project = resolver.resolve_initialized(&request).unwrap();
    assert_eq!(project.project_alias.as_deref(), Some("shared-project"));
    assert_eq!(
        project.effective_project_key.as_str(),
        prepared.project.effective_project_key.as_str()
    );
}

#[test]
fn two_conversations_can_rejoin_one_alias_without_sharing_native_identity() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(&temp.path().join("state.sqlite3")).unwrap();
    let resolver = ProjectResolver::new(temp.path().join("workspace"), storage).unwrap();
    let first = identity("subject-a", "conversation-a");
    let second = identity("subject-b", "conversation-b");

    let (first_project, first_joined) = resolver.initialize(&first, Some("team-project")).unwrap();
    let (second_project, second_joined) =
        resolver.initialize(&second, Some("team-project")).unwrap();

    assert!(!first_joined);
    assert!(second_joined);
    assert_ne!(
        first_project.native_project_key.as_str(),
        second_project.native_project_key.as_str()
    );
    assert_eq!(
        first_project.effective_project_key.as_str(),
        second_project.effective_project_key.as_str()
    );
}
