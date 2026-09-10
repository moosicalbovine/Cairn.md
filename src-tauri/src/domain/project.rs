use std::path::{Component, Path};

use super::library::LibraryError;

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn validate_project_name(name: &str) -> Result<String, LibraryError> {
    validate_component(name, false)
}

pub fn validate_document_name(name: &str) -> Result<String, LibraryError> {
    let name = validate_component(name, true)?;
    let extension = Path::new(&name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("md") {
        return Err(LibraryError::invalid_path(
            "Managed documents must have a .md extension",
        ));
    }
    Ok(name)
}

fn validate_component(value: &str, allow_extension: bool) -> Result<String, LibraryError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.trim() != value
        || value.ends_with('.')
        || value.ends_with(' ')
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return Err(LibraryError::invalid_path("Invalid direct-child name"));
    }

    if Path::new(value).components().count() != 1
        || !matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(LibraryError::invalid_path(
            "Paths must name exactly one direct child",
        ));
    }

    let stem = if allow_extension {
        value.split('.').next().unwrap_or(value)
    } else {
        value
    };
    if WINDOWS_RESERVED
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return Err(LibraryError::invalid_path("Reserved Windows name"));
    }

    Ok(value.to_owned())
}

pub fn document_relative_path(project: &str, document: &str) -> String {
    format!("{project}/{document}")
}

pub fn path_key(relative_path: &str) -> String {
    relative_path.replace('\\', "/").to_lowercase()
}

pub fn validate_relative_path(
    relative_path: &str,
    expected_components: usize,
) -> Result<String, LibraryError> {
    if relative_path.is_empty()
        || relative_path.contains('\\')
        || relative_path.starts_with('/')
        || relative_path.as_bytes().get(1) == Some(&b':')
    {
        return Err(LibraryError::invalid_path(
            "Managed paths must be relative and use forward slashes",
        ));
    }

    let parts = relative_path.split('/').collect::<Vec<_>>();
    if parts.len() != expected_components
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        || Path::new(relative_path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LibraryError::invalid_path(
            "Managed path has an invalid component",
        ));
    }
    if parts.iter().any(|part| {
        part.trim() != *part
            || part.ends_with('.')
            || part.ends_with(' ')
            || part.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*')
            })
    }) {
        return Err(LibraryError::invalid_path(
            "Managed path contains an invalid Windows component",
        ));
    }
    Ok(relative_path.to_owned())
}
