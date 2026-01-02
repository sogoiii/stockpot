//! Stockpot - The main assistant agent.

use crate::agents::{AgentCapabilities, SpotAgent};

/// Stockpot - Your AI coding companion 🍲
pub struct StockpotAgent;

impl SpotAgent for StockpotAgent {
    fn name(&self) -> &str {
        "stockpot"
    }

    fn display_name(&self) -> &str {
        "Coding Agent"
    }

    fn description(&self) -> &str {
        "Your AI coding companion - helps with all coding tasks"
    }

    fn system_prompt(&self) -> String {
        include_str!("prompts/stockpot.md").to_string()
    }

    fn available_tools(&self) -> Vec<&str> {
        vec![
            "list_files",
            "read_file",
            "edit_file",
            "delete_file",
            "grep",
            "run_shell_command",
            "share_your_reasoning",
            "invoke_agent",
            "list_agents",
        ]
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::full()
    }
}
