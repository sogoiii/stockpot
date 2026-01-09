//! Shell command execution.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use thiserror::Error;

/// Maximum characters in shell command output to protect context window
const SHELL_MAX_OUTPUT_CHARS: usize = 50_000;

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command not found: {0}")]
    NotFound(String),
    #[error("Command failed with exit code {0}")]
    ExitCode(i32),
    #[error("Timeout after {0} seconds")]
    Timeout(u64),
}

/// Result of running a command.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Truncate output to a maximum character limit, cutting at line boundaries.
fn truncate_output(output: String, max_chars: usize) -> (String, bool) {
    if output.len() <= max_chars {
        return (output, false);
    }

    let mut truncated: String = output.chars().take(max_chars).collect();

    // Try to cut at a newline boundary for cleaner output
    if let Some(last_newline) = truncated.rfind('\n') {
        truncated.truncate(last_newline);
    }

    truncated.push_str(&format!(
        "\n\n[OUTPUT TRUNCATED: {} chars exceeded {} char limit]",
        output.len(),
        max_chars
    ));

    (truncated, true)
}

/// Command runner with configuration.
pub struct CommandRunner {
    working_dir: Option<String>,
    timeout_secs: Option<u64>,
    env: Vec<(String, String)>,
}

impl CommandRunner {
    /// Create a new command runner.
    pub fn new() -> Self {
        Self {
            working_dir: None,
            timeout_secs: None,
            env: Vec::new(),
        }
    }

    /// Set working directory.
    pub fn working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set timeout.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Add environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Run a command.
    pub fn run(&self, command: &str) -> Result<CommandResult, ShellError> {
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        let shell_arg = if cfg!(windows) { "/C" } else { "-c" };

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg).arg(command);

        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let output = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;

        let exit_code = output.status.code().unwrap_or(-1);

        let (stdout, stdout_truncated) = truncate_output(
            String::from_utf8_lossy(&output.stdout).to_string(),
            SHELL_MAX_OUTPUT_CHARS,
        );
        let (stderr, stderr_truncated) = truncate_output(
            String::from_utf8_lossy(&output.stderr).to_string(),
            SHELL_MAX_OUTPUT_CHARS,
        );

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
            success: output.status.success(),
            stdout_truncated,
            stderr_truncated,
        })
    }

    /// Run a command with streaming output (callback for each line).
    pub fn run_streaming<F>(
        &self,
        command: &str,
        mut on_line: F,
    ) -> Result<CommandResult, ShellError>
    where
        F: FnMut(&str, bool), // (line, is_stderr)
    {
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        let shell_arg = if cfg!(windows) { "/C" } else { "-c" };

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg).arg(command);

        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let mut stdout_content = String::new();
        let mut stderr_content = String::new();

        // Read stdout
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                on_line(&line, false);
                stdout_content.push_str(&line);
                stdout_content.push('\n');
            }
        }

        // Read stderr
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                on_line(&line, true);
                stderr_content.push_str(&line);
                stderr_content.push('\n');
            }
        }

        let status = child.wait()?;
        let exit_code = status.code().unwrap_or(-1);

        let (stdout, stdout_truncated) = truncate_output(stdout_content, SHELL_MAX_OUTPUT_CHARS);
        let (stderr, stderr_truncated) = truncate_output(stderr_content, SHELL_MAX_OUTPUT_CHARS);

        Ok(CommandResult {
            stdout,
            stderr,
            exit_code,
            success: status.success(),
            stdout_truncated,
            stderr_truncated,
        })
    }
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to run a simple command.
pub fn run_command(command: &str) -> Result<CommandResult, ShellError> {
    CommandRunner::new().run(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_output_no_truncation() {
        let small = "hello world".to_string();
        let (result, truncated) = truncate_output(small.clone(), 1000);
        assert!(!truncated);
        assert_eq!(result, small);
    }

    #[test]
    fn test_truncate_output_with_truncation() {
        // Create output larger than limit
        let large = "x".repeat(60_000);
        let (result, truncated) = truncate_output(large, SHELL_MAX_OUTPUT_CHARS);
        assert!(truncated);
        assert!(result.len() < 60_000);
        assert!(result.contains("OUTPUT TRUNCATED"));
        assert!(result.contains("char limit"));
    }

    #[test]
    fn test_truncate_at_newline() {
        let content = "line1\nline2\nline3\nline4\nline5".to_string();
        // Set small limit that would cut in middle of line
        let (result, truncated) = truncate_output(content, 15);
        assert!(truncated);
        // Should cut at newline, not mid-line
        assert!(result.starts_with("line1\nline2"));
        assert!(result.contains("TRUNCATED"));
    }

    #[test]
    fn test_command_result_truncation_flags() {
        // This test verifies the CommandResult struct has truncation flags
        let result = CommandResult {
            stdout: "test".to_string(),
            stderr: "error".to_string(),
            exit_code: 0,
            success: true,
            stdout_truncated: true,
            stderr_truncated: false,
        };
        assert!(result.stdout_truncated);
        assert!(!result.stderr_truncated);
    }

    // =========================================================================
    // Additional CommandRunner Tests (from PR)
    // =========================================================================

    #[test]
    fn test_command_runner_new() {
        let runner = CommandRunner::new();
        assert!(runner.working_dir.is_none());
        assert!(runner.timeout_secs.is_none());
        assert!(runner.env.is_empty());
    }

    #[test]
    fn test_command_runner_default() {
        let runner = CommandRunner::default();
        assert!(runner.working_dir.is_none());
        assert!(runner.timeout_secs.is_none());
        assert!(runner.env.is_empty());
    }

    #[test]
    fn test_command_runner_builder_chain() {
        let runner = CommandRunner::new()
            .working_dir("/tmp")
            .timeout(30)
            .env("KEY", "value");

        assert_eq!(runner.working_dir, Some("/tmp".to_string()));
        assert_eq!(runner.timeout_secs, Some(30));
        assert_eq!(runner.env.len(), 1);
        assert_eq!(runner.env[0], ("KEY".to_string(), "value".to_string()));
    }

    #[test]
    fn test_run_simple_command() {
        let result = CommandRunner::new().run("echo hello").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_run_command_convenience_fn() {
        let result = run_command("echo convenience").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("convenience"));
    }

    #[test]
    fn test_shell_error_display() {
        let io_err = ShellError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(io_err.to_string().contains("IO error"));

        let not_found = ShellError::NotFound("cmd".to_string());
        assert!(not_found.to_string().contains("Command not found"));

        let exit_code = ShellError::ExitCode(42);
        assert!(exit_code.to_string().contains("42"));

        let timeout = ShellError::Timeout(60);
        assert!(timeout.to_string().contains("60"));
    }

    #[test]
    fn test_empty_command() {
        // Empty command should still execute (shell handles it)
        let result = CommandRunner::new().run("");
        // Empty command typically succeeds with no output
        assert!(result.is_ok());
    }

    #[test]
    fn test_command_result_clone() {
        let result = CommandResult {
            stdout: "output".to_string(),
            stderr: "error".to_string(),
            exit_code: 42,
            success: false,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        let cloned = result.clone();
        assert_eq!(result.stdout, cloned.stdout);
        assert_eq!(result.stderr, cloned.stderr);
        assert_eq!(result.exit_code, cloned.exit_code);
        assert_eq!(result.success, cloned.success);
    }
}
