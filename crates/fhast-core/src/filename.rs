use std::path::{Component, Path, PathBuf};

use url::Url;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub struct OutputPaths {
    pub final_path: PathBuf,
    pub temp_path: PathBuf,
}

pub fn resolve_output_path(
    url: &str,
    output_path: Option<&Path>,
    working_dir: &Path,
) -> Result<OutputPaths, CoreError> {
    reject_parent_components(working_dir)?;

    let final_path = match output_path {
        Some(path) => {
            reject_parent_components(path)?;
            let candidate = if path.is_absolute() {
                path.to_path_buf()
            } else {
                working_dir.join(path)
            };

            if candidate.is_dir() {
                candidate.join(filename_from_url(url)?)
            } else {
                candidate
            }
        }
        None => working_dir.join(filename_from_url(url)?),
    };

    reject_parent_components(&final_path)?;

    if final_path.exists() {
        return Err(CoreError::OutputExists(final_path));
    }

    let temp_path = temp_path_for(&final_path)?;

    Ok(OutputPaths {
        final_path,
        temp_path,
    })
}

pub fn sanitize_filename(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    for character in input.chars() {
        if character.is_control()
            || matches!(
                character,
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        {
            output.push('_');
        } else {
            output.push(character);
        }
    }

    let output = output.trim_matches([' ', '.']).to_owned();
    let output = if output.is_empty() || output == "." || output == ".." {
        "download".to_owned()
    } else {
        output
    };

    truncate_filename(output, 255)
}

fn filename_from_url(value: &str) -> Result<String, CoreError> {
    let url = Url::parse(value).map_err(|error| CoreError::InvalidUrl(error.to_string()))?;
    let filename = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(sanitize_filename)
        .unwrap_or_else(|| "download".to_owned());

    Ok(filename)
}

fn temp_path_for(final_path: &Path) -> Result<PathBuf, CoreError> {
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::UnsafeOutputPath(final_path.display().to_string()))?;
    let temp_name = format!("{file_name}.tmp");

    Ok(final_path.with_file_name(temp_name))
}

fn reject_parent_components(path: &Path) -> Result<(), CoreError> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CoreError::UnsafeOutputPath(path.display().to_string()));
    }

    Ok(())
}

fn truncate_filename(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{resolve_output_path, sanitize_filename};

    #[test]
    fn sanitizes_unsafe_filename_characters() {
        assert_eq!(sanitize_filename("../bad:name?.zip"), "_bad_name_.zip");
        assert_eq!(sanitize_filename("..."), "download");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn rejects_parent_components_in_output_path() {
        let result = resolve_output_path(
            "https://example.com/file.zip",
            Some(Path::new("../file.zip")),
            Path::new("/tmp"),
        );

        assert!(result.is_err());
    }

    #[test]
    fn resolves_current_directory_default_output() {
        let output = resolve_output_path(
            "https://example.com/releases/file.zip?token=secret",
            None,
            Path::new("/tmp"),
        )
        .expect("output path should resolve");

        assert_eq!(output.final_path, Path::new("/tmp/file.zip"));
        assert_eq!(output.temp_path, Path::new("/tmp/file.zip.tmp"));
    }
}
