use codex_bridge::{
    project::ProjectResolver,
    request_context::{RequestIdentity, TransportMode},
    storage::{PlanItemRecord, Storage},
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
fn memory_and_plan_survive_storage_reopen_together() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("state.sqlite3");
    {
        let storage = Storage::open(&database).unwrap();
        storage
            .memory_set("project", "decision", "keep sqlite")
            .unwrap();
        storage
            .plan_set(
                "project",
                Some("contract".to_owned()),
                vec![PlanItemRecord {
                    step: "verify".to_owned(),
                    status: "completed".to_owned(),
                }],
            )
            .unwrap();
    }

    let reopened = Storage::open(&database).unwrap();
    assert_eq!(
        reopened
            .memory_get("project", "decision")
            .unwrap()
            .as_deref(),
        Some("keep sqlite")
    );
    let plan = reopened.plan_get("project").unwrap().unwrap();
    assert_eq!(plan.explanation.as_deref(), Some("contract"));
    assert_eq!(plan.items[0].step, "verify");
    assert_eq!(plan.items[0].status, "completed");
}

#[test]
fn invalid_plan_update_preserves_last_committed_state() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(&temp.path().join("state.sqlite3")).unwrap();
    let saved = storage
        .plan_set(
            "project",
            None,
            vec![PlanItemRecord {
                step: "keep".to_owned(),
                status: "in_progress".to_owned(),
            }],
        )
        .unwrap();

    let error = storage
        .plan_set(
            "project",
            None,
            vec![
                PlanItemRecord {
                    step: "one".to_owned(),
                    status: "in_progress".to_owned(),
                },
                PlanItemRecord {
                    step: "two".to_owned(),
                    status: "in_progress".to_owned(),
                },
            ],
        )
        .unwrap_err();
    assert_eq!(error.code(), "INVALID_INPUT");
    assert_eq!(storage.plan_get("project").unwrap().unwrap(), saved);
}

#[test]
fn conversations_joining_one_alias_share_effective_state_but_not_native_identity() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(&temp.path().join("state.sqlite3")).unwrap();
    let resolver = ProjectResolver::new(temp.path().join("workspace"), storage.clone()).unwrap();
    let owner = resolver
        .initialize(&identity("owner", "conversation-a"), Some("team"))
        .unwrap()
        .0;
    let joiner = resolver
        .initialize(&identity("joiner", "conversation-b"), Some("team"))
        .unwrap()
        .0;

    assert_ne!(owner.native_project_key, joiner.native_project_key);
    assert_eq!(owner.effective_project_key, joiner.effective_project_key);
    storage
        .memory_set(owner.effective_project_key.as_str(), "shared", "visible")
        .unwrap();
    assert_eq!(
        storage
            .memory_get(joiner.effective_project_key.as_str(), "shared")
            .unwrap()
            .as_deref(),
        Some("visible")
    );
}

#[test]
fn unrelated_effective_projects_keep_memory_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(&temp.path().join("state.sqlite3")).unwrap();
    let resolver = ProjectResolver::new(temp.path().join("workspace"), storage.clone()).unwrap();
    let first = resolver
        .initialize(&identity("user", "one"), None)
        .unwrap()
        .0;
    let second = resolver
        .initialize(&identity("user", "two"), None)
        .unwrap()
        .0;
    assert_ne!(first.effective_project_key, second.effective_project_key);

    storage
        .memory_set(first.effective_project_key.as_str(), "private", "one")
        .unwrap();
    assert_eq!(
        storage
            .memory_get(second.effective_project_key.as_str(), "private")
            .unwrap(),
        None
    );
}
