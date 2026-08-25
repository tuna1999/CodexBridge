use std::collections::BTreeMap;

use codex_bridge::config::{ConfigBuilder, UpstreamMode};

fn config_for(
    path: &std::path::Path,
) -> Result<codex_bridge::config::Config, codex_bridge::error::AppError> {
    ConfigBuilder::from_map(BTreeMap::from([
        ("MCP_AUTH_TOKEN".to_owned(), "1234567890abcdef".to_owned()),
        ("MCP_UPSTREAM_CONFIG".to_owned(), path.display().to_string()),
    ]))
    .build()
}

#[test]
fn standard_mcp_servers_yaml_loads_stdio_http_and_modes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("upstreams.yaml");
    std::fs::write(
        &path,
        r#"
mcpServers:
  local:
    command: mock-server
    args: [--stdio]
    env:
      MODE: test
  remote:
    type: streamable-http
    url: https://example.invalid/mcp
    mode: direct
  disabled:
    command: ignored
    disabled: true
"#,
    )
    .unwrap();
    let config = config_for(&path).unwrap();
    assert_eq!(config.upstreams.len(), 3);
    let local = &config.upstreams["local"];
    assert_eq!(local.command.as_deref(), Some("mock-server"));
    assert_eq!(local.args, vec!["--stdio"]);
    assert_eq!(local.env.get("MODE").map(String::as_str), Some("test"));
    assert!(matches!(local.mode, UpstreamMode::Gateway));

    let remote = &config.upstreams["remote"];
    assert_eq!(remote.transport.as_deref(), Some("streamable-http"));
    assert_eq!(remote.url.as_deref(), Some("https://example.invalid/mcp"));
    assert!(matches!(remote.mode, UpstreamMode::Direct));
    assert!(config.upstreams["disabled"].disabled);
}

#[test]
fn upstream_tool_allowlist_is_preserved_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("upstreams.json");
    std::fs::write(
        &path,
        r#"{"mcpServers":{"one":{"command":"mock","tools":["alpha","beta"]}}}"#,
    )
    .unwrap();
    let config = config_for(&path).unwrap();
    assert_eq!(
        config.upstreams["one"].tools.as_deref(),
        Some(&["alpha".to_owned(), "beta".to_owned()][..])
    );
}

#[test]
fn invalid_upstream_environment_key_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("upstreams.yaml");
    std::fs::write(
        &path,
        "mcpServers:\n  bad:\n    command: mock\n    env:\n      'BAD=KEY': value\n",
    )
    .unwrap();
    let error = config_for(&path).unwrap_err();
    assert_eq!(error.code(), "CONFIG_ERROR");
}

#[test]
fn upstream_config_size_limit_fails_before_parsing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("huge.yaml");
    std::fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    let error = config_for(&path).unwrap_err();
    assert_eq!(error.code(), "CONFIG_ERROR");
    assert!(error.message().contains("exceeds"));
}

#[test]
fn upstream_server_count_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("many.yaml");
    let mut yaml = String::from("mcpServers:\n");
    for index in 0..65 {
        yaml.push_str(&format!("  server_{index}:\n    command: mock\n"));
    }
    std::fs::write(&path, yaml).unwrap();
    let error = config_for(&path).unwrap_err();
    assert_eq!(error.code(), "CONFIG_ERROR");
}
