//! `monocode visual-mcp` and `monocode visual …`: the tools an agent uses to
//! put a visual in its chat. Checking runs here, in the agent's own process;
//! publishing hands the checked program to the window that owns the turn,
//! over the same localhost channel as `monocode app`.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use monocode_visual::{VisualDocument, DOCUMENTATION, MAX_SOURCE_BYTES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_FRAME_BYTES: usize = 1_048_576;

/// The Claude tool prefix that runs the visual tools without asking.
pub const ALLOWED_TOOLS: &str = "mcp__monocode_visual";

fn documentation() -> Value {
    json!({
        "documentation": DOCUMENTATION,
        "cli": {
            "check": "monocode visual check --input FILE",
            "show": "monocode visual show --input FILE --title TITLE"
        }
    })
}

/// Validate without publishing. Needs no session.
fn check(source: &str) -> Result<Value, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Visual source exceeds 64 KiB".into());
    }
    let tree = monocode_visual::check(source)?;
    Ok(json!({"ok": true, "tree": tree}))
}

/// The Markdown line that places a published visual in an answer's prose.
pub fn embed(document: &VisualDocument) -> String {
    let caption: String = document
        .title
        .chars()
        .map(|c| {
            if matches!(c, '[' | ']' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    format!("![{}](visual:{})", caption.trim(), document.id)
}

/// The document a `visual_show` input names, checked and ready to send.
fn document(input: &Value) -> Result<(VisualDocument, String), String> {
    if !input.is_object() {
        return Err("Visual input must be an object".into());
    }
    for field in ["title", "id", "requestId"] {
        if input.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("{field} must be a string"));
        }
    }
    let source = input["source"]
        .as_str()
        .ok_or("source must be a JavaScript string")?;
    let request_id = input["requestId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("Request id must contain 1–128 bytes".into());
    }
    let digest = Sha256::digest(request_id.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let document = VisualDocument {
        id: input["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("visual-{digest}")),
        title: input["title"]
            .as_str()
            .unwrap_or("Visual")
            .trim()
            .to_owned(),
        source: source.to_owned(),
    };
    document.validate()?;
    monocode_visual::check(&document.source)?;
    Ok((document, request_id))
}

fn publish(input: &Value) -> Result<Value, String> {
    let (document, request_id) = document(input)?;
    let endpoint = std::env::var("MONOCODE_APP_ENDPOINT")
        .map_err(|_| "Visual publication needs a turn started in MonoBOT")?;
    let token = std::env::var("MONOCODE_APP_TOKEN")
        .map_err(|_| "Visual publication needs a turn started in MonoBOT")?;
    let address: SocketAddr = endpoint.parse().map_err(|_| "Invalid MonoBOT endpoint")?;
    if !address.ip().is_loopback() {
        return Err("MonoBOT visuals only connect to localhost".into());
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|e| format!("Cannot connect to MonoBOT at {address}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(5))))
        .map_err(|e| e.to_string())?;
    let request = json!({
        "token": token,
        "namespace": "visual",
        "action": "visual.show",
        "requestId": request_id,
        "input": {"id": document.id, "title": document.title, "source": document.source},
    });
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(MAX_FRAME_BYTES as u64)
        .read_line(&mut line)
        .map_err(|e| format!("No reply from MonoBOT: {e}"))?;
    let reply: Value =
        serde_json::from_str(&line).map_err(|_| "MonoBOT closed without a response")?;
    if reply["ok"] != true {
        return Err(reply["error"]
            .as_str()
            .unwrap_or("Visual publication failed")
            .into());
    }
    Ok(json!({
        "ok": true,
        "id": document.id,
        "embed": embed(&document),
        "placement": "Put the embed line on its own line in your final reply, where this visual belongs. Do not describe or restate the visual around it.",
    }))
}

fn tool_definitions() -> Vec<Value> {
    let properties = json!({"source":{"type":"string","description":"JavaScript function body using the native visual SDK"}});
    let mut show = properties.clone();
    show["title"] = json!({"type":"string","maxLength":160});
    show["id"] = json!({"type":"string","maxLength":96,"description":"Optional document identifier. Reusing one replaces that visual."});
    show["requestId"] = json!({"type":"string","maxLength":128,"description":"Reuse when retrying an uncertain publication"});
    vec![
        json!({"name":"visual_docs","description":"Read the visual SDK reference, examples and styling rules before building an inline chart, diagram or visualizer. Does not publish.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}),
        json!({"name":"visual_check","description":"Validate a visual program and representative controls without publishing. Returns its component tree. Read visual_docs for the SDK reference.","inputSchema":{"type":"object","properties":properties,"required":["source"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}),
        json!({"name":"visual_show","description":"Publish a visual (chart, stat row, diagram, interactive explainer) inline in this conversation. Use it for data with a trend, comparison or breakdown, and whenever the user asks for a visual. Returns an embed line: put it on its own line in your final reply where the visual belongs, between the sentences it supports. Read visual_docs first; validate with visual_check.","inputSchema":{"type":"object","properties":show,"required":["source"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}),
    ]
}

fn call_tool(name: &str, input: &Value) -> Result<Value, String> {
    match name {
        "visual_docs" => Ok(documentation()),
        "visual_check" => input["source"]
            .as_str()
            .ok_or_else(|| "source must be a JavaScript string".to_string())
            .and_then(check),
        "visual_show" => publish(input),
        _ => Err("Unknown visual tool".into()),
    }
}

fn mcp_response(request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "monocode_visual", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            Ok(match call_tool(name, &request["params"]["arguments"]) {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                }
                Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
            })
        }
        _ => Err(json!({"code": -32601, "message": "Method not found"})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
    })
}

/// The stdio MCP server. stdout carries JSON-RPC frames only.
pub fn run_mcp() -> i32 {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut line = String::new();
        if (&mut input)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_line(&mut line)
            .is_err()
        {
            return 1;
        }
        if line.is_empty() {
            return 0;
        }
        if line.len() > MAX_FRAME_BYTES {
            return 1;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => mcp_response(&request),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(response) = response {
            if writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .is_err()
            {
                return 1;
            }
        }
    }
}

const HELP: &str = "Usage: monocode visual docs\n       monocode visual check --input FILE\n       monocode visual show --input FILE [--title TITLE] [--id ID] [--request-id ID]\nFILE holds the JavaScript source; use - for stdin.";

fn run_args(args: &[String]) -> Result<Value, String> {
    let Some(action) = args.first() else {
        return Err(HELP.into());
    };
    match action.as_str() {
        "help" | "--help" | "-h" => return Ok(json!({"help": HELP})),
        "docs" if args.len() == 1 => return Ok(documentation()),
        "check" | "show" => {}
        _ => return Err(HELP.into()),
    }
    let mut input = json!({});
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--input" => {
                let reader: Box<dyn Read> = if value == "-" {
                    Box::new(std::io::stdin())
                } else {
                    Box::new(std::fs::File::open(value).map_err(|e| e.to_string())?)
                };
                let mut source = String::new();
                reader
                    .take((MAX_SOURCE_BYTES + 1) as u64)
                    .read_to_string(&mut source)
                    .map_err(|e| e.to_string())?;
                input["source"] = json!(source);
            }
            "--title" => input["title"] = json!(value),
            "--id" => input["id"] = json!(value),
            "--request-id" => input["requestId"] = json!(value),
            _ => return Err(format!("Unknown option {flag}\n{HELP}")),
        }
    }
    if input.get("source").is_none() {
        return Err(format!("--input is required\n{HELP}"));
    }
    if action == "check" {
        check(input["source"].as_str().unwrap_or(""))
    } else {
        publish(&input)
    }
}

/// `monocode visual …`: prints JSON, exits 0 on success.
pub fn run(args: Vec<String>) -> i32 {
    match run_args(&args) {
        Ok(value) => {
            match value.get("documentation").and_then(Value::as_str) {
                Some(docs) => println!("{docs}"),
                None => println!("{value}"),
            }
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, params: Value) -> Value {
        mcp_response(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).unwrap()
    }

    #[test]
    fn lists_three_tools_and_ignores_notifications() {
        let tools = call("tools/list", json!({}));
        let names: Vec<_> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["visual_docs", "visual_check", "visual_show"]);
        assert!(mcp_response(&json!({"method":"notifications/initialized"})).is_none());
    }

    #[test]
    fn checks_programs_and_reports_errors_as_tool_errors() {
        let ok = call(
            "tools/call",
            json!({"name":"visual_check","arguments":{"source":"return () => visual.text('Hi');"}}),
        );
        assert_eq!(ok["result"]["isError"], false);
        let bad = call(
            "tools/call",
            json!({"name":"visual_check","arguments":{"source":"return 3;"}}),
        );
        assert_eq!(bad["result"]["isError"], true);
    }

    #[test]
    fn publication_checks_before_it_needs_a_session() {
        let error = document(&json!({"source":"return 3;"})).unwrap_err();
        assert!(error.contains("render function"), "{error}");
        let (doc, _) = document(
            &json!({"source":"return () => visual.text('Hi');","title":"Hi [1]","id":"hi"}),
        )
        .unwrap();
        assert_eq!(embed(&doc), "![Hi  1](visual:hi)");
        let (first, _) =
            document(&json!({"source":"return () => visual.text('a');","requestId":"r"})).unwrap();
        let (again, _) =
            document(&json!({"source":"return () => visual.text('a');","requestId":"r"})).unwrap();
        assert_eq!(first.id, again.id);
    }
}
