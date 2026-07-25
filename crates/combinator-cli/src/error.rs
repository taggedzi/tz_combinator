//! Stable, machine- and human-readable diagnostics.

#[derive(Debug)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub context: Vec<(String, String)>,
    pub exit: i32,
}

impl AppError {
    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), context: Vec::new(), exit: 2 }
    }

    pub fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), context: Vec::new(), exit: 1 }
    }

    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.context.push((key.to_string(), value.to_string()));
        self
    }
}

/// Renders a diagnostic as a single stderr line.
pub fn render(err: &AppError, json: bool) -> String {
    render_line(err.code, &err.message, &err.context, json)
}

/// Renders a non-fatal warning (exit code unaffected).
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
        let obj = serde_json::json!({
            "error": { "code": code, "message": message, "context": ctx }
        });
        obj.to_string()
    } else if context.is_empty() {
        format!("error[{code}]: {message}")
    } else {
        let ctx = context
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("error[{code}]: {message} ({ctx})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_error_has_exit_2() {
        let e = AppError::usage("NO_LISTS", "no input lists were provided");
        assert_eq!(e.exit, 2);
        assert_eq!(e.code, "NO_LISTS");
    }

    #[test]
    fn runtime_error_has_exit_1() {
        let e = AppError::runtime("OUTPUT_EXISTS", "output file already exists");
        assert_eq!(e.exit, 1);
    }

    #[test]
    fn text_render_is_stable() {
        let e = AppError::runtime("OUTPUT_EXISTS", "output file already exists")
            .with("path", "out.txt");
        assert_eq!(
            render(&e, false),
            "error[OUTPUT_EXISTS]: output file already exists (path=out.txt)"
        );
    }

    #[test]
    fn json_render_is_parseable() {
        let e = AppError::runtime("INSUFFICIENT_SPACE", "not enough disk space")
            .with("needed", 100u64)
            .with("available", 40u64);
        let line = render(&e, true);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["code"], "INSUFFICIENT_SPACE");
        assert_eq!(v["error"]["context"]["needed"], "100");
    }
}
