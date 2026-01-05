//! Interactive REPL implementation.
//!
//! This module provides the main REPL (Read-Eval-Print Loop) for interactive
//! sessions with the AI agent. It handles:
//!
//! - User input via reedline (readline alternative)
//! - Command dispatching (slash commands like /help, /model)
//! - Prompt handling (sending messages to the AI)
//! - Session management
//! - MCP server lifecycle

mod commands;
mod prompt;

use crate::agents::{AgentManager, UserMode};
use crate::cli::completion_reedline::{create_reedline, SpotCompleter, SpotPrompt};
use crate::config::Settings;
use crate::db::Database;
use crate::mcp::McpManager;
use crate::messaging::{Message, MessageBus, TerminalRenderer};
use crate::models::ModelRegistry;
use crate::session::SessionManager;
use crate::tools::SpotToolRegistry;
use reedline::{FileBackedHistory, Signal};
use serdes_ai_core::ModelRequest;
use tracing::debug;

use commands::CommandResult;

/// REPL state.
pub struct Repl<'a> {
    db: &'a Database,
    agents: AgentManager,
    renderer: TerminalRenderer,
    current_model: String,
    message_history: Vec<ModelRequest>,
    tool_registry: SpotToolRegistry,
    mcp_manager: McpManager,
    session_manager: SessionManager,
    current_session: Option<String>,
    /// Model configuration registry
    model_registry: ModelRegistry,
    /// Message bus for event-driven rendering
    message_bus: MessageBus,
}

impl<'a> Repl<'a> {
    /// Create a new REPL.
    pub fn new(db: &'a Database) -> Self {
        let settings = Settings::new(db);
        let current_model = settings.model();

        Self {
            db,
            agents: AgentManager::new(),
            renderer: TerminalRenderer::new(),
            current_model,
            message_history: Vec::new(),
            tool_registry: SpotToolRegistry::new(),
            mcp_manager: McpManager::new(),
            session_manager: SessionManager::new(),
            current_session: None,
            model_registry: ModelRegistry::load_from_db(db).unwrap_or_default(),
            message_bus: MessageBus::new(),
        }
    }

    /// Set the initial agent.
    pub fn with_agent(self, agent_name: &str) -> Self {
        if self.agents.exists(agent_name) {
            let _ = self.agents.switch(agent_name);
        }
        self
    }

    /// Set the initial model.
    pub fn with_model(mut self, model: &str) -> Self {
        self.current_model = model.to_string();
        self
    }

    /// Run the REPL loop.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Set up completer
        let mut completer = SpotCompleter::new();
        let user_mode = Settings::new(self.db).user_mode();
        completer.set_agents(
            self.agents
                .list_filtered(user_mode)
                .iter()
                .map(|a| a.name.clone())
                .collect(),
        );
        completer.set_sessions(
            self.session_manager
                .list()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.name)
                .collect(),
        );
        completer.set_mcp_servers(self.mcp_manager.config().servers.keys().cloned().collect());
        completer.set_models(self.model_registry.list_available(self.db));

        let mut line_editor = create_reedline(completer);
        // Load history
        let history_path = crate::config::XdgDirs::new().state.join("history.txt");
        if let Some(parent) = history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(h) = FileBackedHistory::with_file(500, history_path.clone()) {
            line_editor = line_editor.with_history(Box::new(h));
        }
        self.start_mcp_servers().await;

        loop {
            // Check for pinned model for current agent
            let agent_name = self.agents.current_name();
            let settings = Settings::new(self.db);
            let (effective_model, is_pinned) = match settings.get_agent_pinned_model(&agent_name) {
                Some(pinned) => (pinned, true),
                None => (self.current_model.clone(), false),
            };

            let prompt = SpotPrompt::with_pinned(
                self.agents
                    .current()
                    .map(|a| a.display_name())
                    .unwrap_or("Coding Agent"),
                &effective_model,
                is_pinned,
            );

            match line_editor.read_line(&prompt) {
                Ok(Signal::Success(line)) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    // Try to complete partial commands
                    let line = match super::completion_reedline::try_complete_input(
                        line,
                        &self.completion_context(),
                    ) {
                        Some(l) => l,
                        None => continue,
                    };

                    match self.handle_input(&line).await {
                        Ok(true) => {
                            self.stop_mcp_servers().await;
                            println!("👋 Stay simmering! 🍲");
                            break;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            let _ = self
                                .renderer
                                .render(&Message::error(format!("Error: {}", e)));
                        }
                    }
                }
                Ok(Signal::CtrlC) => {
                    println!("^C");
                    continue;
                }
                Ok(Signal::CtrlD) => {
                    self.stop_mcp_servers().await;
                    println!("👋 Stay simmering! 🍲");
                    break;
                }
                Err(err) => {
                    let _ = self
                        .renderer
                        .render(&Message::error(format!("Readline error: {}", err)));
                    self.stop_mcp_servers().await;
                    break;
                }
            }
        }

        Ok(())
    }

    /// Start all enabled MCP servers.
    async fn start_mcp_servers(&self) {
        let enabled_count = self.mcp_manager.config().enabled_servers().count();
        if enabled_count > 0 {
            println!(
                "\x1b[2m🔌 Starting {} MCP server(s)...\x1b[0m",
                enabled_count
            );
            if let Err(e) = self.mcp_manager.start_all().await {
                println!(
                    "\x1b[1;33m⚠️  Some MCP servers failed to start: {}\x1b[0m",
                    e
                );
            } else {
                let running = self.mcp_manager.running_servers().await;
                if !running.is_empty() {
                    println!(
                        "\x1b[2m✓ MCP servers running: {}\x1b[0m",
                        running.join(", ")
                    );
                }
            }
        }
    }

    /// Stop all running MCP servers.
    async fn stop_mcp_servers(&self) {
        let running = self.mcp_manager.running_servers().await;
        if !running.is_empty() {
            println!("\x1b[2m🔌 Stopping MCP servers...\x1b[0m");
            let _ = self.mcp_manager.stop_all().await;
        }
    }

    /// Get completion context for the current state.
    fn completion_context(&self) -> super::completion_reedline::CompletionContext {
        let user_mode = Settings::new(self.db).user_mode();
        super::completion_reedline::CompletionContext {
            models: self
                .model_registry
                .list()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            agents: self
                .agents
                .list_filtered(user_mode)
                .iter()
                .map(|a| a.name.clone())
                .collect(),
            sessions: self
                .session_manager
                .list()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.name)
                .collect(),
            mcp_servers: self.mcp_manager.config().servers.keys().cloned().collect(),
        }
    }

    /// Handle user input (either command or prompt).
    async fn handle_input(&mut self, input: &str) -> anyhow::Result<bool> {
        if input.starts_with('/') {
            return self.handle_command(input).await;
        }
        self.handle_prompt(input).await?;
        Ok(false)
    }

    /// Handle a slash command.
    async fn handle_command(&mut self, input: &str) -> anyhow::Result<bool> {
        let result = commands::handle_command(
            self.db,
            &mut self.agents,
            &mut self.model_registry,
            &self.message_bus,
            &mut self.message_history,
            &mut self.current_model,
            &mut self.current_session,
            &self.tool_registry,
            &self.mcp_manager,
            &self.session_manager,
            input,
        )
        .await?;

        match result {
            CommandResult::Exit => Ok(true),
            CommandResult::Continue => Ok(false),
        }
    }

    /// Handle a regular prompt (send to agent).
    pub async fn handle_prompt(&mut self, prompt: &str) -> anyhow::Result<()> {
        debug!(prompt_len = prompt.len(), "handle_prompt called");
        prompt::handle_prompt_with_bus(
            self.db,
            &self.agents,
            &self.model_registry,
            &self.message_bus,
            &mut self.message_history,
            &self.current_model,
            &self.tool_registry,
            &self.mcp_manager,
            &self.session_manager,
            &mut self.current_session,
            prompt,
        )
        .await
    }
}
