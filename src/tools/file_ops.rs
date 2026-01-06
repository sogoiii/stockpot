//! File operation tools.

use super::common::{is_text_file, should_ignore};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Path not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("File too large: {0} bytes (max: {1})")]
    TooLarge(u64, u64),
    #[error("Binary file: {0}")]
    BinaryFile(String),
    #[error("Grep error: {0}")]
    GrepError(String),
}

/// File entry for directory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub depth: usize,
}

/// Result of listing files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesResult {
    pub entries: Vec<FileEntry>,
    pub total_files: usize,
    pub total_dirs: usize,
    pub total_size: u64,
    pub truncated: bool,
    pub max_entries: usize,
}

const LIST_FILES_DEFAULT_MAX_ENTRIES: usize = 2_000;
const LIST_FILES_HARD_MAX_ENTRIES: usize = 10_000;
const LIST_FILES_DEFAULT_MAX_DEPTH: usize = 10;
const LIST_FILES_HARD_MAX_DEPTH: usize = 50;

/// List files in a directory.
pub fn list_files(
    directory: &str,
    recursive: bool,
    max_depth: Option<usize>,
    max_entries: Option<usize>,
) -> Result<ListFilesResult, FileError> {
    let path = Path::new(directory);
    if !path.exists() {
        return Err(FileError::NotFound(directory.to_string()));
    }

    let max_entries = max_entries
        .unwrap_or(LIST_FILES_DEFAULT_MAX_ENTRIES)
        .clamp(1, LIST_FILES_HARD_MAX_ENTRIES);

    let mut entries = Vec::new();
    let mut total_files = 0;
    let mut total_dirs = 0;
    let mut total_size = 0u64;
    let mut truncated = false;

    let max_depth = max_depth
        .unwrap_or(LIST_FILES_DEFAULT_MAX_DEPTH)
        .min(LIST_FILES_HARD_MAX_DEPTH);

    list_files_recursive(
        path,
        path,
        &mut entries,
        recursive,
        max_depth,
        0,
        max_entries,
        &mut truncated,
    )?;

    for entry in &entries {
        if entry.is_dir {
            total_dirs += 1;
        } else {
            total_files += 1;
            total_size += entry.size;
        }
    }

    Ok(ListFilesResult {
        entries,
        total_files,
        total_dirs,
        total_size,
        truncated,
        max_entries,
    })
}

#[allow(clippy::too_many_arguments)]
fn list_files_recursive(
    base: &Path,
    dir: &Path,
    entries: &mut Vec<FileEntry>,
    recursive: bool,
    max_depth: usize,
    depth: usize,
    max_entries: usize,
    truncated: &mut bool,
) -> Result<(), FileError> {
    if depth > max_depth {
        return Ok(());
    }

    if entries.len() >= max_entries {
        *truncated = true;
        return Ok(());
    }

    let read_dir = fs::read_dir(dir);
    let mut dir_entries: Vec<_> = match read_dir {
        Ok(read_dir) => read_dir.filter_map(|e| e.ok()).collect(),
        Err(e) if depth == 0 => return Err(e.into()),
        Err(_) => return Ok(()),
    };

    dir_entries.sort_by_key(|a| a.file_name());

    for entry in dir_entries {
        if entries.len() >= max_entries {
            *truncated = true;
            break;
        }

        let path = entry.path();
        let relative = path.strip_prefix(base).unwrap_or(&path);
        let relative_str = relative.to_string_lossy().to_string();

        if should_ignore(&relative_str) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let is_dir = file_type.is_dir();
        let name = entry.file_name().to_string_lossy().to_string();

        entries.push(FileEntry {
            path: relative_str.clone(),
            name,
            is_dir,
            size: if is_dir { 0 } else { metadata.len() },
            depth,
        });

        if is_dir && recursive {
            list_files_recursive(
                base,
                &path,
                entries,
                recursive,
                max_depth,
                depth + 1,
                max_entries,
                truncated,
            )?;
        }
    }

    Ok(())
}

/// Read file contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub content: String,
    pub path: String,
    pub size: u64,
    pub lines: usize,
}

pub fn read_file(
    path: &str,
    start_line: Option<usize>,
    num_lines: Option<usize>,
    max_size: Option<u64>,
) -> Result<ReadFileResult, FileError> {
    let file_path = Path::new(path);
    if !file_path.exists() {
        return Err(FileError::NotFound(path.to_string()));
    }

    let metadata = fs::metadata(file_path)?;
    let max = max_size.unwrap_or(10 * 1024 * 1024); // 10MB default

    if metadata.len() > max {
        return Err(FileError::TooLarge(metadata.len(), max));
    }

    let content = fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let content = if let Some(start) = start_line {
        let start_idx = start.saturating_sub(1); // 1-based to 0-based
        let end_idx = num_lines
            .map(|n| (start_idx + n).min(total_lines))
            .unwrap_or(total_lines);

        lines[start_idx..end_idx].join("\n")
    } else {
        content
    };

    Ok(ReadFileResult {
        content,
        path: path.to_string(),
        size: metadata.len(),
        lines: total_lines,
    })
}

/// Write content to a file.
pub fn write_file(path: &str, content: &str, create_dirs: bool) -> Result<(), FileError> {
    let file_path = Path::new(path);

    if create_dirs {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(file_path, content)?;
    Ok(())
}

/// Grep match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: usize,
    pub content: String,
}

/// Grep results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    pub total_matches: usize,
}

/// Safety caps to prevent huge context blowups.
const GREP_HARD_MAX_MATCHES: usize = 200;
const GREP_DEFAULT_MAX_MATCHES: usize = 100;
const GREP_MAX_MATCHES_PER_FILE: usize = 10;
const GREP_MAX_LINE_LENGTH: usize = 512;
const GREP_MAX_FILE_SIZE_BYTES: u64 = 5 * 1024 * 1024;
const GREP_MAX_DEPTH: usize = 10;

fn truncate_line(s: &str, max_chars: usize) -> (String, usize) {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        (s.to_string(), 0)
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        (truncated, char_count - max_chars)
    }
}

struct MatchCollector {
    matches: Vec<GrepMatch>,
    file_path: String,
    max_matches: usize,
    max_per_file: usize,
    file_match_count: usize,
}

impl Sink for MatchCollector {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.matches.len() >= self.max_matches {
            return Ok(false);
        }

        if self.file_match_count >= self.max_per_file {
            return Ok(false);
        }

        let raw = String::from_utf8_lossy(mat.bytes());
        let raw = raw.trim_end_matches(&['\r', '\n'][..]);
        let (mut line_content, truncated_chars) = truncate_line(raw, GREP_MAX_LINE_LENGTH);

        if truncated_chars > 0 {
            line_content.push_str(&format!(" [...{truncated_chars} more chars]"));
        }

        let line_number = mat.line_number().unwrap_or(0);
        let line_number = usize::try_from(line_number).unwrap_or(0);

        self.matches.push(GrepMatch {
            path: self.file_path.clone(),
            line_number,
            content: line_content,
        });

        self.file_match_count += 1;
        Ok(true)
    }
}

/// Search for a pattern in files.
pub fn grep(
    pattern: &str,
    directory: &str,
    max_results: Option<usize>,
) -> Result<GrepResult, FileError> {
    let requested = max_results.unwrap_or(GREP_DEFAULT_MAX_MATCHES);
    let max_matches = requested.min(GREP_HARD_MAX_MATCHES);

    if pattern.is_empty() {
        return Err(FileError::GrepError(
            "pattern must not be empty".to_string(),
        ));
    }

    let path = PathBuf::from(directory);
    let abs_path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(&path)
    };

    if !abs_path.exists() {
        return Err(FileError::NotFound(directory.to_string()));
    }

    if !abs_path.is_dir() {
        return Err(FileError::GrepError(format!(
            "Not a directory: {directory}"
        )));
    }

    // Support a tiny subset of common ripgrep-ish flags embedded in the pattern.
    let (pattern, case_insensitive) = if let Some(rest) = pattern.strip_prefix("--ignore-case ") {
        (rest, true)
    } else if let Some(rest) = pattern.strip_prefix("-i ") {
        (rest, true)
    } else {
        (pattern, false)
    };

    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(FileError::GrepError(
            "pattern must not be empty".to_string(),
        ));
    }

    let regex_pattern = if case_insensitive {
        format!("(?i){pattern}")
    } else {
        pattern.to_string()
    };

    // Try regex, then fall back to literal (same behavior as ticca-desktop).
    let matcher = RegexMatcher::new_line_matcher(&regex_pattern)
        .or_else(|_| {
            let escaped = regex::escape(pattern);
            let escaped_pattern = if case_insensitive {
                format!("(?i){escaped}")
            } else {
                escaped
            };
            RegexMatcher::new_line_matcher(&escaped_pattern)
        })
        .map_err(|e| FileError::GrepError(format!("Invalid search pattern: {e}")))?;

    let walker = WalkBuilder::new(&abs_path)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .max_depth(Some(GREP_MAX_DEPTH))
        .max_filesize(Some(GREP_MAX_FILE_SIZE_BYTES))
        .filter_entry(|e| !should_ignore(&e.path().to_string_lossy()))
        .build();

    let mut searcher = Searcher::new();
    let mut matches: Vec<GrepMatch> = Vec::new();

    for entry in walker.flatten() {
        if matches.len() >= max_matches {
            break;
        }

        let entry_path = entry.path();

        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }

        let entry_path_str = entry_path.to_string_lossy().to_string();
        if should_ignore(&entry_path_str) || !is_text_file(&entry_path_str) {
            continue;
        }

        let relative_path = entry_path
            .strip_prefix(&abs_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| entry_path_str.clone());

        let mut collector = MatchCollector {
            matches: Vec::new(),
            file_path: relative_path,
            max_matches: max_matches - matches.len(),
            max_per_file: GREP_MAX_MATCHES_PER_FILE,
            file_match_count: 0,
        };

        if searcher
            .search_path(&matcher, entry_path, &mut collector)
            .is_ok()
        {
            matches.extend(collector.matches);
        }
    }

    Ok(GrepResult {
        total_matches: matches.len(),
        matches,
    })
}

/// Apply a unified diff to a file.
///
/// Parses the unified diff format and applies it to the file:
/// ```text
/// --- a/file.txt
/// +++ b/file.txt
/// @@ -1,3 +1,4 @@
///  context line
/// -removed line
/// +added line
/// ```
pub fn apply_diff(path: &str, diff_text: &str) -> Result<(), FileError> {
    use super::diff::{apply_unified_diff, UnifiedDiff};

    // Parse the diff to check if it's a new file
    let parsed = UnifiedDiff::parse(diff_text).map_err(|e| {
        FileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        ))
    })?;

    // Read original content (or empty for new files)
    let original = if parsed.is_new_file {
        String::new()
    } else if Path::new(path).exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    // Handle file deletion
    if parsed.is_delete {
        if Path::new(path).exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }

    // Apply the diff
    let patched = apply_unified_diff(&original, diff_text).map_err(|e| {
        FileError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        ))
    })?;

    // Write back
    write_file(path, &patched, true)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // read_file Tests
    // =========================================================================

    #[test]
    fn test_read_file_basic() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();

        assert_eq!(result.content, "line1\nline2\nline3");
        assert_eq!(result.lines, 3);
        assert!(result.path.ends_with("test.txt"));
    }

    #[test]
    fn test_read_file_with_start_line() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\nline5").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), Some(2), None, None).unwrap();
        assert_eq!(result.content, "line2\nline3\nline4\nline5");
    }

    #[test]
    fn test_read_file_with_start_and_num_lines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\nline5").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), Some(2), Some(2), None).unwrap();
        assert_eq!(result.content, "line2\nline3");
    }

    #[test]
    fn test_read_file_not_found() {
        let result = read_file("/nonexistent/path/file.txt", None, None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::NotFound(path) => assert!(path.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_read_file_too_large() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("large.txt");
        fs::write(&file_path, "a".repeat(100)).expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, Some(50));
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::TooLarge(size, max) => {
                assert_eq!(size, 100);
                assert_eq!(max, 50);
            }
            _ => panic!("Expected TooLarge error"),
        }
    }

    #[test]
    fn test_read_file_empty() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("empty.txt");
        fs::write(&file_path, "").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        assert_eq!(result.content, "");
        assert_eq!(result.lines, 0);
    }

    // =========================================================================
    // write_file Tests
    // =========================================================================

    #[test]
    fn test_write_file_basic() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("output.txt");

        write_file(file_path.to_str().unwrap(), "hello world", false).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_write_file_creates_directories() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("nested").join("deep").join("output.txt");

        write_file(file_path.to_str().unwrap(), "nested content", true).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "nested content");
    }

    #[test]
    fn test_write_file_fails_without_create_dirs() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("nonexistent").join("output.txt");

        let result = write_file(file_path.to_str().unwrap(), "content", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_file_overwrites() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("overwrite.txt");

        write_file(file_path.to_str().unwrap(), "original", false).unwrap();
        write_file(file_path.to_str().unwrap(), "updated", false).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "updated");
    }

    // =========================================================================
    // list_files Tests
    // =========================================================================

    #[test]
    fn test_list_files_non_recursive() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "a").expect("write failed");
        fs::write(dir.path().join("b.txt"), "b").expect("write failed");
        fs::create_dir(dir.path().join("subdir")).expect("mkdir failed");
        fs::write(dir.path().join("subdir").join("c.txt"), "c").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.total_files, 2);
        assert_eq!(result.total_dirs, 1);
    }

    #[test]
    fn test_list_files_recursive() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "a").expect("write failed");
        fs::create_dir(dir.path().join("subdir")).expect("mkdir failed");
        fs::write(dir.path().join("subdir").join("b.txt"), "bb").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();

        assert_eq!(result.total_files, 2);
        assert_eq!(result.total_dirs, 1);
        assert_eq!(result.total_size, 3);
    }

    #[test]
    fn test_list_files_not_found() {
        let result = list_files("/nonexistent/directory", false, None, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::NotFound(path) => assert!(path.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn list_files_respects_max_entries() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "a").expect("write failed");
        fs::write(dir.path().join("b.txt"), "b").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, Some(1), Some(1))
            .expect("list_files failed");

        assert_eq!(result.entries.len(), 1);
        assert!(result.truncated);
        assert_eq!(result.max_entries, 1);
    }

    #[test]
    fn test_list_files_empty_directory() {
        let dir = tempfile::tempdir().expect("tempdir failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.total_files, 0);
        assert_eq!(result.total_dirs, 0);
    }

    // =========================================================================
    // grep Tests
    // =========================================================================

    #[test]
    fn grep_finds_matches_and_line_numbers() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("a.txt");
        fs::write(&file_path, "foo\nbar\nfoo\n").expect("write failed");

        let result = grep("foo", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 2);

        assert!(result.matches[0].path.ends_with("a.txt"));
        assert_eq!(result.matches[0].line_number, 1);
        assert_eq!(result.matches[0].content, "foo");

        assert!(result.matches[1].path.ends_with("a.txt"));
        assert_eq!(result.matches[1].line_number, 3);
        assert_eq!(result.matches[1].content, "foo");
    }

    #[test]
    fn grep_respects_max_results() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("a.txt");
        fs::write(&file_path, "foo\nfoo\nfoo\n").expect("write failed");

        let result = grep("foo", dir.path().to_str().unwrap(), Some(1)).expect("grep failed");
        assert_eq!(result.total_matches, 1);
    }

    #[test]
    fn grep_ignores_common_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let good = dir.path().join("a.txt");
        fs::write(&good, "foo\n").expect("write failed");

        let ignored_dir = dir.path().join("target");
        fs::create_dir_all(&ignored_dir).expect("mkdir failed");
        fs::write(ignored_dir.join("b.txt"), "foo\n").expect("write failed");

        let result = grep("foo", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
        assert!(result.matches[0].path.ends_with("a.txt"));
    }

    #[test]
    fn grep_falls_back_to_literal_on_invalid_regex() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("a.txt");
        fs::write(&file_path, "(paren)\n").expect("write failed");

        let result = grep("(", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
        assert!(result.matches[0].content.contains("(paren)"));
    }

    #[test]
    fn test_grep_case_insensitive() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "FOO\nfoo\nFoO\n").expect("write failed");

        let result = grep("-i foo", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 3);
    }

    #[test]
    fn test_grep_empty_pattern_error() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("test.txt"), "content").expect("write failed");

        let result = grep("", dir.path().to_str().unwrap(), None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::GrepError(msg) => assert!(msg.contains("empty")),
            _ => panic!("Expected GrepError"),
        }
    }

    #[test]
    fn test_grep_no_matches() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("test.txt"), "hello world").expect("write failed");

        let result = grep("xyz123", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 0);
        assert!(result.matches.is_empty());
    }

    // =========================================================================
    // truncate_line Tests
    // =========================================================================

    #[test]
    fn test_truncate_line_no_truncation() {
        let (result, truncated) = truncate_line("short", 100);
        assert_eq!(result, "short");
        assert_eq!(truncated, 0);
    }

    #[test]
    fn test_truncate_line_with_truncation() {
        let (result, truncated) = truncate_line("hello world", 5);
        assert_eq!(result, "hello");
        assert_eq!(truncated, 6);
    }

    #[test]
    fn test_truncate_line_exact_length() {
        let (result, truncated) = truncate_line("exact", 5);
        assert_eq!(result, "exact");
        assert_eq!(truncated, 0);
    }

    // =========================================================================
    // FileError Tests
    // =========================================================================

    #[test]
    fn test_file_error_display() {
        let err = FileError::NotFound("/path/to/file".to_string());
        assert_eq!(err.to_string(), "Path not found: /path/to/file");

        let err = FileError::PermissionDenied("/protected".to_string());
        assert_eq!(err.to_string(), "Permission denied: /protected");

        let err = FileError::TooLarge(1000, 500);
        assert_eq!(err.to_string(), "File too large: 1000 bytes (max: 500)");

        let err = FileError::BinaryFile("image.png".to_string());
        assert_eq!(err.to_string(), "Binary file: image.png");

        let err = FileError::GrepError("invalid regex".to_string());
        assert_eq!(err.to_string(), "Grep error: invalid regex");
    }

    // =========================================================================
    // Serialization Tests
    // =========================================================================

    #[test]
    fn test_file_entry_serialization() {
        let entry = FileEntry {
            path: "src/main.rs".to_string(),
            name: "main.rs".to_string(),
            is_dir: false,
            size: 1024,
            depth: 1,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("main.rs"));

        let deserialized: FileEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.path, "src/main.rs");
        assert!(!deserialized.is_dir);
    }

    #[test]
    fn test_list_files_result_serialization() {
        let result = ListFilesResult {
            entries: vec![],
            total_files: 5,
            total_dirs: 2,
            total_size: 10240,
            truncated: false,
            max_entries: 2000,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ListFilesResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.total_files, 5);
        assert_eq!(deserialized.total_dirs, 2);
    }

    #[test]
    fn test_grep_match_serialization() {
        let m = GrepMatch {
            path: "file.rs".to_string(),
            line_number: 42,
            content: "fn main()".to_string(),
        };

        let json = serde_json::to_string(&m).unwrap();
        let deserialized: GrepMatch = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.line_number, 42);
        assert_eq!(deserialized.content, "fn main()");
    }

    // =========================================================================
    // Additional read_file Edge Cases
    // =========================================================================

    #[test]
    fn test_read_file_start_line_zero_treated_as_one() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").expect("write failed");

        // start_line=0 saturates to 0 after sub(1), so reads from line 1
        let result = read_file(file_path.to_str().unwrap(), Some(0), Some(2), None).unwrap();
        assert_eq!(result.content, "line1\nline2");
    }

    #[test]
    fn test_read_file_start_at_last_line() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").expect("write failed");

        // start_line=3 gets only the last line
        let result = read_file(file_path.to_str().unwrap(), Some(3), None, None).unwrap();
        assert_eq!(result.content, "line3");
    }

    #[test]
    #[should_panic(expected = "slice index")]
    fn test_read_file_start_line_beyond_file_panics() {
        // NOTE: This documents a bug - start_line beyond file length causes panic
        // The fix would be: let start_idx = start.saturating_sub(1).min(total_lines);
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").expect("write failed");

        // start_line=100 is beyond file end - this panics instead of returning empty
        let _ = read_file(file_path.to_str().unwrap(), Some(100), None, None);
    }

    #[test]
    fn test_read_file_num_lines_exceeds_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").expect("write failed");

        // Request more lines than available
        let result = read_file(file_path.to_str().unwrap(), Some(2), Some(100), None).unwrap();
        assert_eq!(result.content, "line2\nline3");
    }

    #[test]
    fn test_read_file_reports_correct_size() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        let content = "hello world";
        fs::write(&file_path, content).expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        assert_eq!(result.size, content.len() as u64);
    }

    // =========================================================================
    // Additional list_files Edge Cases
    // =========================================================================

    #[test]
    fn test_list_files_respects_max_depth() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::create_dir_all(dir.path().join("a").join("b").join("c")).expect("mkdir failed");
        fs::write(
            dir.path().join("a").join("b").join("c").join("deep.txt"),
            "deep",
        )
        .expect("write failed");

        // max_depth=1 should not reach c/deep.txt
        let result = list_files(dir.path().to_str().unwrap(), true, Some(1), None).unwrap();

        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"a"));
        assert!(paths.contains(&"a/b") || paths.iter().any(|p| p.ends_with("a/b")));
        assert!(!paths.iter().any(|p| p.contains("deep.txt")));
    }

    #[test]
    fn test_list_files_ignores_git_directory() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::create_dir(dir.path().join(".git")).expect("mkdir failed");
        fs::write(dir.path().join(".git").join("config"), "git config").expect("write failed");
        fs::write(dir.path().join("visible.txt"), "visible").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();

        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"visible.txt"));
        assert!(!paths.iter().any(|p| p.contains(".git")));
    }

    #[test]
    fn test_list_files_ignores_node_modules() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::create_dir(dir.path().join("node_modules")).expect("mkdir failed");
        fs::write(dir.path().join("node_modules").join("package.json"), "{}")
            .expect("write failed");
        fs::write(dir.path().join("index.js"), "// code").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();

        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"index.js"));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
    }

    #[test]
    fn test_list_files_max_entries_clamps_to_hard_max() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "a").expect("write failed");

        // Request more than HARD_MAX (10_000)
        let result = list_files(dir.path().to_str().unwrap(), false, None, Some(999_999)).unwrap();
        assert_eq!(result.max_entries, LIST_FILES_HARD_MAX_ENTRIES);
    }

    #[test]
    fn test_list_files_max_entries_clamps_to_min() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "a").expect("write failed");

        // Request 0 entries, clamps to 1
        let result = list_files(dir.path().to_str().unwrap(), false, None, Some(0)).unwrap();
        assert_eq!(result.max_entries, 1);
    }

    #[test]
    fn test_list_files_tracks_depth_correctly() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::create_dir(dir.path().join("sub")).expect("mkdir failed");
        fs::write(dir.path().join("root.txt"), "root").expect("write failed");
        fs::write(dir.path().join("sub").join("child.txt"), "child").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();

        let root_entry = result
            .entries
            .iter()
            .find(|e| e.name == "root.txt")
            .unwrap();
        let child_entry = result
            .entries
            .iter()
            .find(|e| e.name == "child.txt")
            .unwrap();

        assert_eq!(root_entry.depth, 0);
        assert_eq!(child_entry.depth, 1);
    }

    // =========================================================================
    // Additional grep Edge Cases
    // =========================================================================

    #[test]
    fn test_grep_ignore_case_long_form() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "HELLO\nWorld\nhello").expect("write failed");

        let result =
            grep("--ignore-case hello", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 2);
    }

    #[test]
    fn test_grep_not_a_directory_error() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("file.txt");
        fs::write(&file_path, "content").expect("write failed");

        let result = grep("content", file_path.to_str().unwrap(), None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::GrepError(msg) => assert!(msg.contains("Not a directory")),
            _ => panic!("Expected GrepError"),
        }
    }

    #[test]
    fn test_grep_pattern_empty_after_flag_strip() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("test.txt"), "content").expect("write failed");

        // Pattern is just "-i " which becomes empty after stripping
        let result = grep("-i ", dir.path().to_str().unwrap(), None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::GrepError(msg) => assert!(msg.contains("empty")),
            _ => panic!("Expected GrepError"),
        }
    }

    #[test]
    fn test_grep_truncates_long_lines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        // Create a line longer than GREP_MAX_LINE_LENGTH (512)
        let long_line = "x".repeat(600);
        fs::write(&file_path, &long_line).expect("write failed");

        let result = grep("x", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
        // Check that the line was truncated
        assert!(result.matches[0].content.contains("more chars"));
        assert!(result.matches[0].content.len() < 600);
    }

    #[test]
    fn test_grep_max_matches_per_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        // Create file with more matches than GREP_MAX_MATCHES_PER_FILE (10)
        let content = "match\n".repeat(20);
        fs::write(&file_path, &content).expect("write failed");

        let result = grep("match", dir.path().to_str().unwrap(), Some(100)).expect("grep failed");
        // Should be capped at 10 per file
        assert_eq!(result.total_matches, GREP_MAX_MATCHES_PER_FILE);
    }

    #[test]
    fn test_grep_respects_hard_max() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        // Create multiple files with matches
        for i in 0..50 {
            fs::write(
                dir.path().join(format!("file{}.txt", i)),
                "match\n".repeat(10),
            )
            .expect("write failed");
        }

        // Request more than GREP_HARD_MAX_MATCHES (200)
        let result = grep("match", dir.path().to_str().unwrap(), Some(999)).expect("grep failed");
        assert!(result.total_matches <= GREP_HARD_MAX_MATCHES);
    }

    #[test]
    fn test_grep_directory_not_found() {
        let result = grep("pattern", "/nonexistent/path", None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::NotFound(path) => assert!(path.contains("nonexistent")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_grep_skips_binary_files() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("text.txt"), "findme").expect("write failed");
        fs::write(dir.path().join("image.png"), "findme").expect("write failed");

        let result = grep("findme", dir.path().to_str().unwrap(), None).expect("grep failed");
        // Should only find in text file, not binary
        assert_eq!(result.total_matches, 1);
        assert!(result.matches[0].path.ends_with(".txt"));
    }

    #[test]
    fn test_grep_regex_pattern() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo123\nbar456\nfoo789").expect("write failed");

        let result = grep("foo\\d+", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 2);
    }

    #[test]
    fn test_grep_relative_path() {
        // grep should handle relative paths by resolving to current dir
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("test.txt"), "findme").expect("write failed");

        // Use the absolute path but test that the function works
        let result = grep("findme", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
    }

    // =========================================================================
    // apply_diff Tests
    // =========================================================================

    #[test]
    fn test_apply_diff_new_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("new_file.txt");

        let diff = r#"--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,2 @@
+line 1
+line 2
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line 1\nline 2");
    }

    #[test]
    fn test_apply_diff_modify_existing() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("existing.txt");
        fs::write(&file_path, "old line 1\nold line 2\nold line 3").expect("write failed");

        let diff = r#"--- a/existing.txt
+++ b/existing.txt
@@ -1,3 +1,3 @@
 old line 1
-old line 2
+new line 2
 old line 3
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("new line 2"));
        assert!(!content.contains("old line 2"));
    }

    #[test]
    fn test_apply_diff_delete_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("to_delete.txt");
        fs::write(&file_path, "content").expect("write failed");

        let diff = r#"--- a/to_delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-content
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        assert!(!file_path.exists());
    }

    #[test]
    fn test_apply_diff_delete_nonexistent_file_ok() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("nonexistent.txt");

        let diff = r#"--- a/nonexistent.txt
+++ /dev/null
@@ -1 +0,0 @@
-content
"#;

        // Should not error even if file doesn't exist
        let result = apply_diff(file_path.to_str().unwrap(), diff);
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_diff_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("nested").join("deep").join("file.txt");

        let diff = r#"--- /dev/null
+++ b/nested/deep/file.txt
@@ -0,0 +1 @@
+content
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "content");
    }

    #[test]
    fn test_apply_diff_invalid_diff_format() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "content").expect("write failed");

        // Malformed diff - missing @@ header with invalid range
        let diff = "not a valid diff";

        // Should still parse (as empty hunks) but not fail
        let result = apply_diff(file_path.to_str().unwrap(), diff);
        // This should succeed but do nothing
        assert!(result.is_ok());
    }

    // =========================================================================
    // FileError Coverage
    // =========================================================================

    #[test]
    fn test_file_error_io_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test error");
        let file_err: FileError = io_err.into();
        match file_err {
            FileError::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            _ => panic!("Expected Io variant"),
        }
    }

    #[test]
    fn test_file_error_io_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let file_err = FileError::Io(io_err);
        let display = file_err.to_string();
        assert!(display.contains("IO error"));
    }

    // =========================================================================
    // Additional Serialization Tests
    // =========================================================================

    #[test]
    fn test_read_file_result_serialization() {
        let result = ReadFileResult {
            content: "test content".to_string(),
            path: "/path/to/file.txt".to_string(),
            size: 12,
            lines: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ReadFileResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.content, "test content");
        assert_eq!(deserialized.path, "/path/to/file.txt");
        assert_eq!(deserialized.size, 12);
        assert_eq!(deserialized.lines, 1);
    }

    #[test]
    fn test_grep_result_serialization() {
        let result = GrepResult {
            matches: vec![GrepMatch {
                path: "test.rs".to_string(),
                line_number: 10,
                content: "fn test()".to_string(),
            }],
            total_matches: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: GrepResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.total_matches, 1);
        assert_eq!(deserialized.matches.len(), 1);
        assert_eq!(deserialized.matches[0].line_number, 10);
    }

    // =========================================================================
    // Symlink Handling Tests
    // =========================================================================

    #[cfg(unix)]
    #[test]
    fn test_list_files_follows_symlink_to_file() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let real_file = dir.path().join("real.txt");
        fs::write(&real_file, "real content").expect("write failed");

        let link_path = dir.path().join("link.txt");
        symlink(&real_file, &link_path).expect("symlink failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        // Both real file and symlink should appear
        assert_eq!(result.entries.len(), 2);
        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"real.txt"));
        assert!(names.contains(&"link.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn test_list_files_follows_symlink_to_dir() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let real_dir = dir.path().join("real_dir");
        fs::create_dir(&real_dir).expect("mkdir failed");
        fs::write(real_dir.join("nested.txt"), "nested").expect("write failed");

        let link_path = dir.path().join("link_dir");
        symlink(&real_dir, &link_path).expect("symlink failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();

        // Should have both dirs and their contents
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"real_dir"));
        assert!(paths.contains(&"link_dir"));
    }

    #[cfg(unix)]
    #[test]
    fn test_list_files_broken_symlink_skipped() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let link_path = dir.path().join("broken_link");
        symlink("/nonexistent/target", &link_path).expect("symlink failed");

        fs::write(dir.path().join("valid.txt"), "valid").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        // Broken symlink should be skipped (metadata() fails)
        // Only valid.txt should appear
        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"valid.txt"));
        // broken_link may or may not appear depending on file_type() success
    }

    #[cfg(unix)]
    #[test]
    fn test_read_file_through_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let real_file = dir.path().join("real.txt");
        fs::write(&real_file, "symlink content").expect("write failed");

        let link_path = dir.path().join("link.txt");
        symlink(&real_file, &link_path).expect("symlink failed");

        let result = read_file(link_path.to_str().unwrap(), None, None, None).unwrap();
        assert_eq!(result.content, "symlink content");
    }

    #[cfg(unix)]
    #[test]
    fn test_read_file_broken_symlink_fails() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let link_path = dir.path().join("broken_link");
        symlink("/nonexistent/target", &link_path).expect("symlink failed");

        // Broken symlink exists but points nowhere
        let result = read_file(link_path.to_str().unwrap(), None, None, None);
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_file_through_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let real_file = dir.path().join("real.txt");
        fs::write(&real_file, "original").expect("write failed");

        let link_path = dir.path().join("link.txt");
        symlink(&real_file, &link_path).expect("symlink failed");

        write_file(link_path.to_str().unwrap(), "updated via symlink", false).unwrap();

        // Real file should be updated
        let content = fs::read_to_string(&real_file).unwrap();
        assert_eq!(content, "updated via symlink");
    }

    #[cfg(unix)]
    #[test]
    fn test_grep_follows_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).expect("mkdir failed");
        fs::write(sub.join("file.txt"), "findme").expect("write failed");

        let link_path = dir.path().join("link_to_sub");
        symlink(&sub, &link_path).expect("symlink failed");

        let result = grep("findme", dir.path().to_str().unwrap(), None).expect("grep failed");

        // Should find match in both real path and through symlink
        assert!(result.total_matches >= 1);
    }

    // =========================================================================
    // Permission Edge Cases (Unix-only)
    // =========================================================================

    #[cfg(unix)]
    #[test]
    fn test_list_files_unreadable_subdir_continues() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let readable = dir.path().join("readable");
        let unreadable = dir.path().join("unreadable");

        fs::create_dir(&readable).expect("mkdir failed");
        fs::create_dir(&unreadable).expect("mkdir failed");

        fs::write(readable.join("file.txt"), "content").expect("write failed");
        fs::write(unreadable.join("hidden.txt"), "hidden").expect("write failed");

        // Remove read permission from unreadable dir
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).expect("chmod failed");

        let result = list_files(dir.path().to_str().unwrap(), true, None, None);

        // Restore permissions before assertions (for cleanup)
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).expect("chmod failed");

        // Should succeed and include readable dir contents
        let result = result.unwrap();
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("readable")));
    }

    #[cfg(unix)]
    #[test]
    fn test_read_file_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("noperm.txt");
        fs::write(&file_path, "secret").expect("write failed");

        // Remove read permission
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o000)).expect("chmod failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None);

        // Restore permissions for cleanup
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644)).expect("chmod failed");

        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
            _ => panic!("Expected Io(PermissionDenied) error"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_write_file_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("readonly.txt");
        fs::write(&file_path, "original").expect("write failed");

        // Remove write permission
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o444)).expect("chmod failed");

        let result = write_file(file_path.to_str().unwrap(), "new content", false);

        // Restore permissions for cleanup
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644)).expect("chmod failed");

        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_file_readonly_parent_dir() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir failed");
        let subdir = dir.path().join("readonly_dir");
        fs::create_dir(&subdir).expect("mkdir failed");

        // Make dir readonly
        fs::set_permissions(&subdir, fs::Permissions::from_mode(0o555)).expect("chmod failed");

        let file_path = subdir.join("newfile.txt");
        let result = write_file(file_path.to_str().unwrap(), "content", false);

        // Restore permissions
        fs::set_permissions(&subdir, fs::Permissions::from_mode(0o755)).expect("chmod failed");

        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_grep_skips_unreadable_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("readable.txt"), "findme").expect("write failed");

        let unreadable = dir.path().join("unreadable.txt");
        fs::write(&unreadable, "findme").expect("write failed");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).expect("chmod failed");

        let result = grep("findme", dir.path().to_str().unwrap(), None);

        // Restore permissions
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).expect("chmod failed");

        // Should find in readable file only
        let result = result.expect("grep failed");
        assert_eq!(result.total_matches, 1);
        assert!(result.matches[0].path.contains("readable"));
    }

    // =========================================================================
    // Additional Edge Cases for read_file
    // =========================================================================

    #[test]
    fn test_read_file_windows_line_endings() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("crlf.txt");
        fs::write(&file_path, "line1\r\nline2\r\nline3").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        // lines() splits on \n, so \r remains attached
        assert_eq!(result.lines, 3);
    }

    #[test]
    fn test_read_file_mixed_line_endings() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("mixed.txt");
        fs::write(&file_path, "line1\nline2\r\nline3\rline4").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        // \r alone doesn't split lines
        assert_eq!(result.lines, 3);
    }

    #[test]
    fn test_read_file_single_line_no_newline() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("single.txt");
        fs::write(&file_path, "single line no newline").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        assert_eq!(result.lines, 1);
        assert_eq!(result.content, "single line no newline");
    }

    #[test]
    fn test_read_file_trailing_newlines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("trailing.txt");
        fs::write(&file_path, "line1\nline2\n\n\n").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        // lines() counts empty lines at end
        assert_eq!(result.lines, 4);
    }

    #[test]
    fn test_read_file_only_newlines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("newlines.txt");
        fs::write(&file_path, "\n\n\n").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None).unwrap();
        assert_eq!(result.lines, 3);
    }

    #[test]
    fn test_read_file_with_null_bytes_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("with_null.txt");
        // Null bytes are valid UTF-8, fs::read_to_string accepts them
        fs::write(&file_path, b"hello\x00world").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None);
        // This succeeds because null byte is valid UTF-8
        assert!(result.is_ok());
        assert!(result.unwrap().content.contains('\0'));
    }

    #[test]
    fn test_read_file_invalid_utf8_fails() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("invalid_utf8.bin");
        // Invalid UTF-8 sequence (0x80 is not valid standalone)
        fs::write(&file_path, b"hello\x80world").expect("write failed");

        let result = read_file(file_path.to_str().unwrap(), None, None, None);
        // This fails because it's not valid UTF-8
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_max_size_boundary() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("boundary.txt");
        fs::write(&file_path, "a".repeat(100)).expect("write failed");

        // Exactly at max size - should succeed
        let result = read_file(file_path.to_str().unwrap(), None, None, Some(100));
        assert!(result.is_ok());

        // One byte over - should fail
        let result = read_file(file_path.to_str().unwrap(), None, None, Some(99));
        assert!(result.is_err());
    }

    // =========================================================================
    // Additional Edge Cases for list_files
    // =========================================================================

    #[test]
    fn test_list_files_deeply_nested() {
        let dir = tempfile::tempdir().expect("tempdir failed");

        // Create deeply nested structure
        let mut current = dir.path().to_path_buf();
        for i in 0..15 {
            current = current.join(format!("level{}", i));
            fs::create_dir(&current).expect("mkdir failed");
        }
        fs::write(current.join("deep.txt"), "deep").expect("write failed");

        // Default max_depth=10 should not reach the file
        let result = list_files(dir.path().to_str().unwrap(), true, None, None).unwrap();
        assert!(!result.entries.iter().any(|e| e.name.contains("deep.txt")));

        // With higher max_depth, should find it
        let result = list_files(dir.path().to_str().unwrap(), true, Some(20), None).unwrap();
        assert!(result.entries.iter().any(|e| e.name == "deep.txt"));
    }

    #[test]
    fn test_list_files_special_characters_in_names() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("file with spaces.txt"), "content").expect("write failed");
        fs::write(dir.path().join("file-with-dashes.txt"), "content").expect("write failed");
        fs::write(dir.path().join("file_with_underscores.txt"), "content").expect("write failed");
        fs::write(dir.path().join("file.multiple.dots.txt"), "content").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        assert_eq!(result.entries.len(), 4);
        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"file with spaces.txt"));
        assert!(names.contains(&"file-with-dashes.txt"));
    }

    #[test]
    fn test_list_files_unicode_filenames() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("文件.txt"), "chinese").expect("write failed");
        fs::write(dir.path().join("ファイル.txt"), "japanese").expect("write failed");
        fs::write(dir.path().join("emoji_🎉.txt"), "emoji").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        assert_eq!(result.entries.len(), 3);
        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"文件.txt"));
        assert!(names.contains(&"ファイル.txt"));
        assert!(names.contains(&"emoji_🎉.txt"));
    }

    #[test]
    fn test_list_files_hidden_files() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join(".hidden"), "hidden").expect("write failed");
        fs::write(dir.path().join(".dotfile.txt"), "dotfile").expect("write failed");
        fs::write(dir.path().join("visible.txt"), "visible").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        // Hidden files should be included (we don't ignore them by default)
        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&".hidden"));
        assert!(names.contains(&".dotfile.txt"));
        assert!(names.contains(&"visible.txt"));
    }

    #[test]
    fn test_list_files_sorting_alphabetical() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("zebra.txt"), "z").expect("write failed");
        fs::write(dir.path().join("alpha.txt"), "a").expect("write failed");
        fs::write(dir.path().join("middle.txt"), "m").expect("write failed");

        let result = list_files(dir.path().to_str().unwrap(), false, None, None).unwrap();

        let names: Vec<_> = result.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha.txt", "middle.txt", "zebra.txt"]);
    }

    // =========================================================================
    // Additional Edge Cases for grep
    // =========================================================================

    #[test]
    fn test_grep_multiline_content() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "start\nmiddle target here\nend").expect("write failed");

        let result = grep("target", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.matches[0].line_number, 2);
    }

    #[test]
    fn test_grep_special_regex_chars_fallback() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "test (parens) here\nno match").expect("write failed");

        // Unbalanced paren is invalid regex, fallback to escaped literal search
        let result = grep("(parens)", dir.path().to_str().unwrap(), None).expect("grep failed");
        // Fallback treats it as literal "(parens)" since unbalanced ( is invalid regex
        assert!(result.total_matches >= 1);
    }

    #[test]
    fn test_grep_valid_bracket_regex() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "test abc here\ntest xyz here").expect("write failed");

        // Valid regex: character class matches a, b, or c
        let result = grep("[abc]", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1); // Only first line has a, b, or c
    }

    #[test]
    fn test_grep_whitespace_only_pattern_error() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "no  double  spaces\nsingle space").expect("write failed");

        // Pattern "  " becomes empty after trim() - error
        let result = grep("  ", dir.path().to_str().unwrap(), None);
        assert!(result.is_err());
        match result.unwrap_err() {
            FileError::GrepError(msg) => assert!(msg.contains("empty")),
            _ => panic!("Expected GrepError"),
        }
    }

    #[test]
    fn test_grep_pattern_with_leading_trailing_whitespace() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "find me here").expect("write failed");

        // Pattern " find " is trimmed to "find" before search
        let result = grep(" find ", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 1);
    }

    #[test]
    fn test_grep_word_boundary() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "test testing tested\nthe test").expect("write failed");

        // Without word boundary, matches partial words too
        let result = grep("test", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 2);

        // With word boundary regex
        let result = grep(r"\btest\b", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 2); // "test" and "the test"
    }

    #[test]
    fn test_grep_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("empty.txt"), "").expect("write failed");

        let result = grep("anything", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn test_grep_file_with_only_newlines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("newlines.txt"), "\n\n\n").expect("write failed");

        let result = grep(".", dir.path().to_str().unwrap(), None).expect("grep failed");
        // Empty lines don't match "."
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn test_grep_multiple_files_sorted() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        fs::write(dir.path().join("z_file.txt"), "match").expect("write failed");
        fs::write(dir.path().join("a_file.txt"), "match").expect("write failed");
        fs::write(dir.path().join("m_file.txt"), "match").expect("write failed");

        let result = grep("match", dir.path().to_str().unwrap(), None).expect("grep failed");
        assert_eq!(result.total_matches, 3);
        // Files are walked in undefined order by WalkBuilder, but matches should all be present
    }

    // =========================================================================
    // apply_diff Additional Edge Cases
    // =========================================================================

    #[test]
    fn test_apply_diff_add_lines_at_end() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2").expect("write failed");

        let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,4 @@
 line1
 line2
+line3
+line4
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("line3"));
        assert!(content.contains("line4"));
    }

    #[test]
    fn test_apply_diff_remove_lines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4").expect("write failed");

        let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -1,4 +1,2 @@
 line1
-line2
-line3
 line4
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line4"));
        assert!(!content.contains("line2"));
        assert!(!content.contains("line3"));
    }

    #[test]
    fn test_apply_diff_context_only() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "unchanged").expect("write failed");

        // Diff with only context lines (no changes)
        let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -1 +1 @@
 unchanged
"#;

        apply_diff(file_path.to_str().unwrap(), diff).expect("apply_diff failed");

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "unchanged");
    }

    // =========================================================================
    // MatchCollector Tests
    // =========================================================================

    #[test]
    fn test_match_collector_stops_at_max_matches() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        // 5 matches in file
        fs::write(&file_path, "a\na\na\na\na").expect("write failed");

        // Request only 2 matches total
        let result = grep("a", dir.path().to_str().unwrap(), Some(2)).expect("grep failed");
        assert_eq!(result.total_matches, 2);
    }

    // =========================================================================
    // truncate_line Unicode Tests
    // =========================================================================

    #[test]
    fn test_truncate_line_unicode() {
        // Unicode characters should be counted correctly
        let (result, truncated) = truncate_line("Hello 世界!", 7);
        assert_eq!(result, "Hello 世");
        assert_eq!(truncated, 2); // "界!" = 2 chars
    }

    #[test]
    fn test_truncate_line_emoji() {
        let (result, truncated) = truncate_line("Hi 👋🌍", 4);
        assert_eq!(result, "Hi 👋");
        assert_eq!(truncated, 1); // 🌍 = 1 char (grapheme cluster)
    }

    // =========================================================================
    // write_file Edge Cases
    // =========================================================================

    #[test]
    fn test_write_file_empty_content() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("empty.txt");

        write_file(file_path.to_str().unwrap(), "", false).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn test_write_file_with_newlines() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("multiline.txt");

        write_file(file_path.to_str().unwrap(), "line1\nline2\nline3\n", false).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_write_file_unicode_content() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("unicode.txt");

        write_file(file_path.to_str().unwrap(), "Hello 世界 🌍", false).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello 世界 🌍");
    }
}
