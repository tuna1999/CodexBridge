use std::collections::BTreeMap;

use codex_bridge::config::{AuthMode, ConfigBuilder};

fn base() -> BTreeMap<String, String> {
    BTreeMap::from([("MCP_AUTH_TOKEN".to_owned(), "1234567890abcdef".to_owned())])
}

#[test]
fn default_execution_policy_matches_documented_yolo_model() {
    let config = ConfigBuilder::from_map(base()).build().unwrap();
    assert_eq!(config.sandbox_backend, "auto");
    assert!(config.allow_unsandboxed_exec);
    assert!(config.allowed_hosts.is_empty());
    assert_eq!(config.auth_mode, AuthMode::Path);
}

#[test]
fn allowed_hosts_are_trimmed_and_empty_entries_are_removed() {
    let mut values = base();
    values.insert(
        "MCP_ALLOWED_HOSTS".to_owned(),
        " crates.io, github.com ,,registry.npmjs.org ".to_owned(),
    );
    let config = ConfigBuilder::from_map(values).build().unwrap();
    assert_eq!(
        config.allowed_hosts,
        vec!["crates.io", "github.com", "registry.npmjs.org"]
    );
}

#[test]
fn diagnostic_summary_does_not_expose_auth_token() {
    let token = "super-secret-token-value";
    let config = ConfigBuilder::from_map(BTreeMap::from([(
        "MCP_AUTH_TOKEN".to_owned(),
        token.to_owned(),
    )]))
    .build()
    .unwrap();
    let rendered = config.diagnostic_summary().to_string();
    assert!(!rendered.contains(token));
    assert!(rendered.contains("\"yolo_tools\":true"));
    assert!(rendered.contains("\"init_required\":true"));
}

#[test]
fn nested_concurrency_limits_fail_closed() {
    let mut values = base();
    values.insert("MAX_CONCURRENT_PROCESSES".to_owned(), "2".to_owned());
    values.insert("MAX_PROJECT_PROCESSES".to_owned(), "3".to_owned());
    let error = ConfigBuilder::from_map(values).build().unwrap_err();
    assert_eq!(error.code(), "CONFIG_ERROR");
    assert!(error.message().contains("MAX_PROJECT_PROCESSES"));
}

#[test]
fn input_write_and_patch_limits_cannot_exceed_request_body_limit() {
    for key in [
        "MAX_INPUT_STRING_BYTES",
        "MAX_WRITE_BYTES",
        "MAX_PATCH_BYTES",
    ] {
        let mut values = base();
        values.insert("MAX_REQUEST_BODY_BYTES".to_owned(), "4096".to_owned());
        values.insert("MAX_INPUT_STRING_BYTES".to_owned(), "1024".to_owned());
        values.insert("MAX_WRITE_BYTES".to_owned(), "1024".to_owned());
        values.insert("MAX_PATCH_BYTES".to_owned(), "1024".to_owned());
        values.insert(key.to_owned(), "8192".to_owned());
        let error = ConfigBuilder::from_map(values).build().unwrap_err();
        assert_eq!(error.code(), "CONFIG_ERROR", "{key}");
        assert!(error.message().contains(key), "{key}: {}", error.message());
    }
}

#[test]
fn invalid_auth_and_sandbox_modes_are_rejected() {
    let mut bad_auth = base();
    bad_auth.insert("MCP_AUTH_MODE".to_owned(), "sometimes".to_owned());
    assert_eq!(
        ConfigBuilder::from_map(bad_auth)
            .build()
            .unwrap_err()
            .code(),
        "CONFIG_ERROR"
    );

    let mut bad_sandbox = base();
    bad_sandbox.insert("MCP_EXEC_SANDBOX".to_owned(), "magic".to_owned());
    assert_eq!(
        ConfigBuilder::from_map(bad_sandbox)
            .build()
            .unwrap_err()
            .code(),
        "CONFIG_ERROR"
    );
}
