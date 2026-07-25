//! Small, bounded templates for rendering selected list fields.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Reference {
    Index(usize),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Literal(String),
    Reference(Reference),
}

/// A compiled literal/field template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pieces: Vec<Piece>,
}

/// Errors returned while parsing, validating, or rendering a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    InvalidSyntax { position: usize },
    InvalidReference { position: usize },
    InvalidName { position: usize },
    DuplicateName { position: usize },
    NameCountMismatch { expected: usize, actual: usize },
    UnknownField { position: usize },
}

impl Template {
    /// Parses literals, `{0}`-style indices, `{name}` references, and doubled
    /// braces (`{{` / `}}`).
    pub fn parse(source: &str) -> Result<Self, TemplateError> {
        let chars: Vec<char> = source.chars().collect();
        let mut pieces = Vec::new();
        let mut literal = String::new();
        let mut i = 0;

        while i < chars.len() {
            match chars[i] {
                '{' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                    literal.push('{');
                    i += 2;
                }
                '{' => {
                    if !literal.is_empty() {
                        pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                    }
                    let start = i;
                    i += 1;
                    let content_start = i;
                    while i < chars.len() && chars[i] != '}' {
                        if chars[i] == '{' {
                            return Err(TemplateError::InvalidSyntax { position: i });
                        }
                        i += 1;
                    }
                    if i == chars.len() || i == content_start {
                        return Err(TemplateError::InvalidSyntax { position: start });
                    }
                    let content: String = chars[content_start..i].iter().collect();
                    let reference = if content.chars().all(|c| c.is_ascii_digit()) {
                        content
                            .parse::<usize>()
                            .map(Reference::Index)
                            .map_err(|_| TemplateError::InvalidReference { position: start })?
                    } else if valid_name(&content) {
                        Reference::Name(content)
                    } else {
                        return Err(TemplateError::InvalidReference { position: start });
                    };
                    pieces.push(Piece::Reference(reference));
                    i += 1;
                }
                '}' if i + 1 < chars.len() && chars[i + 1] == '}' => {
                    literal.push('}');
                    i += 2;
                }
                '}' => return Err(TemplateError::InvalidSyntax { position: i }),
                c => {
                    literal.push(c);
                    i += 1;
                }
            }
        }

        if !literal.is_empty() {
            pieces.push(Piece::Literal(literal));
        }
        Ok(Self { pieces })
    }

    /// Validates names and all references against the selected list count.
    pub fn validate_fields(
        &self,
        names: &[String],
        field_count: usize,
    ) -> Result<(), TemplateError> {
        validate_names(names, field_count)?;
        for piece in &self.pieces {
            if let Piece::Reference(reference) = piece {
                match reference {
                    Reference::Index(index) if *index >= field_count => {
                        return Err(TemplateError::UnknownField { position: *index });
                    }
                    Reference::Name(name) if names.iter().position(|n| n == name).is_none() => {
                        return Err(TemplateError::UnknownField {
                            position: name.len(),
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Renders against selected fields after `validate_fields` has succeeded.
    pub fn render(&self, fields: &[&str], names: &[String]) -> Result<String, TemplateError> {
        let mut output = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Literal(value) => output.push_str(value),
                Piece::Reference(Reference::Index(index)) => output.push_str(
                    fields
                        .get(*index)
                        .ok_or(TemplateError::UnknownField { position: *index })?,
                ),
                Piece::Reference(Reference::Name(name)) => {
                    let index = names.iter().position(|candidate| candidate == name).ok_or(
                        TemplateError::UnknownField {
                            position: name.len(),
                        },
                    )?;
                    output.push_str(
                        fields
                            .get(index)
                            .ok_or(TemplateError::UnknownField { position: index })?,
                    );
                }
            }
        }
        Ok(output)
    }
}

/// Validates a field name according to the F3 identifier grammar.
pub fn validate_name(name: &str) -> bool {
    valid_name(name)
}

fn validate_names(names: &[String], field_count: usize) -> Result<(), TemplateError> {
    if !names.is_empty() && names.len() != field_count {
        return Err(TemplateError::NameCountMismatch {
            expected: field_count,
            actual: names.len(),
        });
    }
    for (index, name) in names.iter().enumerate() {
        if !valid_name(name) {
            return Err(TemplateError::InvalidName { position: index });
        }
        if names[..index].iter().any(|previous| previous == name) {
            return Err(TemplateError::DuplicateName { position: index });
        }
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn renders_positional_fields_and_literals() {
        let template = Template::parse("https://{0}/{1}").unwrap();
        let fields = ["host", "path"];
        template.validate_fields(&[], fields.len()).unwrap();
        assert_eq!(template.render(&fields, &[]).unwrap(), "https://host/path");
    }

    #[test]
    fn renders_named_fields() {
        let template = Template::parse("{host}:{port}").unwrap();
        let names = names(&["host", "port"]);
        let fields = ["server", "443"];
        template.validate_fields(&names, fields.len()).unwrap();
        assert_eq!(template.render(&fields, &names).unwrap(), "server:443");
    }

    #[test]
    fn escaped_braces_are_literals() {
        let template = Template::parse("{{{0}}}").unwrap();
        let fields = ["x"];
        template.validate_fields(&[], 1).unwrap();
        assert_eq!(template.render(&fields, &[]).unwrap(), "{x}");
    }

    #[test]
    fn rejects_malformed_templates() {
        for source in ["{", "}", "{}", "{a", "{0{1}", "{0}}x"] {
            assert!(Template::parse(source).is_err(), "{source:?}");
        }
    }

    #[test]
    fn rejects_unknown_and_invalid_names() {
        let template = Template::parse("{missing}").unwrap();
        let names = names(&["host"]);
        assert!(matches!(
            template.validate_fields(&names, 1),
            Err(TemplateError::UnknownField { .. })
        ));
        assert!(!validate_name("1host"));
        assert!(!validate_name("host space"));
        assert!(validate_name("host-1.value"));
    }

    #[test]
    fn rejects_name_count_and_duplicates() {
        let template = Template::parse("{host}").unwrap();
        assert!(matches!(
            template.validate_fields(&names(&["host"]), 2),
            Err(TemplateError::NameCountMismatch { .. })
        ));
        assert!(matches!(
            template.validate_fields(&names(&["host", "host"]), 2),
            Err(TemplateError::DuplicateName { .. })
        ));
    }
}
