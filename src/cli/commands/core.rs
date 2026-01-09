//! Core REPL command handlers.
//!
//! This module contains implementations for basic REPL commands
//! that don't require complex state management.

use crate::agents::{AgentInfo, AgentVisibility};
use crate::config::Settings;
use crate::db::Database;
use crate::models::{ModelConfig, ModelRegistry, ModelSettings};
use crate::tokens::format_tokens_with_separator;

/// Handle the /cd command - change or show working directory.
pub fn cmd_cd(args: &str) {
    if args.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            println!("📁 {}", cwd.display());
        }
    } else {
        let expanded = shellexpand::tilde(args);
        match std::env::set_current_dir(expanded.as_ref()) {
            Ok(()) => {
                if let Ok(cwd) = std::env::current_dir() {
                    println!("📁 Changed to: {}", cwd.display());
                }
            }
            Err(e) => println!("❌ Failed to change directory: {}", e),
        }
    }
}

/// Handle the /show command - display current status.
pub fn cmd_show(
    db: &Database,
    agent_name: &str,
    agent_display_name: &str,
    current_model: &str,
    current_session: Option<&str>,
    message_count: usize,
    registry: &ModelRegistry,
) {
    let settings = Settings::new(db);

    println!("\n\x1b[1m📊 Current Status\x1b[0m\n");
    println!("  Agent:       \x1b[36m{}\x1b[0m", agent_display_name);

    // Check for pinned model
    if let Some(pinned) = settings.get_agent_pinned_model(agent_name) {
        println!(
            "  Model:       \x1b[33m{}\x1b[0m \x1b[2m(pinned)\x1b[0m",
            pinned
        );
    } else {
        println!("  Model:       \x1b[33m{}\x1b[0m", current_model);
    }
    println!(
        "  YOLO mode:   {}",
        if settings.yolo_mode() {
            "\x1b[31mON\x1b[0m"
        } else {
            "\x1b[32moff\x1b[0m"
        }
    );
    println!("  Session:     {}", current_session.unwrap_or("(unsaved)"));
    println!("  Messages:    {}", message_count);

    // Model info from registry
    if let Some(config) = registry.get(current_model) {
        println!("  Context:     {} tokens", config.context_length);

        // Rough token estimate
        let est_tokens = message_count * 500;
        println!(
            "  Usage:       ~{}/{}",
            format_tokens_with_separator(est_tokens),
            format_tokens_with_separator(config.context_length)
        );
    }

    // Model-specific settings
    let model_settings = ModelSettings::load(db, current_model).unwrap_or_default();
    if !model_settings.is_empty() {
        println!("\n  \x1b[2mModel settings:\x1b[0m");
        if let Some(t) = model_settings.temperature {
            println!("    temperature: {:.2}", t);
        }
        if let Some(ref r) = model_settings.reasoning_effort {
            println!("    reasoning:   {}", r);
        }
        if let Some(e) = model_settings.extended_thinking {
            println!(
                "    thinking:    {}",
                if e { "enabled" } else { "disabled" }
            );
        }
    }

    println!();
}

/// Handle the /version command.
pub fn cmd_version() {
    println!("🍲 stockpot v{}", env!("CARGO_PKG_VERSION"));
}

/// Handle the /agents command - list available agents.
pub fn cmd_agents(agents: &[AgentInfo], current_name: &str, show_visibility: bool) {
    println!("\n📋 \x1b[1mAvailable Agents:\x1b[0m\n");
    for agent in agents {
        let marker = if agent.name == current_name {
            "→ "
        } else {
            "  "
        };
        let visibility_badge = if show_visibility {
            let label = match agent.visibility {
                AgentVisibility::Main => "Main",
                AgentVisibility::Sub => "Sub",
                AgentVisibility::Hidden => "Hidden",
            };
            format!(" \x1b[2m[{}]\x1b[0m", label)
        } else {
            String::new()
        };

        println!(
            "{}\x1b[1;36m{}\x1b[0m{}",
            marker, agent.display_name, visibility_badge
        );
        println!("    Name: {}", agent.name);
        println!("    {}\n", agent.description);
    }
}

/// Handle the /new command.
pub fn cmd_new() {
    println!("🆕 Started new conversation");
}

/// Handle the /tools command - list available tools.
pub fn cmd_tools() {
    println!("\n\x1b[1m🔧 Available Tools\x1b[0m\n");
    println!("  \x1b[36mlist_files\x1b[0m       List directory contents");
    println!("  \x1b[36mread_file\x1b[0m        Read file contents");
    println!("  \x1b[36medit_file\x1b[0m        Create or modify files");
    println!("  \x1b[36mgrep\x1b[0m             Search text in files");
    println!("  \x1b[36mshell\x1b[0m            Execute shell commands");
    println!("  \x1b[36minvoke_agent\x1b[0m     Call sub-agent");
    println!("  \x1b[36mlist_agents\x1b[0m      List available agents\n");
}

/// Handle the /reasoning command - set reasoning effort level.
pub fn cmd_reasoning(db: &Database, args: &str) {
    let valid = ["minimal", "low", "medium", "high", "xhigh"];
    if args.is_empty() || !valid.contains(&args) {
        let settings = Settings::new(db);
        let current = settings.get_or("reasoning_effort", "medium");
        println!("Current reasoning effort: \x1b[33m{}\x1b[0m", current);
        println!("Usage: /reasoning <{}>", valid.join("|"));
    } else {
        let settings = Settings::new(db);
        let _ = settings.set("reasoning_effort", args);
        println!("✅ Reasoning effort set to: \x1b[33m{}\x1b[0m", args);
    }
}

/// Handle the /verbosity command - set verbosity level.
pub fn cmd_verbosity(db: &Database, args: &str) {
    let valid = ["low", "medium", "high"];
    if args.is_empty() || !valid.contains(&args) {
        let settings = Settings::new(db);
        let current = settings.get_or("verbosity", "medium");
        println!("Current verbosity: \x1b[33m{}\x1b[0m", current);
        println!("Usage: /verbosity <{}>", valid.join("|"));
    } else {
        let settings = Settings::new(db);
        let _ = settings.set("verbosity", args);
        println!("✅ Verbosity set to: \x1b[33m{}\x1b[0m", args);
    }
}

/// Display the help message.
pub fn show_help() {
    println!(
        "
\x1b[1m🍲 Stockpot Commands\x1b[0m

  \x1b[1;36m/help, /h, /?\x1b[0m          Show this help message
  \x1b[1;36m/exit, /quit, /q\x1b[0m       Exit stockpot
  \x1b[1;36m/clear, /cls\x1b[0m           Clear the screen
  \x1b[1;36m/new\x1b[0m                   Start a new conversation
  \x1b[1;36m/cd [path]\x1b[0m             Show or change working directory

\x1b[1mAgents & Models:\x1b[0m
  \x1b[1;33m/model [name]\x1b[0m          Show/set model (interactive picker if no name)
  \x1b[1;33m/models\x1b[0m                List available models
  \x1b[1;33m/agent [name]\x1b[0m          Show/switch agent (interactive picker if no name)
  \x1b[1;33m/agents\x1b[0m                List all available agents
  \x1b[1;33m/pin <model>\x1b[0m           Pin a model to the current agent
  \x1b[1;33m/pin <agent> <model>\x1b[0m   Pin a model to a specific agent
  \x1b[1;33m/unpin [agent]\x1b[0m         Remove model pin (current or specific agent)
  \x1b[1;33m/pins\x1b[0m                  List all agent model pins

\x1b[1mModel Settings:\x1b[0m
  \x1b[1;32m/ms, /model_settings\x1b[0m   Edit model settings (interactive)
  \x1b[1;32m/ms --show\x1b[0m             Show current model settings
  \x1b[1;32m/reasoning <level>\x1b[0m     Set reasoning effort (minimal/low/medium/high/xhigh)
  \x1b[1;32m/verbosity <level>\x1b[0m     Set verbosity (low/medium/high)

\x1b[1mSessions & Context:\x1b[0m
  \x1b[1;35m/save [name]\x1b[0m           Save current session
  \x1b[1;35m/load [name]\x1b[0m           Load a session (picker if no name)
  \x1b[1;35m/resume\x1b[0m                Interactive session loader
  \x1b[1;35m/sessions\x1b[0m              List saved sessions
  \x1b[1;35m/session, /s\x1b[0m           Show current session info
  \x1b[1;35m/context\x1b[0m               Show context usage (with visual bar)
  \x1b[1;35m/compact [n]\x1b[0m           Compact message history (keep first + last N)
  \x1b[1;35m/truncate [n]\x1b[0m          Keep only last N messages (default: 10)
  \x1b[1;35m/delete-session <n>\x1b[0m    Delete a saved session

\x1b[1mConfiguration:\x1b[0m
  \x1b[1;32m/show\x1b[0m                  Show current status (agent, model, context)
  \x1b[1;32m/set [key=value]\x1b[0m       Show or set configuration
  \x1b[1;32m/yolo\x1b[0m                  Toggle YOLO mode (auto-approve commands)
  \x1b[1;32m/tools\x1b[0m                 List available tools

\x1b[1mMCP:\x1b[0m
  \x1b[1;34m/mcp\x1b[0m                   MCP server management (try /mcp help)
  \x1b[1;34m/mcp list\x1b[0m              List configured servers
  \x1b[1;34m/mcp status\x1b[0m            Show running servers
  \x1b[1;34m/mcp add\x1b[0m               Add new server (interactive wizard)
  \x1b[1;34m/mcp start [name]\x1b[0m      Start a server
  \x1b[1;34m/mcp stop [name]\x1b[0m       Stop a server

\x1b[1mAuth:\x1b[0m
  \x1b[1;34m/chatgpt-auth\x1b[0m          Authenticate with ChatGPT OAuth
  \x1b[1;34m/claude-code-auth\x1b[0m      Authenticate with Claude Code OAuth

\x1b[1mModel Discovery:\x1b[0m
  \x1b[1;34m/add-model\x1b[0m             Browse and add models from bundled models.conf
  \x1b[1;34m/extra-models\x1b[0m          List configured extra models

  \x1b[1;34m/version, /v\x1b[0m           Show version

\x1b[2mJust type normally to chat with the current agent!\x1b[0m
\x1b[2mTab completion available for commands, models, and agents.\x1b[0m
\x1b[2mJust type normally to chat with the current agent!\x1b[0m
\x1b[2mTab completion available for commands, models, and agents.\x1b[0m
"
    );
}

/// Display the list of available models from the registry.
pub fn show_models(db: &Database, registry: &ModelRegistry, current_model: &str) {
    use crate::models::ModelType;
    use std::collections::HashMap;

    // Get all available models (ones with valid API keys/OAuth)
    let available = registry.list_available(db);

    if available.is_empty() {
        println!("\n\x1b[2mNo available models found.\x1b[0m");
        println!("\x1b[2mUse /add-model to add models from bundled models.conf (edit + rebuild to change it)\x1b[0m");
        println!("\x1b[2mOr set API keys: OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.\x1b[0m\n");
        return;
    }

    // Group models by provider type
    let mut by_type: HashMap<String, Vec<(&str, Option<&str>)>> = HashMap::new();

    for name in &available {
        if let Some(config) = registry.get(name) {
            let type_label = match config.model_type {
                ModelType::Openai => "OpenAI",
                ModelType::Anthropic => "Anthropic",
                ModelType::Gemini => "Google Gemini",
                ModelType::ClaudeCode => "Claude Code (OAuth)",
                ModelType::ChatgptOauth => "ChatGPT (OAuth)",
                ModelType::AzureOpenai => "Azure OpenAI",
                ModelType::Openrouter => "OpenRouter",
                ModelType::CustomOpenai | ModelType::CustomAnthropic => {
                    // For custom endpoints, try to extract provider from name
                    if let Some(idx) = name.find(':') {
                        let provider = &name[..idx];
                        // Capitalize first letter
                        let mut chars = provider.chars();
                        match chars.next() {
                            Some(c) => {
                                let capitalized: String = c.to_uppercase().chain(chars).collect();
                                // Leak is fine here - static lifetime for display
                                Box::leak(capitalized.into_boxed_str()) as &str
                            }
                            None => "Custom",
                        }
                    } else {
                        "Custom"
                    }
                }
                ModelType::RoundRobin => "Round Robin",
            };

            by_type
                .entry(type_label.to_string())
                .or_default()
                .push((name.as_str(), config.description.as_deref()));
        }
    }

    println!("\n\x1b[1m📦 Available Models:\x1b[0m\n");

    // Sort provider types for consistent display
    let mut types: Vec<_> = by_type.keys().cloned().collect();
    types.sort();

    for type_label in types {
        if let Some(models) = by_type.get(&type_label) {
            println!("\x1b[1m{}:\x1b[0m", type_label);
            for (name, desc) in models {
                let marker = if *name == current_model { "→" } else { " " };
                let desc_str = desc.unwrap_or("");
                if desc_str.is_empty() {
                    println!("  {} \x1b[36m{}\x1b[0m", marker, name);
                } else {
                    println!("  {} \x1b[36m{}\x1b[0m", marker, name);
                    println!("      \x1b[2m{}\x1b[0m", desc_str);
                }
            }
            println!();
        }
    }

    println!("\x1b[2mCurrent: {}\x1b[0m", current_model);
    println!("\x1b[2mUse /model <name> to switch\x1b[0m\n");
}

// =========================================================================
// Utility functions for testability
// =========================================================================

/// Parse and validate reasoning effort level.
/// Returns None if invalid, Some(level) if valid.
pub fn validate_reasoning_effort(level: &str) -> Option<&'static str> {
    match level {
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        _ => None,
    }
}

/// Parse and validate verbosity level.
/// Returns None if invalid, Some(level) if valid.
pub fn validate_verbosity(level: &str) -> Option<&'static str> {
    match level {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentInfo, AgentVisibility};
    use tempfile::TempDir;

    // =========================================================================
    // Helper: Create test database
    // =========================================================================

    fn create_test_db() -> (TempDir, Database) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();
        (tmp, db)
    }

    // =========================================================================
    // cmd_cd Tests
    // =========================================================================

    #[test]
    fn test_cmd_cd_no_args_prints_current_dir() {
        // Just verify it doesn't panic when called with empty args
        cmd_cd("");
    }

    #[test]
    fn test_cmd_cd_tilde_expansion() {
        // Test that tilde expansion is performed
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let original = std::env::current_dir().unwrap();

        // Change to home via tilde
        cmd_cd("~");

        // Verify we're in home (or close to it - symlinks may differ)
        let current = std::env::current_dir().unwrap();
        assert!(
            current.starts_with(&home) || current.to_string_lossy().contains("home"),
            "Expected to be in home dir after cd ~"
        );

        // Restore original
        std::env::set_current_dir(original).ok();
    }

    #[test]
    fn test_cmd_cd_invalid_path() {
        // Should print error but not panic
        cmd_cd("/nonexistent/path/that/does/not/exist/12345");
    }

    // =========================================================================
    // cmd_version Tests
    // =========================================================================

    #[test]
    fn test_cmd_version_no_panic() {
        // Just verify it runs without panic
        cmd_version();
    }

    // =========================================================================
    // cmd_agents Tests
    // =========================================================================

    #[test]
    fn test_cmd_agents_empty_list() {
        let agents: Vec<AgentInfo> = vec![];
        cmd_agents(&agents, "stockpot", false);
    }

    #[test]
    fn test_cmd_agents_with_agents() {
        let agents = vec![
            AgentInfo {
                name: "stockpot".to_string(),
                display_name: "Stockpot".to_string(),
                description: "Main agent".to_string(),
                visibility: AgentVisibility::Main,
            },
            AgentInfo {
                name: "planning".to_string(),
                display_name: "Planning".to_string(),
                description: "Planning agent".to_string(),
                visibility: AgentVisibility::Main,
            },
        ];

        // Should not panic
        cmd_agents(&agents, "stockpot", false);
    }

    #[test]
    fn test_cmd_agents_with_visibility_badges() {
        let agents = vec![
            AgentInfo {
                name: "main".to_string(),
                display_name: "Main Agent".to_string(),
                description: "A main agent".to_string(),
                visibility: AgentVisibility::Main,
            },
            AgentInfo {
                name: "sub".to_string(),
                display_name: "Sub Agent".to_string(),
                description: "A sub agent".to_string(),
                visibility: AgentVisibility::Sub,
            },
            AgentInfo {
                name: "hidden".to_string(),
                display_name: "Hidden Agent".to_string(),
                description: "A hidden agent".to_string(),
                visibility: AgentVisibility::Hidden,
            },
        ];

        // With visibility badges
        cmd_agents(&agents, "main", true);
    }

    #[test]
    fn test_cmd_agents_current_marker() {
        let agents = vec![
            AgentInfo {
                name: "agent1".to_string(),
                display_name: "Agent 1".to_string(),
                description: "First".to_string(),
                visibility: AgentVisibility::Main,
            },
            AgentInfo {
                name: "agent2".to_string(),
                display_name: "Agent 2".to_string(),
                description: "Second".to_string(),
                visibility: AgentVisibility::Main,
            },
        ];

        // Marker should show for agent2
        cmd_agents(&agents, "agent2", false);
    }

    // =========================================================================
    // cmd_new Tests
    // =========================================================================

    #[test]
    fn test_cmd_new_no_panic() {
        cmd_new();
    }

    // =========================================================================
    // cmd_tools Tests
    // =========================================================================

    #[test]
    fn test_cmd_tools_no_panic() {
        cmd_tools();
    }

    // =========================================================================
    // show_help Tests
    // =========================================================================

    #[test]
    fn test_show_help_no_panic() {
        show_help();
    }

    // =========================================================================
    // validate_reasoning_effort Tests
    // =========================================================================

    #[test]
    fn test_validate_reasoning_effort_valid() {
        assert_eq!(validate_reasoning_effort("minimal"), Some("minimal"));
        assert_eq!(validate_reasoning_effort("low"), Some("low"));
        assert_eq!(validate_reasoning_effort("medium"), Some("medium"));
        assert_eq!(validate_reasoning_effort("high"), Some("high"));
        assert_eq!(validate_reasoning_effort("xhigh"), Some("xhigh"));
    }

    #[test]
    fn test_validate_reasoning_effort_invalid() {
        assert_eq!(validate_reasoning_effort(""), None);
        assert_eq!(validate_reasoning_effort("invalid"), None);
        assert_eq!(validate_reasoning_effort("MEDIUM"), None);
        assert_eq!(validate_reasoning_effort("extreme"), None);
    }

    // =========================================================================
    // validate_verbosity Tests
    // =========================================================================

    #[test]
    fn test_validate_verbosity_valid() {
        assert_eq!(validate_verbosity("low"), Some("low"));
        assert_eq!(validate_verbosity("medium"), Some("medium"));
        assert_eq!(validate_verbosity("high"), Some("high"));
    }

    #[test]
    fn test_validate_verbosity_invalid() {
        assert_eq!(validate_verbosity(""), None);
        assert_eq!(validate_verbosity("invalid"), None);
        assert_eq!(validate_verbosity("LOW"), None);
        assert_eq!(validate_verbosity("minimal"), None);
    }

    // =========================================================================
    // cmd_reasoning Tests (with database)
    // =========================================================================

    #[test]
    fn test_cmd_reasoning_empty_args() {
        let (_tmp, db) = create_test_db();
        // Should show current value without panicking
        cmd_reasoning(&db, "");
    }

    #[test]
    fn test_cmd_reasoning_valid_level() {
        let (_tmp, db) = create_test_db();
        cmd_reasoning(&db, "high");

        // Verify it was set
        let settings = Settings::new(&db);
        assert_eq!(settings.get_or("reasoning_effort", "medium"), "high");
    }

    #[test]
    fn test_cmd_reasoning_invalid_level() {
        let (_tmp, db) = create_test_db();
        cmd_reasoning(&db, "invalid");
        // Should print usage, not set anything
    }

    // =========================================================================
    // cmd_verbosity Tests (with database)
    // =========================================================================

    #[test]
    fn test_cmd_verbosity_empty_args() {
        let (_tmp, db) = create_test_db();
        cmd_verbosity(&db, "");
    }

    #[test]
    fn test_cmd_verbosity_valid_level() {
        let (_tmp, db) = create_test_db();
        cmd_verbosity(&db, "high");

        let settings = Settings::new(&db);
        assert_eq!(settings.get_or("verbosity", "medium"), "high");
    }

    #[test]
    fn test_cmd_verbosity_invalid_level() {
        let (_tmp, db) = create_test_db();
        cmd_verbosity(&db, "extreme");
    }

    // =========================================================================
    // cmd_show Tests (with database)
    // =========================================================================

    #[test]
    fn test_cmd_show_basic() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        cmd_show(
            &db,
            "stockpot",
            "Stockpot",
            "gpt-4o",
            Some("test-session"),
            5,
            &registry,
        );
    }

    #[test]
    fn test_cmd_show_no_session() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        cmd_show(&db, "stockpot", "Stockpot", "gpt-4o", None, 0, &registry);
    }

    #[test]
    fn test_cmd_show_with_model_settings() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        // Set some model settings via direct database insert
        db.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)",
                ["model_settings.gpt-4o.temperature", "0.7"],
            )
            .ok();

        cmd_show(
            &db,
            "stockpot",
            "Stockpot",
            "gpt-4o",
            Some("test"),
            10,
            &registry,
        );
    }

    #[test]
    fn test_cmd_show_with_pinned_model() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();
        let settings = Settings::new(&db);

        // Pin a model to stockpot agent
        settings
            .set_agent_pinned_model("stockpot", "claude-3-opus")
            .unwrap();

        cmd_show(
            &db,
            "stockpot",
            "Stockpot",
            "gpt-4o", // current_model different from pinned
            Some("test-session"),
            5,
            &registry,
        );
    }

    #[test]
    fn test_cmd_show_yolo_mode_on() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();
        let settings = Settings::new(&db);

        // Enable yolo mode
        settings.set("yolo_mode", "true").ok();

        cmd_show(&db, "stockpot", "Stockpot", "gpt-4o", None, 3, &registry);
    }

    #[test]
    fn test_cmd_show_with_model_in_registry() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        // Add a model to registry with context length
        registry.add(ModelConfig {
            name: "test-model".to_string(),
            context_length: 128000,
            ..Default::default()
        });

        cmd_show(
            &db,
            "stockpot",
            "Stockpot",
            "test-model",
            Some("session-1"),
            20,
            &registry,
        );
    }

    #[test]
    fn test_cmd_show_with_reasoning_effort_setting() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        // Set reasoning effort via database
        db.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)",
                ["model_settings.gpt-4o.reasoning_effort", "high"],
            )
            .ok();

        cmd_show(&db, "stockpot", "Stockpot", "gpt-4o", None, 5, &registry);
    }

    #[test]
    fn test_cmd_show_with_extended_thinking() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        // Set extended thinking via database
        db.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)",
                ["model_settings.claude-3.extended_thinking", "true"],
            )
            .ok();

        cmd_show(
            &db,
            "stockpot",
            "Stockpot",
            "claude-3",
            Some("thinking-session"),
            10,
            &registry,
        );
    }

    #[test]
    fn test_cmd_show_extended_thinking_disabled() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        // Set extended thinking to false
        db.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?, ?)",
                ["model_settings.claude-3.extended_thinking", "false"],
            )
            .ok();

        cmd_show(&db, "stockpot", "Stockpot", "claude-3", None, 0, &registry);
    }

    // =========================================================================
    // cmd_cd Additional Tests
    // =========================================================================

    #[test]
    fn test_cmd_cd_valid_path() {
        let tmp = TempDir::new().unwrap();

        // Just verify cmd_cd doesn't panic on valid paths
        // Note: We can't reliably check current_dir() changed because
        // other tests running in parallel may change it
        cmd_cd(tmp.path().to_str().unwrap());

        // Verify the directory still exists
        assert!(tmp.path().exists());
    }

    #[test]
    fn test_cmd_cd_with_spaces_in_path() {
        let tmp = TempDir::new().unwrap();
        let spaced_dir = tmp.path().join("path with spaces");
        std::fs::create_dir(&spaced_dir).unwrap();

        // Just verify cmd_cd doesn't panic on paths with spaces
        // Note: We can't reliably check current_dir() changed because
        // other tests running in parallel may change it
        cmd_cd(spaced_dir.to_str().unwrap());

        // Verify the directory still exists (wasn't corrupted)
        assert!(spaced_dir.exists());
    }

    // =========================================================================
    // show_models Tests
    // =========================================================================

    #[test]
    fn test_show_models_empty_registry() {
        let (_tmp, db) = create_test_db();
        let registry = ModelRegistry::default();

        // Should not panic, prints "no models" message
        show_models(&db, &registry, "gpt-4o");
    }

    #[test]
    fn test_show_models_no_credentials() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "gpt-4o".to_string(),
            model_type: crate::models::ModelType::Openai,
            ..Default::default()
        });

        // No API key, so list_available returns empty
        show_models(&db, &registry, "gpt-4o");
    }

    #[test]
    fn test_show_models_with_available_models() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        // Add OpenAI model
        registry.add(ModelConfig {
            name: "gpt-4o".to_string(),
            model_type: crate::models::ModelType::Openai,
            description: Some("GPT-4 Omni model".to_string()),
            context_length: 128000,
            ..Default::default()
        });

        // Save API key so model is available
        db.save_api_key("OPENAI_API_KEY", "sk-test").unwrap();

        show_models(&db, &registry, "gpt-4o");
    }

    #[test]
    fn test_show_models_multiple_providers() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        // Add models from different providers
        registry.add(ModelConfig {
            name: "gpt-4o".to_string(),
            model_type: crate::models::ModelType::Openai,
            ..Default::default()
        });
        registry.add(ModelConfig {
            name: "claude-3-opus".to_string(),
            model_type: crate::models::ModelType::Anthropic,
            description: Some("Anthropic's most capable model".to_string()),
            ..Default::default()
        });
        registry.add(ModelConfig {
            name: "gemini-pro".to_string(),
            model_type: crate::models::ModelType::Gemini,
            ..Default::default()
        });

        // Set up credentials
        db.save_api_key("OPENAI_API_KEY", "sk-test").unwrap();
        db.save_api_key("ANTHROPIC_API_KEY", "sk-ant-test").unwrap();
        db.save_api_key("GEMINI_API_KEY", "AIza-test").unwrap();

        show_models(&db, &registry, "claude-3-opus");
    }

    #[test]
    fn test_show_models_current_marker() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "model-a".to_string(),
            model_type: crate::models::ModelType::RoundRobin, // always available
            ..Default::default()
        });
        registry.add(ModelConfig {
            name: "model-b".to_string(),
            model_type: crate::models::ModelType::RoundRobin,
            ..Default::default()
        });

        // Show models with model-b as current
        show_models(&db, &registry, "model-b");
    }

    #[test]
    fn test_show_models_without_description() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "simple-model".to_string(),
            model_type: crate::models::ModelType::RoundRobin,
            description: None,
            ..Default::default()
        });

        show_models(&db, &registry, "simple-model");
    }

    #[test]
    fn test_show_models_with_description() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "described-model".to_string(),
            model_type: crate::models::ModelType::RoundRobin,
            description: Some("A model with a description".to_string()),
            ..Default::default()
        });

        show_models(&db, &registry, "other-model");
    }

    #[test]
    fn test_show_models_custom_provider() {
        use std::collections::HashMap;
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        // Custom OpenAI-compatible model
        registry.add(ModelConfig {
            name: "mycustom:gpt-4".to_string(),
            model_type: crate::models::ModelType::CustomOpenai,
            custom_endpoint: Some(crate::models::CustomEndpoint {
                url: "https://custom.api.com".to_string(),
                api_key: Some("literal-key".to_string()),
                headers: HashMap::new(),
                ca_certs_path: None,
            }),
            ..Default::default()
        });

        show_models(&db, &registry, "mycustom:gpt-4");
    }

    #[test]
    fn test_show_models_azure_provider() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "azure-gpt4".to_string(),
            model_type: crate::models::ModelType::AzureOpenai,
            ..Default::default()
        });

        db.save_api_key("AZURE_OPENAI_API_KEY", "azure-key")
            .unwrap();

        show_models(&db, &registry, "azure-gpt4");
    }

    #[test]
    fn test_show_models_openrouter_provider() {
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "openrouter-model".to_string(),
            model_type: crate::models::ModelType::Openrouter,
            description: Some("Via OpenRouter".to_string()),
            ..Default::default()
        });

        db.save_api_key("OPENROUTER_API_KEY", "or-key").unwrap();

        show_models(&db, &registry, "openrouter-model");
    }

    #[test]
    fn test_show_models_claude_code_oauth() {
        use crate::auth::TokenStorage;
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "claude-code".to_string(),
            model_type: crate::models::ModelType::ClaudeCode,
            ..Default::default()
        });

        // Set up OAuth token
        let storage = TokenStorage::new(&db);
        storage
            .save(
                "claude_code",
                "access",
                Some("refresh"),
                Some(3600),
                None,
                None,
            )
            .unwrap();

        show_models(&db, &registry, "claude-code");
    }

    #[test]
    fn test_show_models_chatgpt_oauth() {
        use crate::auth::TokenStorage;
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "chatgpt-oauth".to_string(),
            model_type: crate::models::ModelType::ChatgptOauth,
            ..Default::default()
        });

        let storage = TokenStorage::new(&db);
        storage
            .save("chatgpt", "access", Some("refresh"), Some(3600), None, None)
            .unwrap();

        show_models(&db, &registry, "chatgpt-oauth");
    }

    #[test]
    fn test_show_models_custom_anthropic() {
        use std::collections::HashMap;
        let (_tmp, db) = create_test_db();
        let mut registry = ModelRegistry::default();

        registry.add(ModelConfig {
            name: "bedrock:claude".to_string(),
            model_type: crate::models::ModelType::CustomAnthropic,
            custom_endpoint: Some(crate::models::CustomEndpoint {
                url: "https://bedrock.amazonaws.com".to_string(),
                api_key: Some("aws-key".to_string()),
                headers: HashMap::new(),
                ca_certs_path: None,
            }),
            ..Default::default()
        });

        show_models(&db, &registry, "bedrock:claude");
    }

    // =========================================================================
    // cmd_reasoning Additional Tests
    // =========================================================================

    #[test]
    fn test_cmd_reasoning_all_valid_levels() {
        let (_tmp, db) = create_test_db();

        for level in ["minimal", "low", "medium", "high", "xhigh"] {
            cmd_reasoning(&db, level);
            let settings = Settings::new(&db);
            assert_eq!(settings.get_or("reasoning_effort", ""), level);
        }
    }

    // =========================================================================
    // cmd_verbosity Additional Tests
    // =========================================================================

    #[test]
    fn test_cmd_verbosity_all_valid_levels() {
        let (_tmp, db) = create_test_db();

        for level in ["low", "medium", "high"] {
            cmd_verbosity(&db, level);
            let settings = Settings::new(&db);
            assert_eq!(settings.get_or("verbosity", ""), level);
        }
    }
}
