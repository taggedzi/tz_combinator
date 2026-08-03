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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pieces: Vec<Piece>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    InvalidSyntax { position: usize },
    InvalidReference { position: usize },
    InvalidName { position: usize },
    DuplicateName { position: usize },
    NameCountMismatch { expected: usize, actual: usize },
    UnknownField { position: usize },
    OutputTooLarge { limit: u128 },
    OutputEncoding,
}

impl Template {
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
                        return Err(TemplateError::UnknownField { position: *index })
                    }
                    Reference::Name(name) if names.iter().position(|n| n == name).is_none() => {
                        return Err(TemplateError::UnknownField {
                            position: name.len(),
                        })
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn render(&self, fields: &[&str], names: &[String]) -> Result<String, TemplateError> {
        self.render_bounded(fields, names, u128::MAX)
    }

    /// Renders only after the exact expanded byte length is known to fit.
    pub fn render_bounded(
        &self,
        fields: &[&str],
        names: &[String],
        max_bytes: u128,
    ) -> Result<String, TemplateError> {
        let mut output_len = 0u128;
        for piece in &self.pieces {
            let piece_len = match piece {
                Piece::Literal(value) => value.len(),
                Piece::Reference(reference) => resolve_reference(reference, fields, names)?.len(),
            };
            output_len = output_len
                .checked_add(piece_len as u128)
                .ok_or(TemplateError::OutputTooLarge { limit: max_bytes })?;
            if output_len > max_bytes {
                return Err(TemplateError::OutputTooLarge { limit: max_bytes });
            }
        }

        let capacity = usize::try_from(output_len)
            .map_err(|_| TemplateError::OutputTooLarge { limit: max_bytes })?;
        let mut output = String::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| TemplateError::OutputEncoding)?;
        for piece in &self.pieces {
            match piece {
                Piece::Literal(value) => output.push_str(value),
                Piece::Reference(reference) => {
                    output.push_str(resolve_reference(reference, fields, names)?)
                }
            }
        }
        Ok(output)
    }
}

fn resolve_reference<'a>(
    reference: &Reference,
    fields: &[&'a str],
    names: &[String],
) -> Result<&'a str, TemplateError> {
    match reference {
        Reference::Index(index) => fields
            .get(*index)
            .copied()
            .ok_or(TemplateError::UnknownField { position: *index }),
        Reference::Name(name) => {
            let index = names.iter().position(|candidate| candidate == name).ok_or(
                TemplateError::UnknownField {
                    position: name.len(),
                },
            )?;
            fields
                .get(index)
                .copied()
                .ok_or(TemplateError::UnknownField { position: index })
        }
    }
}

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

    #[test]
    fn repeated_references_are_rejected_before_bounded_rendering() {
        let template = Template::parse("{0}{0}{0}").unwrap();
        assert_eq!(
            template.render_bounded(&["abcd"], &[], 11),
            Err(TemplateError::OutputTooLarge { limit: 11 })
        );
        assert_eq!(
            template.render_bounded(&["abcd"], &[], 12).unwrap(),
            "abcdabcdabcd"
        );
    }
}
