//! `hardpoint mcp`: the gatekeeper as a Model Context Protocol server, so
//! any agent that speaks MCP can ask it about devices and propose changes.
//!
//! It speaks JSON-RPC over standard input and output, one message per line,
//! and passes each request to the running daemon, which owns the hardware.
//! An agent can list, read, propose, undo and look at the journal. It is
//! given no way to keep or approve a change: those belong to a person.

use crate::model::Request;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Hardpoint lets you read and change the devices on this computer. \
Every ability has a label that decides what happens when you propose a change. \
read: it can only be read. \
reversible: the change is made at once and you can undo it. \
guarded: the change is made, then goes back by itself unless the person keeps it. \
gated: nothing is done until the person approves that exact change. \
You cannot keep or approve a change yourself. Tell the person what you proposed and how to answer. \
Call `devices` first: it lists what is attached, what each device can do, and each label.";

/// The tools, with MCP's own hints set from what each one can do. The hints
/// are only that: the daemon enforces the labels whatever a host makes of them.
fn tools() -> Value {
    let text = |description: &str| json!({ "type": "string", "description": description });
    let device = text("A device id from `devices`, for example zsa/moonlander-mk1.");
    let capability = text("An ability id from `devices`, for example light.global.");
    let change = json!({ "type": "integer", "description": "The id of a change, as returned by `propose`." });
    let reads = json!({ "readOnlyHint": true, "openWorldHint": false });
    json!([
        {
            "name": "devices",
            "description": "List the devices attached to this computer that have a card: what each can do, the label on each ability, what goes wrong if it is done carelessly, how far it has been checked, and whether this build can reach it yet.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": reads,
        },
        {
            "name": "read",
            "description": "Read one ability of one device. Reading never changes anything.",
            "inputSchema": { "type": "object", "properties": { "device": device, "capability": capability }, "required": ["device", "capability"] },
            "annotations": reads,
        },
        {
            "name": "propose",
            "description": "Propose a change to one ability of one device. What happens depends on its label: a reversible change is made at once; a guarded one is made and goes back unless the person keeps it; a gated one waits for the person's approval and does nothing until then. The reply says which happened and what the person has to do.",
            "inputSchema": {
                "type": "object",
                "properties": { "device": device, "capability": capability, "value": { "description": "The new value. `devices` shows the form each ability takes." } },
                "required": ["device", "capability", "value"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false },
        },
        {
            "name": "undo",
            "description": "Put back the value from before a change you proposed.",
            "inputSchema": { "type": "object", "properties": { "change": change }, "required": ["change"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
        },
        {
            "name": "changes",
            "description": "The journal: every change proposed since the daemon started, and what became of it.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": reads,
        },
    ])
}

fn call(params: &Value, ask: &dyn Fn(Request) -> Result<Value, String>) -> Value {
    let arguments = &params["arguments"];
    let word = |name: &str| arguments[name].as_str().map(str::to_string).ok_or(format!("`{name}` is missing"));
    let number = |name: &str| arguments[name].as_u64().ok_or(format!("`{name}` is missing"));
    let request = match params["name"].as_str().unwrap_or("") {
        "devices" => Ok(Request::Devices),
        "changes" => Ok(Request::Changes),
        "read" => word("device").and_then(|device| Ok(Request::Read { device, capability: word("capability")? })),
        "undo" => number("change").map(|change| Request::Undo { change }),
        "propose" => word("device").and_then(|device| {
            let value = arguments.get("value").cloned().ok_or("`value` is missing")?;
            Ok(Request::Propose { device, capability: word("capability")?, value })
        }),
        other => Err(format!("there is no tool called {other:?}")),
    };
    let (text, failed) = match request.and_then(ask) {
        Ok(reply) if reply["ok"].as_bool() == Some(true) => (serde_json::to_string_pretty(&reply["result"]).unwrap_or_default(), false),
        Ok(reply) => (reply["error"].as_str().unwrap_or("the daemon refused").to_string(), true),
        Err(error) => (error, true),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
}

/// Answer one message. A notification gets no answer.
pub fn answer(line: &str, ask: &dyn Fn(Request) -> Result<Value, String>) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(error) => return Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": error.to_string() } })),
    };
    let id = message.get("id").cloned().filter(|id| !id.is_null());
    let result = match message["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "hardpoint", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => call(&message["params"], ask),
        _ => return id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } })),
    };
    id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

pub fn serve(ask: &dyn Fn(Request) -> Result<Value, String>) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = answer(&line, ask) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: Request) -> Result<Value, String> {
        Err("no daemon".into())
    }

    #[test]
    fn an_agent_is_offered_no_way_to_approve_or_keep() {
        let reply = answer(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, &nothing).unwrap();
        let names: Vec<&str> = reply["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["devices", "read", "propose", "undo", "changes"]);
        for forbidden in ["keep", "approve", "decline"] {
            let line = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{forbidden}","arguments":{{"change":1}}}}}}"#);
            assert_eq!(answer(&line, &nothing).unwrap()["result"]["isError"], json!(true));
        }
    }

    #[test]
    fn the_hints_say_which_tools_only_read() {
        let reply = answer(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, &nothing).unwrap();
        let hint = |name: &str, hint: &str| {
            let tools = reply["result"]["tools"].as_array().unwrap();
            tools.iter().find(|t| t["name"] == name).unwrap()["annotations"][hint].clone()
        };
        assert_eq!(hint("devices", "readOnlyHint"), json!(true));
        assert_eq!(hint("read", "readOnlyHint"), json!(true));
        assert_eq!(hint("propose", "readOnlyHint"), json!(false));
        assert_eq!(hint("propose", "destructiveHint"), json!(true));
    }

    #[test]
    fn it_speaks_the_protocol() {
        let hello = answer(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#, &nothing).unwrap();
        assert_eq!(hello["result"]["serverInfo"]["name"], json!("hardpoint"));
        assert!(answer(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &nothing).is_none());
        assert_eq!(answer(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#, &nothing).unwrap()["error"]["code"], json!(-32601));
        // A proposal is passed on as asked, and the daemon's answer comes back as text.
        let seen = std::cell::RefCell::new(None);
        let daemon = |request: Request| {
            *seen.borrow_mut() = Some(request);
            Ok(json!({ "ok": true, "result": { "state": "applied" } }))
        };
        let line = r##"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"propose","arguments":{"device":"a/b","capability":"light.global","value":"#0000ff"}}}"##;
        let reply = answer(line, &daemon).unwrap();
        assert_eq!(reply["result"]["isError"], json!(false));
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("applied"));
        assert!(matches!(seen.borrow().as_ref(), Some(Request::Propose { device, .. }) if device == "a/b"));
    }
}
