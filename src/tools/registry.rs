//! Tool registry for Stockpot agents.
//!
//! This module provides the SpotToolRegistry which aggregates all available
//! serdesAI-compatible tool implementations.

use std::sync::Arc;

use serdes_ai_tools::Tool;

use super::agent_tools::{InvokeAgentTool, ListAgentsTool};
use super::delete_file_tool::DeleteFileTool;
use super::edit_file_tool::EditFileTool;
use super::grep_tool::GrepTool;
use super::list_files_tool::ListFilesTool;
use super::read_file_tool::ReadFileTool;
use super::reasoning_tool::ShareReasoningTool;
use super::shell_tool::RunShellCommandTool;

/// Arc-wrapped tool for shared ownership.
pub type ArcTool = Arc<dyn Tool + Send + Sync>;

// Re-export all tool types for convenience
pub use super::delete_file_tool::DeleteFileTool as DeleteFileToolType;
pub use super::edit_file_tool::EditFileTool as EditFileToolType;
pub use super::grep_tool::GrepTool as GrepToolType;
pub use super::list_files_tool::ListFilesTool as ListFilesToolType;
pub use super::read_file_tool::ReadFileTool as ReadFileToolType;
pub use super::reasoning_tool::ShareReasoningTool as ShareReasoningToolType;
pub use super::shell_tool::RunShellCommandTool as RunShellCommandToolType;

/// Registry holding all available Stockpot tools.
///
/// This provides a convenient way to create and access all tools
/// for use with serdesAI agents.
///
/// # Example
///
/// ```ignore
/// use stockpot::tools::registry::SpotToolRegistry;
///
/// let registry = SpotToolRegistry::new();
/// let tools = registry.all_tools();
/// ```
#[derive(Debug, Default)]
pub struct SpotToolRegistry {
    pub list_files: ListFilesTool,
    pub read_file: ReadFileTool,
    pub edit_file: EditFileTool,
    pub delete_file: DeleteFileTool,
    pub grep: GrepTool,
    pub run_shell_command: RunShellCommandTool,
    pub share_reasoning: ShareReasoningTool,
    pub invoke_agent: InvokeAgentTool,
    pub list_agents: ListAgentsTool,
}

impl SpotToolRegistry {
    /// Create a new registry with all tools.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all tools as Arc-wrapped trait objects for shared ownership.
    pub fn all_tools(&self) -> Vec<ArcTool> {
        vec![
            Arc::new(self.list_files.clone()),
            Arc::new(self.read_file.clone()),
            Arc::new(self.edit_file.clone()),
            Arc::new(self.delete_file.clone()),
            Arc::new(self.grep.clone()),
            Arc::new(self.run_shell_command.clone()),
            Arc::new(self.share_reasoning.clone()),
            Arc::new(self.invoke_agent.clone()),
            Arc::new(self.list_agents.clone()),
        ]
    }

    /// Get tool definitions for all tools.
    pub fn definitions(&self) -> Vec<serdes_ai_tools::ToolDefinition> {
        self.all_tools().iter().map(|t| t.definition()).collect()
    }

    /// Get a subset of tools by name.
    pub fn tools_by_name(&self, names: &[&str]) -> Vec<ArcTool> {
        let mut tools: Vec<ArcTool> = Vec::new();

        for name in names {
            match *name {
                "list_files" => tools.push(Arc::new(self.list_files.clone())),
                "read_file" => tools.push(Arc::new(self.read_file.clone())),
                "edit_file" => tools.push(Arc::new(self.edit_file.clone())),
                "delete_file" => tools.push(Arc::new(self.delete_file.clone())),
                "grep" => tools.push(Arc::new(self.grep.clone())),
                "run_shell_command" => tools.push(Arc::new(self.run_shell_command.clone())),
                "share_your_reasoning" => tools.push(Arc::new(self.share_reasoning.clone())),
                "invoke_agent" => tools.push(Arc::new(self.invoke_agent.clone())),
                "list_agents" => tools.push(Arc::new(self.list_agents.clone())),
                _ => {} // Unknown tool, skip
            }
        }

        tools
    }

    /// Get read-only tools (safe for reviewers and planning agents).
    pub fn read_only_tools(&self) -> Vec<ArcTool> {
        vec![
            Arc::new(self.list_files.clone()),
            Arc::new(self.read_file.clone()),
            Arc::new(self.grep.clone()),
            Arc::new(self.share_reasoning.clone()),
        ]
    }

    /// Get file operation tools only.
    pub fn file_tools(&self) -> Vec<ArcTool> {
        vec![
            Arc::new(self.list_files.clone()),
            Arc::new(self.read_file.clone()),
            Arc::new(self.edit_file.clone()),
            Arc::new(self.delete_file.clone()),
            Arc::new(self.grep.clone()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serdes_ai_tools::RunContext;

    // =========================================================================
    // SpotToolRegistry Tests
    // =========================================================================

    #[test]
    fn test_registry_creation() {
        let registry = SpotToolRegistry::new();
        assert_eq!(registry.all_tools().len(), 9);
        assert_eq!(registry.definitions().len(), 9);
    }

    #[test]
    fn test_registry_default() {
        let registry = SpotToolRegistry::default();
        assert_eq!(registry.all_tools().len(), 9);
    }

    #[test]
    fn test_registry_new_equals_default() {
        let new_reg = SpotToolRegistry::new();
        let default_reg = SpotToolRegistry::default();
        assert_eq!(new_reg.all_tools().len(), default_reg.all_tools().len());
    }

    #[test]
    fn test_tools_by_name() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["read_file", "edit_file"]);
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_tools_by_name_all_tools() {
        let registry = SpotToolRegistry::new();
        let all_names = [
            "list_files",
            "read_file",
            "edit_file",
            "delete_file",
            "grep",
            "run_shell_command",
            "share_your_reasoning",
            "invoke_agent",
            "list_agents",
        ];
        let tools = registry.tools_by_name(&all_names);
        assert_eq!(tools.len(), 9);
    }

    #[test]
    fn test_tools_by_name_unknown() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["nonexistent_tool"]);
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn test_tools_by_name_mixed() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["read_file", "nonexistent", "grep"]);
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_tools_by_name_empty() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&[]);
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn test_tools_by_name_case_sensitive() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["READ_FILE", "Edit_File"]);
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn test_read_only_tools() {
        let registry = SpotToolRegistry::new();
        let tools = registry.read_only_tools();
        assert_eq!(tools.len(), 4);
        for tool in &tools {
            let name = tool.definition().name;
            assert!(
                name == "list_files"
                    || name == "read_file"
                    || name == "grep"
                    || name == "share_your_reasoning",
                "Unexpected tool in read_only: {name}"
            );
        }
    }

    #[test]
    fn test_file_tools() {
        let registry = SpotToolRegistry::new();
        let tools = registry.file_tools();
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn test_tool_definitions_have_required_fields() {
        let registry = SpotToolRegistry::new();
        for tool in registry.all_tools() {
            let def = tool.definition();
            assert!(!def.name.is_empty(), "Tool name is empty");
            assert!(!def.description.is_empty(), "Tool description is empty");
        }
    }

    #[test]
    fn test_all_tools_unique_names() {
        let registry = SpotToolRegistry::new();
        let tools = registry.all_tools();
        let mut names: Vec<String> = tools
            .iter()
            .map(|t| t.definition().name.to_string())
            .collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "Tool names should be unique");
    }

    #[test]
    fn test_definitions_match_all_tools() {
        let registry = SpotToolRegistry::new();
        assert_eq!(registry.definitions().len(), registry.all_tools().len());
    }

    #[test]
    fn test_read_only_tools_are_subset_of_all() {
        let registry = SpotToolRegistry::new();
        let all_names: Vec<String> = registry
            .all_tools()
            .iter()
            .map(|t| t.definition().name.to_string())
            .collect();
        for tool in registry.read_only_tools() {
            let name = tool.definition().name.to_string();
            assert!(all_names.contains(&name), "{name} not in all_tools");
        }
    }

    #[test]
    fn test_file_tools_are_subset_of_all() {
        let registry = SpotToolRegistry::new();
        let all_names: Vec<String> = registry
            .all_tools()
            .iter()
            .map(|t| t.definition().name.to_string())
            .collect();
        for tool in registry.file_tools() {
            let name = tool.definition().name.to_string();
            assert!(all_names.contains(&name), "{name} not in all_tools");
        }
    }

    #[test]
    fn test_registry_tools_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SpotToolRegistry>();
    }

    // =========================================================================
    // Tool Execution Tests
    // =========================================================================

    #[tokio::test]
    async fn test_list_files_tool() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["list_files"]);
        assert_eq!(tools.len(), 1);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(&ctx, serde_json::json!({"directory": "."}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["read_file"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(&ctx, serde_json::json!({"file_path": "/nonexistent/file.txt"}))
            .await;
        assert!(result.is_ok());
        let ret = result.unwrap();
        assert!(ret.as_text().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_read_file_with_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").expect("write failed");

        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["read_file"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(&ctx, serde_json::json!({"file_path": file_path.to_str().unwrap()}))
            .await;
        assert!(result.is_ok());
        let text = result.unwrap().as_text().unwrap().to_string();
        assert!(text.contains("hello world"));
    }

    #[tokio::test]
    async fn test_edit_file_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("new_file.txt");

        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["edit_file"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(
                &ctx,
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "content": "new content"
                }),
            )
            .await;
        assert!(result.is_ok());
        assert!(file_path.exists());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "new content");
    }

    #[tokio::test]
    async fn test_delete_file_removes_file() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let file_path = dir.path().join("to_delete.txt");
        std::fs::write(&file_path, "delete me").expect("write failed");

        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["delete_file"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(&ctx, serde_json::json!({"file_path": file_path.to_str().unwrap()}))
            .await;
        assert!(result.is_ok());
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_grep_finds_pattern() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        std::fs::write(dir.path().join("test.txt"), "hello world\nfoo bar").expect("write failed");

        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["grep"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(
                &ctx,
                serde_json::json!({
                    "pattern": "hello",
                    "directory": dir.path().to_str().unwrap()
                }),
            )
            .await;
        assert!(result.is_ok());
        let text = result.unwrap().as_text().unwrap().to_string();
        assert!(text.contains("hello world"));
    }

    #[tokio::test]
    async fn test_shell_command_echo() {
        let registry = SpotToolRegistry::new();
        let tools = registry.tools_by_name(&["run_shell_command"]);

        let ctx = RunContext::minimal("test");
        let result = tools[0]
            .call(&ctx, serde_json::json!({"command": "echo hello"}))
            .await;
        assert!(result.is_ok());
        let text = result.unwrap().as_text().unwrap().to_string();
        assert!(text.contains("hello"));
    }

    #[tokio::test]
    async fn test_multiple_tools_parallel() {
        let registry = SpotToolRegistry::new();
        let dir = tempfile::tempdir().expect("tempdir failed");
        std::fs::write(dir.path().join("a.txt"), "content a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "content b").unwrap();

        let read_tools = registry.tools_by_name(&["read_file"]);
        let ctx = RunContext::minimal("test");

        let (res_a, res_b) = tokio::join!(
            read_tools[0].call(
                &ctx,
                serde_json::json!({"file_path": dir.path().join("a.txt").to_str().unwrap()})
            ),
            read_tools[0].call(
                &ctx,
                serde_json::json!({"file_path": dir.path().join("b.txt").to_str().unwrap()})
            )
        );

        assert!(res_a.is_ok());
        assert!(res_b.is_ok());
    }
}
