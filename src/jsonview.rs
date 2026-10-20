/*!
A small JSON inspector. Keys, strings, and numbers are colored the way a
terminal JSON viewer is, because a post is a document you sometimes need
to read raw.
*/

use std::collections::HashSet;

use serde_json::Value;

use crate::render::{Kind, Span, StyledLine};

#[derive(Clone, Debug)]
pub struct Node {
    pub path: String,
    pub expandable: bool,
}

pub fn visible(value: &Value, open: &HashSet<String>) -> Vec<(Node, StyledLine)> {
    let mut out = Vec::new();
    walk(None, value, "", 0, true, open, &mut out);
    out
}

fn walk(
    key: Option<&str>,
    value: &Value,
    path: &str,
    depth: usize,
    force_open: bool,
    open: &HashSet<String>,
    out: &mut Vec<(Node, StyledLine)>,
) {
    let expandable = matches!(value, Value::Object(_) | Value::Array(_));
    let is_open = force_open || (expandable && open.contains(path));
    if let Some(key) = key {
        out.push((
            Node {
                path: path.to_string(),
                expandable,
            },
            field_line(key, value, depth, is_open),
        ));
    }
    if !is_open {
        return;
    }
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                let child_path = if path.is_empty() {
                    child_key.clone()
                } else {
                    format!("{path}.{child_key}")
                };
                walk(
                    Some(child_key),
                    child,
                    &child_path,
                    depth + if key.is_some() { 1 } else { 0 },
                    false,
                    open,
                    out,
                );
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                let child_path = format!("{path}[{i}]");
                walk(
                    Some(&format!("[{i}]")),
                    child,
                    &child_path,
                    depth + 1,
                    false,
                    open,
                    out,
                );
            }
        }
        _ => {}
    }
}

fn field_line(key: &str, value: &Value, depth: usize, open: bool) -> StyledLine {
    let indent = "  ".repeat(depth);
    let mut spans = vec![
        Span {
            kind: Kind::Dim,
            text: indent,
        },
        Span {
            kind: Kind::Bold,
            text: key.to_string(),
        },
        Span {
            kind: Kind::Dim,
            text: ": ".into(),
        },
    ];
    match value {
        Value::Null => spans.push(Span {
            kind: Kind::Dim,
            text: "null".into(),
        }),
        Value::Bool(v) => spans.push(Span {
            kind: Kind::Blue,
            text: v.to_string(),
        }),
        Value::Number(v) => spans.push(Span {
            kind: Kind::Blue,
            text: v.to_string(),
        }),
        Value::String(v) => spans.push(Span {
            kind: Kind::Blue,
            text: format!("\"{}\"", escape_preview(v)),
        }),
        Value::Array(items) if !open => spans.push(Span {
            kind: Kind::Dim,
            text: format!("[{}]", items.len()),
        }),
        Value::Array(_) => spans.push(Span {
            kind: Kind::Dim,
            text: "[".into(),
        }),
        Value::Object(map) if !open => spans.push(Span {
            kind: Kind::Dim,
            text: format!("{{… {} }}", map.len()),
        }),
        Value::Object(_) => spans.push(Span {
            kind: Kind::Dim,
            text: "{".into(),
        }),
    }
    StyledLine {
        spans,
        selected: false,
        hl: None,
        bar: false,
    }
}

fn escape_preview(text: &str) -> String {
    let flat: String = text.replace('\n', "\\n");
    if flat.chars().count() > 80 {
        let cut: String = flat.chars().take(79).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

pub fn toggle(open: &mut HashSet<String>, node: &Node) {
    if !node.expandable || node.path.is_empty() {
        return;
    }
    if open.contains(&node.path) {
        open.remove(&node.path);
    } else {
        open.insert(node.path.clone());
    }
}
