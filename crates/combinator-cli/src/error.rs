//! CLI rendering for errors produced by the reusable core.

pub use combinator_core::CoreError as AppError;

pub fn from_codec(error: combinator_codecs::CodecError) -> AppError {
    let kind = match error.kind {
        combinator_codecs::ErrorKind::Usage => combinator_core::ErrorKind::Usage,
        combinator_codecs::ErrorKind::Runtime => combinator_core::ErrorKind::Runtime,
    };
    AppError {
        code: error.code,
        message: error.message,
        context: error.context,
        kind,
    }
}

pub fn exit_code(error: &AppError) -> i32 {
    match error.kind {
        combinator_core::ErrorKind::Usage => 2,
        combinator_core::ErrorKind::Runtime => 1,
    }
}

pub fn render(err: &AppError, json: bool) -> String {
    render_line(
        err.code,
        &err.message,
        &err.context,
        json,
        false,
        "diagnostic",
    )
}

pub fn render_streamed(err: &AppError, json: bool, event_stream: bool) -> String {
    render_line(
        err.code,
        &err.message,
        &err.context,
        json,
        event_stream,
        "diagnostic",
    )
}

pub fn render_warning(
    code: &str,
    message: &str,
    context: &[(String, String)],
    json: bool,
) -> String {
    render_line(code, message, context, json, false, "warning")
}

pub fn render_warning_streamed(
    code: &str,
    message: &str,
    context: &[(String, String)],
    json: bool,
    event_stream: bool,
) -> String {
    render_line(code, message, context, json, event_stream, "warning")
}

fn render_line(
    code: &str,
    message: &str,
    context: &[(String, String)],
    json: bool,
    event_stream: bool,
    kind: &str,
) -> String {
    if json {
        let ctx: serde_json::Map<String, serde_json::Value> = context
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        if event_stream {
            if kind == "diagnostic" {
                serde_json::json!({"kind":"diagnostic","error":{"code":code,"message":message,"context":ctx}}).to_string()
            } else {
                serde_json::json!({"kind":"warning","warning":{"code":code,"message":message,"context":ctx}}).to_string()
            }
        } else {
            serde_json::json!({"error":{"code":code,"message":message,"context":ctx}}).to_string()
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_text_with_escaped_context() {
        let error = AppError::usage("BAD\nCODE", "bad\tmessage").with("path", "a\\b\n");
        assert_eq!(exit_code(&error), 2);
        assert_eq!(
            render(&error, false),
            "error[BAD\\nCODE]: bad\\tmessage (path=a\\\\b\\n)"
        );
    }

    #[test]
    fn renders_json_and_runtime_exit_code() {
        let error = AppError::runtime("FAILED", "message").with("item", "x");
        assert_eq!(exit_code(&error), 1);
        let json: serde_json::Value = serde_json::from_str(&render(&error, true)).unwrap();
        assert_eq!(json["error"]["code"], "FAILED");
        assert_eq!(json["error"]["context"]["item"], "x");
        assert!(render_warning("WARN", "careful", &[], false).starts_with("error[WARN]"));
    }

    #[test]
    fn streamed_json_escapes_synthetic_control_characters() {
        let error = AppError::runtime("FAILED", "synthetic\nmessage").with("value", "x\t\"");
        let rendered = render_streamed(&error, true, true);
        let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(json["kind"], "diagnostic");
        assert_eq!(json["error"]["message"], "synthetic\nmessage");
        assert_eq!(json["error"]["context"]["value"], "x\t\"");
        assert!(!rendered.contains('\n'));
    }
}
