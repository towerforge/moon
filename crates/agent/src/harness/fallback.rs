//! A call the model wrote as text. Small local models, `qwen2.5-coder`
//! among them, answer a request that offers tools with the call as plain
//! JSON in the reply (`{"name": "write_file", "arguments": {…}}`), which
//! Ollama passes on as content with no `tool_calls`. It is a call all the
//! same: this finds it, and only it, so an ordinary reply that happens to
//! contain JSON is left alone.

use moon_core::ToolCall;
use serde_json::Value;

use crate::agents::Agent;

/// The calls written in `text`, if it is nothing but calls: one object, an
/// array of them, one per `<tool_call>` block, inside a code fence or bare.
/// Empty if the text is a reply.
pub fn calls_in_text(text: &str, agent: &Agent) -> Vec<ToolCall> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    // `<tool_call>…</tool_call>` blocks, the shape the Qwen templates use
    let blocks: Vec<&str> = text
        .split("<tool_call>")
        .skip(1)
        .filter_map(|b| b.split("</tool_call>").next())
        .collect();
    if !blocks.is_empty() {
        let calls: Vec<ToolCall> = blocks
            .iter()
            .flat_map(|b| parse(unfence(b), agent))
            .collect();
        return calls;
    }
    let body = unfence(text);
    let calls = parse(body, agent);
    if !calls.is_empty() {
        return calls;
    }
    // a code fence somewhere in the prose that holds nothing but calls: the
    // model explained itself and then wrote the call the way it was told not to
    let fenced: Vec<&str> = text
        .split("```")
        .skip(1)
        .step_by(2)
        .map(|b| {
            b.trim_start_matches(|c: char| c.is_ascii_alphanumeric())
                .trim()
        })
        .filter(|b| b.starts_with('{') || b.starts_with('['))
        .collect();
    if !fenced.is_empty() {
        let calls: Vec<ToolCall> = fenced.iter().flat_map(|b| parse(b, agent)).collect();
        if !calls.is_empty() {
            return calls;
        }
    }
    // prose around one object: from the first `{` to the last `}`, as long
    // as the prose is a short lead-in and not the answer itself
    if let (Some(a), Some(b)) = (body.find('{'), body.rfind('}')) {
        if a < b && a <= 80 && body.len() - b <= 80 {
            return parse(&body[a..=b], agent);
        }
    }
    Vec::new()
}

/// Without a ```json fence around it.
fn unfence(s: &str) -> &str {
    let s = s.trim();
    let Some(inner) = s.strip_prefix("```") else {
        return s;
    };
    let inner = inner.trim_start_matches(|c: char| c.is_ascii_alphanumeric());
    inner.strip_suffix("```").unwrap_or(inner).trim()
}

/// A JSON object or array of objects, every one a call to a tool the agent has.
fn parse(s: &str, agent: &Agent) -> Vec<ToolCall> {
    let Ok(v) = serde_json::from_str::<Value>(s) else {
        return Vec::new();
    };
    let items: Vec<&Value> = match &v {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let calls: Vec<ToolCall> = items.iter().filter_map(|v| one(v, agent)).collect();
    if calls.len() == items.len() {
        calls
    } else {
        Vec::new()
    }
}

fn one(v: &Value, agent: &Agent) -> Option<ToolCall> {
    // `{"function": {"name", "arguments"}}` is the wire shape; unwrap it
    let v = v.get("function").unwrap_or(v);
    let name = v.get("name")?.as_str()?;
    if !agent.tools.iter().any(|t| t.name() == name) {
        return None;
    }
    let arguments = match v.get("arguments").or_else(|| v.get("parameters")) {
        None => Value::Object(Default::default()),
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::String(s.clone())),
        Some(a) => a.clone(),
    };
    Some(ToolCall {
        id: None,
        name: name.to_string(),
        arguments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::editor;
    use serde_json::json;

    fn names(text: &str) -> Vec<String> {
        calls_in_text(text, &editor())
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    #[test]
    fn bare_fenced_tagged_and_wrapped() {
        let call = r#"{"name": "write_file", "arguments": {"path": "test.txt", "content": "hi"}}"#;
        let c = calls_in_text(call, &editor());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "write_file");
        assert_eq!(c[0].arguments["path"], "test.txt");
        assert!(c[0].id.is_none());
        assert_eq!(names(&format!("```json\n{call}\n```")), vec!["write_file"]);
        assert_eq!(
            names(&format!("<tool_call>\n{call}\n</tool_call>")),
            vec!["write_file"]
        );
        assert_eq!(
            names(&format!(
                "<tool_call>{call}</tool_call>\n<tool_call>{{\"name\": \"list_dir\"}}</tool_call>"
            )),
            vec!["write_file", "list_dir"]
        );
        assert_eq!(
            names(&format!(
                "[{call}, {{\"name\": \"list_dir\", \"parameters\": {{}}}}]"
            )),
            vec!["write_file", "list_dir"]
        );
        assert_eq!(
            names(&format!("{{\"function\": {call}}}")),
            vec!["write_file"]
        );
        // a short lead-in before the object is still a call
        assert_eq!(
            names(&format!("Claro, aquí tienes:\n{call}")),
            vec!["write_file"]
        );
        // arguments as a JSON string are parsed
        let c = calls_in_text(
            r#"{"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}"#,
            &editor(),
        );
        assert_eq!(c[0].arguments, json!({"path": "a.rs"}));
    }

    #[test]
    fn replies_are_left_alone() {
        assert!(names("Sí, puedo crear ficheros.").is_empty());
        assert!(names("").is_empty());
        // JSON that is not a call, or a call to a tool that does not exist
        assert!(names(r#"{"path": "test.txt"}"#).is_empty());
        assert!(names(r#"{"name": "bash", "arguments": {"cmd": "rm -rf /"}}"#).is_empty());
        // one real call next to a bogus one is not trusted
        assert!(names(r#"[{"name": "list_dir"}, {"name": "nope"}]"#).is_empty());
        // an answer that explains the format, with the object deep inside
        let long = format!(
            "{} {{\"name\": \"read_file\", \"arguments\": {{}}}} {}",
            "x".repeat(200),
            "y".repeat(200)
        );
        assert!(names(&long).is_empty());
    }

    #[test]
    fn a_fenced_call_after_prose_of_any_length() {
        let call = r#"{"name": "write_file", "arguments": {"path": "text.txt", "content": "hi"}}"#;
        // qwen2.5-coder, once told to act: a sentence, then the call in a
        // fence it was told not to use
        let long = format!(
            "Sure, I'll add five sentences about Steve Jobs to `text.txt`, one per line, \
             in the order they were said, and then tell you what changed.\n\n```json\n{call}\n```"
        );
        assert_eq!(names(&long), vec!["write_file"]);
        // a fence that is not a call is prose, whatever it holds
        assert!(names("Use it like this:\n```json\n{\"path\": \"x\"}\n```").is_empty());
        assert!(names("```rust\nfn main() {}\n```").is_empty());
        assert!(names("Two blocks:\n```\n{\"a\": 1}\n```\nand\n```json\n[]\n```").is_empty());
    }
}
