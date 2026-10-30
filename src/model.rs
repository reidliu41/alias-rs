use std::collections::HashSet;

use chrono::{DateTime, FixedOffset};
use thiserror::Error;

/// One managed shell alias.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alias {
    pub name: String,
    pub cmd: String,
    pub desc: Option<String>,
    pub tags: Vec<String>,
    pub created_at: Option<DateTime<FixedOffset>>,
}

impl Alias {
    pub fn new(
        name: impl Into<String>,
        cmd: impl Into<String>,
        desc: Option<String>,
        tags: Vec<String>,
    ) -> Result<Self, ValidationError> {
        let cmd = cmd.into();
        let alias = Self {
            name: name.into(),
            cmd: cmd.trim().to_owned(),
            desc: normalize_desc(desc),
            tags: normalize_tags(tags)?,
            created_at: None,
        };
        alias.validate()?;
        Ok(alias)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_name(&self.name)?;
        validate_cmd(&self.cmd)?;

        if let Some(desc) = &self.desc {
            validate_single_line("description", desc)?;
        }

        let mut seen = HashSet::new();
        for tag in &self.tags {
            validate_tag(tag)?;
            if !seen.insert(tag) {
                return Err(ValidationError::DuplicateTag(tag.clone()));
            }
        }

        Ok(())
    }

    /// Returns the creation time used by list views, or `-` for legacy entries.
    pub fn created_at_display(&self) -> String {
        self.created_at
            .as_ref()
            .map(|created_at| created_at.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_owned())
    }
}

/// A partial update to an existing alias.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AliasPatch {
    /// `None` preserves the name; `Some(name)` renames the alias.
    pub name: Option<String>,
    pub cmd: Option<String>,
    /// `None` preserves the field; `Some(None)` removes it.
    pub desc: Option<Option<String>>,
    /// `None` preserves the field; `Some(vec![])` removes it.
    pub tags: Option<Vec<String>>,
}

impl AliasPatch {
    pub fn normalized(mut self) -> Result<Self, ValidationError> {
        if let Some(name) = &mut self.name {
            *name = name.trim().to_owned();
            validate_name(name)?;
        }
        if let Some(cmd) = &mut self.cmd {
            *cmd = cmd.trim().to_owned();
            validate_cmd(cmd)?;
        }

        if let Some(desc) = self.desc.take() {
            let desc = normalize_desc(desc);
            if let Some(value) = &desc {
                validate_single_line("description", value)?;
            }
            self.desc = Some(desc);
        }

        if let Some(tags) = self.tags.take() {
            self.tags = Some(normalize_tags(tags)?);
        }

        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.cmd.is_none() && self.desc.is_none() && self.tags.is_none()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    #[error("alias name cannot be empty")]
    EmptyName,
    #[error(
        "invalid alias name `{0}`; use letters, numbers, `_`, or `-`, and start with a letter or `_`"
    )]
    InvalidName(String),
    #[error("`als` is reserved for the alias manager")]
    ReservedName,
    #[error("`{0}` is a reserved shell keyword and cannot be used as a portable alias name")]
    ReservedKeyword(String),
    #[error("alias command cannot be empty")]
    EmptyCommand,
    #[error("{0} must fit on one line and contain no control characters")]
    MultipleLines(&'static str),
    #[error("tag cannot be empty")]
    EmptyTag,
    #[error("tag `{0}` must not contain commas or control characters")]
    InvalidTag(String),
    #[error("duplicate tag `{0}`")]
    DuplicateTag(String),
}

pub fn normalize_desc(desc: Option<String>) -> Option<String> {
    desc.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

pub fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>, ValidationError> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();

    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        validate_tag(tag)?;
        let tag = tag.to_owned();
        if seen.insert(tag.clone()) {
            normalized.push(tag);
        }
    }

    Ok(normalized)
}

fn validate_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::EmptyName);
    }
    if name == "als" {
        return Err(ValidationError::ReservedName);
    }
    const RESERVED_KEYWORDS: &[&str] = &[
        "begin",
        "case",
        "coproc",
        "do",
        "done",
        "elif",
        "else",
        "end",
        "esac",
        "fi",
        "for",
        "foreach",
        "function",
        "if",
        "in",
        "nocorrect",
        "repeat",
        "select",
        "switch",
        "then",
        "time",
        "until",
        "while",
    ];
    if RESERVED_KEYWORDS.contains(&name) {
        return Err(ValidationError::ReservedKeyword(name.to_owned()));
    }

    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(ValidationError::EmptyName);
    };
    let valid_first = first.is_ascii_alphabetic() || first == '_';
    let valid_rest = chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
    if !valid_first || !valid_rest {
        return Err(ValidationError::InvalidName(name.to_owned()));
    }

    Ok(())
}

fn validate_cmd(cmd: &str) -> Result<(), ValidationError> {
    if cmd.trim().is_empty() {
        return Err(ValidationError::EmptyCommand);
    }
    validate_single_line("command", cmd)
}

fn validate_tag(tag: &str) -> Result<(), ValidationError> {
    if tag.is_empty() {
        return Err(ValidationError::EmptyTag);
    }
    if tag.contains(',') || tag.chars().any(char::is_control) {
        return Err(ValidationError::InvalidTag(tag.to_owned()));
    }
    Ok(())
}

fn validate_single_line(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.chars().any(char::is_control) {
        return Err(ValidationError::MultipleLines(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_optional_values() {
        let alias = Alias::new(
            "gs",
            "git status",
            Some("  Git status  ".into()),
            vec![" git ".into(), "work".into(), "git".into()],
        )
        .unwrap();

        assert_eq!(alias.desc.as_deref(), Some("Git status"));
        assert_eq!(alias.tags, ["git", "work"]);
    }

    #[test]
    fn rejects_names_that_are_not_portable() {
        assert!(matches!(
            Alias::new("bad name", "echo hi", None, vec![]),
            Err(ValidationError::InvalidName(_))
        ));
        assert!(matches!(
            Alias::new("als", "echo hi", None, vec![]),
            Err(ValidationError::ReservedName)
        ));
        assert!(matches!(
            Alias::new("if", "echo hi", None, vec![]),
            Err(ValidationError::ReservedKeyword(_))
        ));
    }
}
