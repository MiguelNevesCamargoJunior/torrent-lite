//! Lexical checks only; these do not protect against symlink or reparse races.
use crate::InputError;
use unicode_normalization::UnicodeNormalization;

pub fn validate_components(components: &[&str]) -> Result<(), InputError> {
    if components.is_empty() || components.len() > 32 {
        return Err(InputError::UnsafePath);
    }
    for name in components {
        if name.is_empty()
            || *name == "."
            || *name == ".."
            || name.ends_with(['.', ' '])
            || name.encode_utf16().count() > 255
            || name
                .chars()
                .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        {
            return Err(InputError::UnsafePath);
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if device {
            return Err(InputError::UnsafePath);
        }
    }
    Ok(())
}

pub fn validate_file_set(files: &[Vec<String>]) -> Result<(), InputError> {
    if files.len() > 10_000 {
        return Err(InputError::TooLarge);
    }
    if files.is_empty() {
        return Err(InputError::UnsafePath);
    }
    let mut byte_count = 0usize;
    for components in files {
        for name in components {
            byte_count = byte_count
                .checked_add(name.len())
                .ok_or(InputError::TooLarge)?;
            if byte_count > 8 * 1024 * 1024 {
                return Err(InputError::TooLarge);
            }
        }
    }
    let mut paths = Vec::with_capacity(files.len());
    for components in files {
        validate_components(&components.iter().map(String::as_str).collect::<Vec<_>>())?;
        let normalized: Vec<String> = components
            .iter()
            .map(|s| s.nfc().collect::<String>().to_uppercase().nfc().collect())
            .collect();
        paths.push(normalized);
    }
    // Component-vector ordering puts each parent immediately before its descendants.
    // Unlike storing every directory prefix, memory stays linear in input size.
    paths.sort_unstable();
    if paths.windows(2).any(|pair| pair[1].starts_with(&pair[0])) {
        return Err(InputError::PathCollision);
    }
    Ok(())
}
