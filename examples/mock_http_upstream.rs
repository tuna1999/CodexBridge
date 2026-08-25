//! Deterministic Streamable HTTP MCP peer used by the integration smoke test.
//! It intentionally implements only initialize, notifications, tools/list, and
//! tools/call so the aggregator exercises RMCP's real HTTP client transport.

use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};

async fn mcp(Json(message): Json<Value>) -> Response {
    let Some(id) = message.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let response = match method {
        "initialize" => json!({
            "jsonrpc":"2.0",
            "id":id,
            "result":{
                "protocolVersion":message.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
                "capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"rust-agent-mock-http-upstream","version":"1"}
            }
        }),
        "tools/list" => json!({
            "jsonrpc":"2.0",
            "id":id,
            "result":{"tools":[{
                "name":"http_echo",
                "description":"Echo through a Streamable HTTP integration-test upstream.",
                "inputSchema":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}
            }]}
        }),
        "tools/call" => {
            let echoed = message
                .pointer("/params/arguments/message")
                .and_then(Value::as_str)
                .unwrap_or("");
            json!({
                "jsonrpc":"2.0",
                "id":id,
                "result":{
                    "content":[{"type":"text","text":format!("http-echo: {echoed}")}],
                    "isError":false,
                    "structuredContent":{"echoed":echoed}
                }
            })
        }
        _ => json!({
            "jsonrpc":"2.0",
            "id":id,
            "error":{"code":-32601,"message":"method not found"}
        }),
    };
    Json(response).into_response()
}

fn main() {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:3056".to_owned());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build mock HTTP runtime");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .expect("bind mock HTTP listener");
        axum::serve(listener, Router::new().route("/mcp", post(mcp)))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .expect("serve mock HTTP upstream");
    });
}
