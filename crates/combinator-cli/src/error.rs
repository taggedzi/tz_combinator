//! CLI rendering for errors produced by the reusable core.

pub use combinator_core::CoreError as AppError;

pub fn exit_code(error: &AppError) -> i32 {
    match error.kind {
        combinator_core::ErrorKind::Usage => 2,
        combinator_core::ErrorKind::Runtime => 1,
    }
}

pub fn render(err: &AppError, json: bool) -> String {
    render_line(err.code, &err.message, &err.context, json)
}

pub fn render_warning(
    code: &str,
    message: &str,
    context: &[(String, String)],
    json: bool,
) -> String {
    render_line(code, message, context, json)
}

fn render_line(code: &str, message: &str, context: &[(String, String)], json: bool) -> String {
    if json {
        let ctx: serde_json::Map<String, serde_json::Value> = context
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        serde_json::json!({"error":{"code":code,"message":message,"context":ctx}}).to_string()
    } else if context.is_empty() {
        format!("error[{}]: {}", escape_text(code), escape_text(message))
    } else {
        let ctx = context
            .iter()
            .map(|(k, v)| format!("{}={}", escape_text(k), escape_text(v)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "error[{}]: {} ({ctx})",
            escape_text(code),
            escape_text(message)
        )
    }
}

fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
