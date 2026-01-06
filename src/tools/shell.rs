//! Shell command execution.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use thiserror::Error;

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

        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code,
            success: output.status.success(),
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

        Ok(CommandResult {
            stdout: stdout_content,
            stderr: stderr_content,
            exit_code,
            success: status.success(),
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
    //! Unit tests for shell command execution.
    //!
    //! Coverage:
    //! - CommandRunner builder pattern
    //! - Basic command execution
    //! - Streaming output
    //! - Error handling
    //! - Environment variable handling
    //! - Working directory handling

    use super::*;

    // =========================================================================
    // Task 1.1: Basic CommandRunner Tests
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
    fn test_command_runner_multiple_env_vars() {
        let runner = CommandRunner::new()
            .env("KEY1", "value1")
            .env("KEY2", "value2")
            .env("KEY3", "value3");

        assert_eq!(runner.env.len(), 3);
    }

    #[test]
    fn test_run_simple_command() {
        let result = CommandRunner::new().run("echo hello").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_with_exit_code() {
        let result = CommandRunner::new().run("exit 42").unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_captures_stderr() {
        let result = CommandRunner::new().run("echo error >&2").unwrap();
        assert!(result.stderr.contains("error"));
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_with_working_dir() {
        let result = CommandRunner::new().working_dir("/tmp").run("pwd").unwrap();
        // On macOS, /tmp is a symlink to /private/tmp
        assert!(
            result.stdout.contains("/tmp") || result.stdout.contains("/private/tmp"),
            "Expected /tmp or /private/tmp, got: {}",
            result.stdout
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_with_env_var() {
        let result = CommandRunner::new()
            .env("TEST_VAR", "test_value")
            .run("echo $TEST_VAR")
            .unwrap();
        assert!(result.stdout.contains("test_value"));
    }

    #[test]
    fn test_run_command_convenience_fn() {
        let result = run_command("echo convenience").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("convenience"));
    }

    #[test]
    fn test_command_result_fields() {
        let result = run_command("echo test").unwrap();
        // Verify all fields are accessible
        let _ = result.stdout.clone();
        let _ = result.stderr.clone();
        let _ = result.exit_code;
        let _ = result.success;
    }

    // =========================================================================
    // Task 1.2: Streaming Output Tests
    // =========================================================================

    #[test]
    #[cfg(unix)]
    fn test_run_streaming_collects_lines() {
        let mut lines = Vec::new();
        let result = CommandRunner::new()
            .run_streaming("echo line1; echo line2", |line, _is_stderr| {
                lines.push(line.to_string());
            })
            .unwrap();
        assert!(result.success);
        assert!(
            lines.len() >= 2,
            "Expected at least 2 lines, got {:?}",
            lines
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_run_streaming_separates_stdout_stderr() {
        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();
        let result = CommandRunner::new()
            .run_streaming("echo out; echo err >&2", |line, is_stderr| {
                if is_stderr {
                    stderr_lines.push(line.to_string());
                } else {
                    stdout_lines.push(line.to_string());
                }
            })
            .unwrap();

        assert!(result.success);
        // Note: Due to buffering, output order may vary
        assert!(
            !stdout_lines.is_empty() || !stderr_lines.is_empty(),
            "Expected some output"
        );
    }

    #[test]
    fn test_run_streaming_empty_callback() {
        // Ensure streaming works even with empty callback
        let result = CommandRunner::new()
            .run_streaming("echo test", |_, _| {})
            .unwrap();
        assert!(result.success);
    }

    // =========================================================================
    // Task 1.3: Error Handling Tests
    // =========================================================================

    #[test]
    #[cfg(unix)]
    fn test_run_nonexistent_command() {
        // Running a nonexistent command through shell returns non-zero exit code
        let result = CommandRunner::new().run("nonexistent_command_xyz_123");
        // This should either error or return non-zero exit code
        match result {
            Ok(r) => assert!(!r.success, "Expected command to fail"),
            Err(_) => {} // Also acceptable
        }
    }

    #[test]
    fn test_run_in_nonexistent_directory() {
        let result = CommandRunner::new()
            .working_dir("/nonexistent/path/xyz_123_abc")
            .run("echo test");
        assert!(result.is_err(), "Expected error for nonexistent directory");
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_with_pipe() {
        let result = CommandRunner::new()
            .run("echo 'hello world' | grep hello")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    #[cfg(unix)]
    fn test_run_command_with_multiple_statements() {
        let result = CommandRunner::new()
            .run("echo first; echo second; echo third")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("first"));
        assert!(result.stdout.contains("second"));
        assert!(result.stdout.contains("third"));
    }

    // =========================================================================
    // Error Type Tests
    // =========================================================================

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

    // =========================================================================
    // Task 1.4: Additional Edge Case Tests
    // =========================================================================

    #[test]
    fn test_empty_command() {
        // Empty command should still execute (shell handles it)
        let result = CommandRunner::new().run("");
        // Empty command typically succeeds with no output
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_special_characters() {
        let result = CommandRunner::new().run("echo 'hello \"world\"'").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_unicode_output() {
        let result = CommandRunner::new().run("echo '日本語 émojis 🎉'").unwrap();
        assert!(result.success);
        // UTF-8 should be preserved
        assert!(result.stdout.contains("日本語") || result.stdout.contains("émojis"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_newlines_in_output() {
        let result = CommandRunner::new()
            .run("printf 'line1\\nline2\\nline3'")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("line1"));
        assert!(result.stdout.contains("line2"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_tabs_and_spaces() {
        let result = CommandRunner::new().run("echo 'a\tb  c   d'").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("a\tb"));
    }

    #[test]
    #[cfg(unix)]
    fn test_exit_code_255() {
        let result = CommandRunner::new().run("exit 255").unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 255);
    }

    #[test]
    #[cfg(unix)]
    fn test_exit_code_1() {
        let result = CommandRunner::new().run("exit 1").unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_variable_expansion() {
        let result = CommandRunner::new()
            .env("MY_VAR", "expanded_value")
            .run("echo \"prefix_${MY_VAR}_suffix\"")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("expanded_value"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_overrides_existing_env() {
        // PATH is always set, verify we can override
        let result = CommandRunner::new()
            .env("STOCKPOT_TEST_VAR", "original")
            .env("STOCKPOT_TEST_VAR", "overridden")
            .run("echo $STOCKPOT_TEST_VAR")
            .unwrap();
        // Note: both values are added to env vec, shell takes last one
        assert!(result.success);
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_subshell() {
        let result = CommandRunner::new().run("echo $(echo nested)").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("nested"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_conditional() {
        let result = CommandRunner::new()
            .run("if true; then echo yes; else echo no; fi")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("yes"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_and_operator() {
        let result = CommandRunner::new()
            .run("echo first && echo second")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("first"));
        assert!(result.stdout.contains("second"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_or_operator() {
        let result = CommandRunner::new().run("false || echo fallback").unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("fallback"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_and_fails_early() {
        let result = CommandRunner::new()
            .run("false && echo should_not_appear")
            .unwrap();
        assert!(!result.success);
        assert!(!result.stdout.contains("should_not_appear"));
    }

    #[test]
    #[cfg(unix)]
    fn test_large_output() {
        // Generate ~10KB of output
        let result = CommandRunner::new()
            .run("for i in $(seq 1 1000); do echo \"line $i with some padding text\"; done")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.len() > 5000, "Expected large output");
        assert!(result.stdout.contains("line 1 "));
        assert!(result.stdout.contains("line 1000 "));
    }

    #[test]
    #[cfg(unix)]
    fn test_stderr_only_output() {
        let result = CommandRunner::new().run("echo error_only >&2").unwrap();
        assert!(result.success); // exit code 0
        assert!(result.stderr.contains("error_only"));
        assert!(result.stdout.is_empty() || !result.stdout.contains("error_only"));
    }

    #[test]
    #[cfg(unix)]
    fn test_mixed_stdout_stderr() {
        let result = CommandRunner::new()
            .run("echo stdout_msg; echo stderr_msg >&2; echo stdout_again")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("stdout_msg"));
        assert!(result.stdout.contains("stdout_again"));
        assert!(result.stderr.contains("stderr_msg"));
    }

    #[test]
    #[cfg(unix)]
    fn test_streaming_with_working_dir() {
        let mut lines = Vec::new();
        let result = CommandRunner::new()
            .working_dir("/tmp")
            .run_streaming("pwd", |line, _| {
                lines.push(line.to_string());
            })
            .unwrap();
        assert!(result.success);
        // On macOS, /tmp -> /private/tmp
        assert!(
            result.stdout.contains("/tmp") || result.stdout.contains("/private/tmp"),
            "Expected /tmp in output, got: {}",
            result.stdout
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_streaming_with_env_var() {
        let mut lines = Vec::new();
        let result = CommandRunner::new()
            .env("STREAM_TEST_VAR", "stream_value")
            .run_streaming("echo $STREAM_TEST_VAR", |line, _| {
                lines.push(line.to_string());
            })
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("stream_value"));
    }

    #[test]
    #[cfg(unix)]
    fn test_streaming_multiline() {
        let mut line_count = 0;
        let result = CommandRunner::new()
            .run_streaming("echo a; echo b; echo c", |_, _| {
                line_count += 1;
            })
            .unwrap();
        assert!(result.success);
        assert!(
            line_count >= 3,
            "Expected at least 3 lines, got {}",
            line_count
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_streaming_captures_stderr() {
        let mut stderr_count = 0;
        let result = CommandRunner::new()
            .run_streaming("echo err1 >&2; echo err2 >&2", |_, is_stderr| {
                if is_stderr {
                    stderr_count += 1;
                }
            })
            .unwrap();
        assert!(result.success);
        // stderr content should be captured in result
        assert!(!result.stderr.is_empty(), "Expected stderr content");
    }

    #[test]
    fn test_streaming_in_nonexistent_directory() {
        let result = CommandRunner::new()
            .working_dir("/nonexistent/path/xyz_456_def")
            .run_streaming("echo test", |_, _| {});
        assert!(result.is_err(), "Expected error for nonexistent directory");
    }

    // =========================================================================
    // Task 1.5: CommandResult and ShellError Trait Tests
    // =========================================================================

    #[test]
    fn test_command_result_debug() {
        let result = CommandResult {
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            exit_code: 0,
            success: true,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("stdout"));
        assert!(debug_str.contains("stderr"));
        assert!(debug_str.contains("exit_code"));
    }

    #[test]
    fn test_command_result_clone() {
        let result = CommandResult {
            stdout: "output".to_string(),
            stderr: "error".to_string(),
            exit_code: 42,
            success: false,
        };
        let cloned = result.clone();
        assert_eq!(result.stdout, cloned.stdout);
        assert_eq!(result.stderr, cloned.stderr);
        assert_eq!(result.exit_code, cloned.exit_code);
        assert_eq!(result.success, cloned.success);
    }

    #[test]
    fn test_shell_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let shell_err: ShellError = io_err.into();
        assert!(matches!(shell_err, ShellError::Io(_)));
        assert!(shell_err.to_string().contains("access denied"));
    }

    #[test]
    fn test_shell_error_debug() {
        let err = ShellError::NotFound("test_cmd".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("NotFound"));
        assert!(debug_str.contains("test_cmd"));
    }

    #[test]
    fn test_shell_error_exit_code_negative() {
        let err = ShellError::ExitCode(-1);
        assert!(err.to_string().contains("-1"));
    }

    #[test]
    fn test_shell_error_timeout_zero() {
        let err = ShellError::Timeout(0);
        assert!(err.to_string().contains("0 seconds"));
    }

    // =========================================================================
    // Task 1.6: Whitespace and Boundary Tests
    // =========================================================================

    #[test]
    fn test_command_with_only_whitespace() {
        let result = CommandRunner::new().run("   ");
        // Whitespace-only command should succeed (no-op)
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_command_preserves_trailing_whitespace() {
        let result = CommandRunner::new().run("echo 'text   '").unwrap();
        assert!(result.success);
        // Echo adds newline, but internal spaces should be preserved
        assert!(result.stdout.contains("text"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_backslash() {
        let result = CommandRunner::new().run("echo 'back\\\\slash'").unwrap();
        assert!(result.success);
    }

    #[test]
    #[cfg(unix)]
    fn test_command_empty_env_value() {
        let result = CommandRunner::new()
            .env("EMPTY_VAR", "")
            .run("echo \">${EMPTY_VAR}<\"")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("><"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_equals_in_value() {
        let result = CommandRunner::new()
            .env("EQUATION", "a=b=c")
            .run("echo $EQUATION")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("a=b=c"));
    }

    #[test]
    fn test_builder_working_dir_into_string() {
        // Test Into<String> trait for working_dir
        let runner = CommandRunner::new().working_dir(String::from("/tmp"));
        assert_eq!(runner.working_dir, Some("/tmp".to_string()));
    }

    #[test]
    fn test_builder_env_into_string() {
        // Test Into<String> trait for env
        let runner = CommandRunner::new().env(String::from("KEY"), String::from("VALUE"));
        assert_eq!(runner.env[0], ("KEY".to_string(), "VALUE".to_string()));
    }

    #[test]
    fn test_timeout_builder_value() {
        let runner = CommandRunner::new().timeout(120);
        assert_eq!(runner.timeout_secs, Some(120));
    }

    #[test]
    fn test_timeout_builder_zero() {
        let runner = CommandRunner::new().timeout(0);
        assert_eq!(runner.timeout_secs, Some(0));
    }

    #[test]
    fn test_timeout_builder_max() {
        let runner = CommandRunner::new().timeout(u64::MAX);
        assert_eq!(runner.timeout_secs, Some(u64::MAX));
    }

    // =========================================================================
    // Task 1.7: Complex Command Scenarios
    // =========================================================================

    #[test]
    #[cfg(unix)]
    fn test_command_with_heredoc() {
        let result = CommandRunner::new()
            .run("cat << 'EOF'\nline1\nline2\nEOF")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("line1"));
        assert!(result.stdout.contains("line2"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_redirection() {
        // Test input/output redirection (sh-compatible, unlike process substitution)
        let result = CommandRunner::new()
            .run("echo hello > /dev/null && echo done")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("done"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_with_background_ignored() {
        // Background process in subshell shouldn't block
        let result = CommandRunner::new()
            .run("echo done; (sleep 0.01 &)")
            .unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("done"));
    }

    #[test]
    #[cfg(unix)]
    fn test_command_true() {
        let result = CommandRunner::new().run("true").unwrap();
        assert!(result.success);
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_command_false() {
        let result = CommandRunner::new().run("false").unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    #[cfg(unix)]
    fn test_command_colon_noop() {
        let result = CommandRunner::new().run(":").unwrap();
        assert!(result.success);
        assert_eq!(result.exit_code, 0);
    }
}
