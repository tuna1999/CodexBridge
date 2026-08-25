//! Minimal deterministic stdio MCP used by `scripts/mcp_smoke.sh` to verify
//! direct aggregation and gateway dispatch without any external dependency.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => json!({
                "protocolVersion":message.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18"),
                "capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"rust-agent-mock-upstream","version":"1"}
            }),
            "tools/list" => json!({"tools":[{
                "name":"mock_echo",
                "description":"Echo a bounded integration-test message.",
                "inputSchema":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}
            }]}),
            "tools/call" => {
                let name = message
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name != "mock_echo" {
                    write_error(&mut stdout, id, -32601, "unknown mock tool");
                    continue;
                }
                let echoed = message
                    .pointer("/params/arguments/message")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                json!({"content":[{"type":"text","text":format!("echo: {echoed}")}],"isError":false,"structuredContent":{"echoed":echoed}})
            }
            _ => {
                write_error(&mut stdout, id, -32601, "method not found");
                continue;
            }
        };
        writeln!(
            stdout,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"result":result})
        )
        .ok();
        stdout.flush().ok();
    }
}

fn write_error(output: &mut impl Write, id: Value, code: i64, message: &str) {
    writeln!(
        output,
        "{}",
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
    )
    .ok();
    output.flush().ok();
}
