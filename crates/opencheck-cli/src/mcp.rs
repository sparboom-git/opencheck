//! `opencheck mcp`: an MCP server on standard input and output for Claude
//! Desktop, so Claude can review the findings of the project open in the
//! OpenCheck app.
//!
//! The server holds no models. Each tool call goes to the running app over
//! its local connection (port and key in `%APPDATA%\OpenCheck\
//! claude-link.json`, written while File → Claude connection is switched
//! on), and the app answers from what it has open. Messages are JSON-RPC
//! 2.0, one per line; nothing else is written to standard output.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use opencheck_core::triage::LINK_FILE;
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Reviews the clash and clearance findings of the project open in \
OpenCheck. Follow the opencheck-triage skill: suggest a C402 category (C1 not critical, C2 \
critical, C3 very critical; noise is C1) with a confidence from 0 to 1 and a short reason for \
each finding or group of alike findings. The reviewer approves the suggestions in OpenCheck. \
When the user asks for issues and the reviewer allows it, create_issues makes them, shown as \
issued by Claude; you cannot approve, decide on findings or change anything else. \
finding_pictures shows findings in the 3D view, and get_finding gives their coordinates.";

const NOT_OPEN: &str = "OpenCheck is not open, or its Claude connection is off. Open the \
project in OpenCheck, run the check, and switch on File → Claude connection → Allow Claude to \
connect.";

/// The tools, with their input schemas.
fn tools() -> Value {
    let page = json!({
        "offset": { "type": "integer", "minimum": 0, "description": "Skip this many (paging)." },
        "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "At most this many (default 50)." },
    });
    let with_page = |mut properties: Value| {
        if let (Some(map), Some(extra)) = (properties.as_object_mut(), page.as_object()) {
            map.extend(extra.clone());
        }
        properties
    };
    json!([
        {
            "name": "project_info",
            "description": "The open project: models with discipline, the check results per ruleset and rule, how many findings are still unrated, whether the project has triage rules, and what C1, C2 and C3 mean.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "triage_rules",
            "description": "The project's own triage rules and the reviewer's corrections of earlier suggestions (examples). They win over general practice: read them before rating.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "list_groups",
            "description": "Findings in groups that can be judged together: same rule and the same kinds of elements (class and type on each side), largest first, with their systems and storeys counted, the depth and up to five example finding ids. A group can be mixed: check a few findings before rating it as a whole, and use list_findings with storey or rule to split it.",
            "inputSchema": { "type": "object", "properties": with_page(json!({
                "unrated": { "type": "boolean", "description": "Only findings without a suggestion or decision (default true)." },
                "rule": { "type": "string", "description": "Only rules whose name contains this." },
            })) },
        },
        {
            "name": "list_findings",
            "description": "Findings one per row: id, rule, the two elements, depth, storey, the point in IFC coordinates (metres) and any suggestion.",
            "inputSchema": { "type": "object", "properties": with_page(json!({
                "group": { "type": "string", "description": "Only the findings of this group id." },
                "rule": { "type": "string", "description": "Only rules whose name contains this." },
                "storey": { "type": "string", "description": "Only this storey." },
                "status": { "type": "string", "enum": ["unrated", "proposed", "approved", "all"], "description": "Default unrated." },
            })) },
        },
        {
            "name": "get_finding",
            "description": "One finding in full: both elements (class, name, type, systems and their kind, materials, storey, size), depth, where it is (location.ifc_point_m: x, y, z in metres in the models' IFC coordinates; location.point_m: from the origin OpenCheck shows the models around), the elements around it, and any decision, issue or suggestion.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string", "description": "The finding's id." },
            }, "required": ["id"] },
        },
        {
            "name": "get_element",
            "description": "One element with its property sets, attributes and classifications (when the reviewer shares properties).",
            "inputSchema": { "type": "object", "properties": {
                "global_id": { "type": "string" },
                "model": { "type": "string", "description": "The model's name, when the GlobalId is in several models." },
            }, "required": ["global_id"] },
        },
        {
            "name": "finding_pictures",
            "description": "Pictures of findings from OpenCheck's 3D view: each finding framed with some context around it, its first element orange and the second blue, the rest faint. Each picture is also saved as a PNG file (paths in the answer) for use in reports. The reviewer's view is put back afterwards. OpenCheck's window must not be minimized.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 12, "description": "Finding ids, at most 12 per call." },
                "view": { "type": "string", "enum": ["angle", "top", "south", "north", "west", "east", "current"], "description": "From above at an angle (default), a plan from the top, an elevation from one side, or the direction the reviewer has the view turned to." },
                "width": { "type": "integer", "minimum": 320, "maximum": 1600, "description": "Widest picture in pixels (default 800)." },
            }, "required": ["ids"] },
        },
        {
            "name": "view_picture",
            "description": "A picture of OpenCheck's 3D view exactly as the reviewer sees it now, also saved as a PNG file.",
            "inputSchema": { "type": "object", "properties": {
                "width": { "type": "integer", "minimum": 320, "maximum": 1600, "description": "Widest picture in pixels (default 800)." },
            } },
        },
        {
            "name": "rate",
            "description": "Suggests categories, for single findings or whole groups. Findings the reviewer already decided on are left alone. Send up to 50 ratings per call.",
            "inputSchema": { "type": "object", "properties": {
                "ratings": { "type": "array", "items": { "type": "object", "properties": {
                    "finding": { "type": "string", "description": "A finding id (or give group)." },
                    "group": { "type": "string", "description": "A group id from list_groups: rates all its findings." },
                    "category": { "type": "string", "enum": ["C1", "C2", "C3"] },
                    "noise": { "type": "boolean", "description": "Not a real problem at all (then C1)." },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                    "reason": { "type": "string", "description": "One or two short sentences the reviewer can check, in the user's language." },
                }, "required": ["category", "confidence", "reason"] } },
            }, "required": ["ratings"] },
        },
        {
            "name": "list_issues",
            "description": "The project's issues: number, title, status, category, responsible disciplines, who issued it (initials, or Claude), when, and its finding ids.",
            "inputSchema": { "type": "object", "properties": with_page(json!({
                "issued_by": { "type": "string", "description": "Only issues made by these initials, or by Claude." },
            })) },
        },
        {
            "name": "create_issues",
            "description": "Makes issues of findings, shown in OpenCheck as issued by Claude. Only when the user asks for issues, and only if the reviewer allows it (project_info.issues_allowed). Findings already in an issue or decided on by the reviewer are skipped. An issue's viewpoint frames its findings.",
            "inputSchema": { "type": "object", "properties": {
                "issues": { "type": "array", "items": { "type": "object", "properties": {
                    "findings": { "type": "array", "items": { "type": "string" }, "description": "Finding ids (or give group)." },
                    "group": { "type": "string", "description": "A group id from list_groups: all its findings." },
                    "category": { "type": "string", "enum": ["C1", "C2", "C3"] },
                    "bundle": { "type": "boolean", "description": "One issue for all its findings (default), or one issue per finding." },
                    "title": { "type": "string", "description": "Short title; OpenCheck makes one when left out." },
                    "description": { "type": "string", "description": "What is wrong and what must be done, in the user's language." },
                }, "required": ["category"] } },
            }, "required": ["issues"] },
        },
        {
            "name": "summary",
            "description": "Where the review stands: suggestions per category and confidence, approved, and unrated findings.",
            "inputSchema": { "type": "object", "properties": {} },
        },
    ])
}

/// `%APPDATA%\OpenCheck\claude-link.json`, as the app writes it.
fn link_file() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OpenCheck")
        .join(LINK_FILE)
}

/// The connection to the running app.
struct App {
    stream: Option<(TcpStream, BufReader<TcpStream>, String)>,
}

impl App {
    fn connect() -> Option<(TcpStream, BufReader<TcpStream>, String)> {
        let text = std::fs::read_to_string(link_file()).ok()?;
        let info: Value = serde_json::from_str(&text).ok()?;
        let port = info["port"].as_u64()? as u16;
        let key = info["key"].as_str()?.to_owned();
        let stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(150)))
            .ok()?;
        let reader = BufReader::new(stream.try_clone().ok()?);
        Some((stream, reader, key))
    }

    /// Sends a call, connecting (again) when needed.
    fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        for _ in 0..2 {
            if self.stream.is_none() {
                self.stream = Self::connect();
            }
            let Some((stream, reader, key)) = &mut self.stream else {
                return Err(NOT_OPEN.to_owned());
            };
            let request = json!({ "key": key, "tool": tool, "args": args });
            let mut line = String::new();
            let sent = writeln!(stream, "{request}").and_then(|_| stream.flush());
            if sent.is_ok() && reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                let answer: Value =
                    serde_json::from_str(&line).map_err(|e| format!("bad answer: {e}"))?;
                return match answer.get("ok") {
                    Some(value) => Ok(value.clone()),
                    None => Err(answer["error"].as_str().unwrap_or("failed").to_owned()),
                };
            }
            // The app was restarted or switched off: try a fresh connection.
            self.stream = None;
        }
        Err(NOT_OPEN.to_owned())
    }
}

/// An answer as MCP content: the text, then any pictures as images.
fn content(mut value: Value) -> Vec<Value> {
    let mut images = Vec::new();
    if let Some(pictures) = value["pictures"].as_array_mut() {
        for picture in pictures {
            if let Some(data) = picture.as_object_mut().and_then(|p| p.remove("png_base64")) {
                images.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
            }
        }
    }
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    std::iter::once(json!({ "type": "text", "text": text }))
        .chain(images)
        .collect()
}

pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut app = App { stream: None };
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            let error = json!({ "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": "parse error" } });
            writeln!(stdout, "{error}")?;
            stdout.flush()?;
            continue;
        };
        // Notifications get no answer.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "opencheck", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Ok(match app.call(name, &args) {
                    Ok(value) => json!({ "content": content(value), "isError": false }),
                    Err(e) => json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true,
                    }),
                })
            }
            other => Err(json!({ "code": -32601, "message": format!("unknown method {other}") })),
        };
        let reply = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        writeln!(stdout, "{reply}")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_schema() {
        let tools = tools();
        let list = tools.as_array().unwrap();
        assert_eq!(list.len(), 12);
        for t in list {
            assert!(t["name"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn pictures_become_images_after_the_text() {
        let answer = json!({
            "pictures": [{ "finding": "f1", "file": "f1.png", "png_base64": "iVBOR" }],
            "missing": [],
        });
        let content = content(answer);
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        let text = content[0]["text"].as_str().unwrap();
        assert!(text.contains("f1.png") && !text.contains("iVBOR"));
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["data"], "iVBOR");
        assert_eq!(content[1]["mimeType"], "image/png");
    }
}
