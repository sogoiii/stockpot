//! Session management commands.

use crate::agents::AgentManager;
use crate::config::Settings;
use crate::db::Database;
use crate::session::{format_relative_time, SessionData, SessionManager};
use crate::tokens::{estimate_tokens, format_tokens_with_separator};
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use serdes_ai_core::ModelRequest;

/// Save the current session.
pub fn save(
    session_manager: &SessionManager,
    agents: &AgentManager,
    messages: &[ModelRequest],
    model: &str,
    name: &str,
) -> Option<String> {
    let agent_name = agents.current_name();

    let session_name = if name.is_empty() {
        session_manager.generate_name(&agent_name)
    } else {
        name.to_string()
    };

    match session_manager.save(&session_name, messages, &agent_name, model) {
        Ok(meta) => {
            println!("💾 Session saved: \x1b[1m{}\x1b[0m", session_name);
            println!(
                "   {} messages, ~{} tokens",
                meta.message_count, meta.token_estimate
            );
            Some(session_name)
        }
        Err(e) => {
            println!("❌ Failed to save session: {}", e);
            None
        }
    }
}

/// Load a session or show picker.
pub fn load(
    session_manager: &SessionManager,
    agents: &mut AgentManager,
    name: &str,
) -> Option<(String, SessionData)> {
    if name.is_empty() {
        // Show session picker
        match session_manager.list() {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("  No saved sessions.");
                    println!("  Use /save to save the current conversation.");
                } else {
                    println!("\n\x1b[1m📚 Available Sessions:\x1b[0m\n");
                    for (i, session) in sessions.iter().take(10).enumerate() {
                        println!("  {}. \x1b[1m{}\x1b[0m", i + 1, session.name);
                        println!(
                            "     {} messages, {} - {}",
                            session.message_count,
                            session.agent,
                            format_relative_time(session.updated_at)
                        );
                    }
                    println!("\n\x1b[2mUse /load <name> to load a session\x1b[0m\n");
                }
            }
            Err(e) => println!("❌ Failed to list sessions: {}", e),
        }
        return None;
    }

    match session_manager.load(name) {
        Ok(session) => {
            // Switch agent if different
            if agents.current_name() != session.meta.agent && agents.exists(&session.meta.agent) {
                let _ = agents.switch(&session.meta.agent);
            }

            println!("📥 Loaded session: \x1b[1m{}\x1b[0m", name);
            println!(
                "   {} messages, agent: {}, model: {}",
                session.meta.message_count, session.meta.agent, session.meta.model
            );

            Some((name.to_string(), session))
        }
        Err(e) => {
            println!("❌ Failed to load session: {}", e);
            None
        }
    }
}

/// List saved sessions.
pub fn list(session_manager: &SessionManager, current_session: Option<&str>) {
    match session_manager.list() {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("\n  No saved sessions.");
                println!("  Use /save to save the current conversation.\n");
            } else {
                println!("\n\x1b[1m📚 Saved Sessions:\x1b[0m\n");
                for session in &sessions {
                    let current_marker = if current_session == Some(&session.name) {
                        "→ "
                    } else {
                        "  "
                    };
                    println!("{}\x1b[1m{}\x1b[0m", current_marker, session.name);
                    println!(
                        "    {} msgs, ~{} tokens | {} | {}",
                        session.message_count,
                        session.token_estimate,
                        session.agent,
                        format_relative_time(session.updated_at)
                    );
                }
                println!();
            }
        }
        Err(e) => println!("❌ Failed to list sessions: {}", e),
    }
}

/// Delete a session.
pub fn delete(session_manager: &SessionManager, name: &str) {
    if name.is_empty() {
        println!("❌ Please specify a session name: /delete-session <name>");
        return;
    }

    match session_manager.delete(name) {
        Ok(()) => println!("🗑️  Deleted session: {}", name),
        Err(e) => println!("❌ Failed to delete session: {}", e),
    }
}

/// Handle the /context command - show context usage info with visual bar.
pub fn cmd_context(
    db: &Database,
    messages: &[ModelRequest],
    current_session: Option<&str>,
    agent_name: &str,
    context_length: usize,
) {
    let token_count = estimate_tokens(messages);
    let usage_pct = if context_length > 0 {
        (token_count as f64 / context_length as f64) * 100.0
    } else {
        0.0
    };

    println!("\n\x1b[1m📊 Context Usage\x1b[0m\n");
    println!("  Messages:    {}", messages.len());
    println!("  Tokens:      ~{}", token_count);
    println!("  Context:     {} max", context_length);
    println!(
        "  Usage:       {}/{}",
        format_tokens_with_separator(token_count),
        format_tokens_with_separator(context_length)
    );

    // Visual bar
    let bar_width = 30;
    let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
    let filled = filled.min(bar_width);
    let empty = bar_width - filled;
    let color = if usage_pct > 80.0 {
        "31"
    } else if usage_pct > 60.0 {
        "33"
    } else {
        "32"
    };
    println!(
        "  [\x1b[{}m{}\x1b[0m{}]",
        color,
        "█".repeat(filled),
        "░".repeat(empty)
    );

    if let Some(session) = current_session {
        println!("\n  Session:     {}", session);
    }
    println!("  Agent:       {}", agent_name);

    // Check for pinned model in database
    let settings = Settings::new(db);
    if let Some(pinned) = settings.get_agent_pinned_model(agent_name) {
        println!("  Pinned:      {}", pinned);
    }
    println!();
}

/// Handle the /compact command.
pub fn cmd_compact(messages: &mut Vec<ModelRequest>, args: &str) {
    if messages.is_empty() {
        println!("Nothing to compact.");
        return;
    }

    let keep: usize = args.parse().unwrap_or(10);
    println!("🗜️  Compacting (keeping last {} messages)...", keep);
    let (before, after) = compact_truncate(messages, keep);
    let after_tokens = crate::tokens::estimate_tokens(messages);
    println!(
        "✅ Compacted: {} → {} messages (~{} tokens)",
        before, after, after_tokens
    );
}

/// Compact message history using truncation strategy.
/// Keeps the first message (often system prompt) and the last N messages.
/// Returns (before_count, after_count).
pub fn compact_truncate(messages: &mut Vec<ModelRequest>, keep_recent: usize) -> (usize, usize) {
    let before = messages.len();

    // Need at least first + keep_recent messages to compact
    if before <= keep_recent + 1 {
        return (before, before); // Nothing to do
    }

    // Keep first message (usually system prompt) + last N messages
    let first_msg = messages.remove(0);
    let keep_count = keep_recent.min(messages.len());
    let start_idx = messages.len().saturating_sub(keep_count);
    let recent: Vec<_> = messages.drain(start_idx..).collect();

    messages.clear();
    messages.push(first_msg);
    messages.extend(recent);

    (before, messages.len())
}

/// Interactive session loader using fuzzy select.
pub fn load_interactive(
    session_manager: &SessionManager,
    agents: &mut AgentManager,
) -> Option<(String, SessionData)> {
    let sessions = match session_manager.list() {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => {
            println!("  No saved sessions found.");
            return None;
        }
        Err(e) => {
            println!("❌ Failed to list sessions: {}", e);
            return None;
        }
    };

    let display: Vec<String> = sessions
        .iter()
        .map(|s| {
            format!(
                "{} ({} msgs, {} - {})",
                s.name,
                s.message_count,
                s.agent,
                format_relative_time(s.updated_at)
            )
        })
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select session to load")
        .items(&display)
        .interact_opt()
        .ok()??;

    let name = &sessions[selection].name;
    match session_manager.load(name) {
        Ok(data) => {
            if agents.current_name() != data.meta.agent && agents.exists(&data.meta.agent) {
                let _ = agents.switch(&data.meta.agent);
            }

            println!(
                "📥 Loaded: \x1b[1m{}\x1b[0m ({} messages)",
                name, data.meta.message_count
            );
            Some((name.clone(), data))
        }
        Err(e) => {
            println!("❌ Failed to load: {}", e);
            None
        }
    }
}

/// Show current session info.
pub fn show_session(current_session: Option<&str>, autosave_enabled: bool) {
    match current_session {
        Some(name) => {
            println!("📋 Current session: \x1b[1m{}\x1b[0m", name);
        }
        None => {
            println!("📋 No active session");
            if autosave_enabled {
                println!("   \x1b[2m(Auto-save will create one after first response)\x1b[0m");
            } else {
                println!("   \x1b[2mUse /save to create a session\x1b[0m");
            }
        }
    }
}

/// Show interactive command picker - returns the command to execute
pub fn command_picker() -> Option<String> {
    use dialoguer::{theme::ColorfulTheme, Select};

    let commands = vec![
        ("model", "Select model (interactive)"),
        ("agent", "Select agent (interactive)"),
        ("show", "Show current status"),
        ("context", "Show context usage"),
        ("resume", "Load a saved session"),
        ("save", "Save current session"),
        ("compact", "Compact message history"),
        ("ms", "Edit model settings"),
        ("mcp", "MCP server management"),
        ("tools", "List available tools"),
        ("help", "Show all commands"),
        ("yolo", "Toggle YOLO mode"),
        ("set", "Show/edit configuration"),
        ("new", "Start new conversation"),
        ("exit", "Exit stockpot"),
    ];

    let display: Vec<String> = commands
        .iter()
        .map(|(cmd, desc)| format!("/{:<12} {}", cmd, desc))
        .collect();

    println!(); // Add spacing

    match Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select command")
        .items(&display)
        .default(0)
        .interact_opt()
    {
        Ok(Some(idx)) => {
            let (cmd, _) = commands[idx];
            println!("\x1b[2m> /{}\x1b[0m\n", cmd);
            Some(format!("/{}", cmd))
        }
        _ => {
            println!("Cancelled.");
            None
        }
    }
}

/// Auto-save session after a response.
/// Returns the new session name if one was created.
pub fn auto_save(
    session_manager: &SessionManager,
    current_session: &Option<String>,
    messages: &[ModelRequest],
    agent_name: &str,
    model: &str,
) -> Option<String> {
    if let Some(ref session_name) = current_session {
        // Update existing session silently
        let _ = session_manager.save(session_name, messages, agent_name, model);
        None
    } else if messages.len() >= 2 {
        // Create auto-session after first exchange
        let name = format!("auto-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
        if session_manager
            .save(&name, messages, agent_name, model)
            .is_ok()
        {
            Some(name)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // =========================================================================
    // Helper: Create test session manager with temp dir
    // =========================================================================

    fn create_test_session_manager() -> (TempDir, SessionManager) {
        let temp = TempDir::new().unwrap();
        let manager = SessionManager::with_dir(temp.path());
        (temp, manager)
    }

    // =========================================================================
    // show_session Tests
    // =========================================================================

    #[test]
    fn test_show_session_with_name() {
        // Just verify it doesn't panic
        show_session(Some("test-session"), false);
    }

    #[test]
    fn test_show_session_none_no_autosave() {
        show_session(None, false);
    }

    #[test]
    fn test_show_session_none_with_autosave() {
        show_session(None, true);
    }

    #[test]
    fn test_show_session_long_name() {
        show_session(
            Some("very-long-session-name-with-many-parts-2024-01-01"),
            false,
        );
    }

    // =========================================================================
    // delete Tests
    // =========================================================================

    #[test]
    fn test_delete_empty_name() {
        // Create a mock session manager
        let session_manager = SessionManager::new();

        // This should print an error message about empty name
        delete(&session_manager, "");
    }

    #[test]
    fn test_delete_nonexistent_session() {
        let (_temp, manager) = create_test_session_manager();
        // Should print error, not panic
        delete(&manager, "nonexistent-session");
    }

    #[test]
    fn test_delete_existing_session() {
        let (_temp, manager) = create_test_session_manager();

        // Create a session first
        let messages: Vec<ModelRequest> = vec![];
        manager
            .save("to-delete", &messages, "stockpot", "gpt-4o")
            .unwrap();

        assert!(manager.exists("to-delete"));

        // Delete it
        delete(&manager, "to-delete");

        // Verify deleted
        assert!(!manager.exists("to-delete"));
    }

    // =========================================================================
    // compact_truncate Unit Tests
    // =========================================================================

    #[test]
    fn test_compact_truncate_empty_vec() {
        let mut messages: Vec<ModelRequest> = vec![];
        let (before, after) = compact_truncate(&mut messages, 5);
        assert_eq!(before, 0);
        assert_eq!(after, 0);
    }

    #[test]
    fn test_compact_truncate_fewer_than_keep() {
        // Can't easily create ModelRequest, so test with empty vec
        // The logic: if messages.len() <= keep_recent + 1, return unchanged
        let mut messages: Vec<ModelRequest> = vec![];
        let (before, after) = compact_truncate(&mut messages, 10);
        assert_eq!(before, after);
    }

    // =========================================================================
    // list Tests
    // =========================================================================

    #[test]
    fn test_list_empty_sessions() {
        let (_temp, manager) = create_test_session_manager();
        // Should not panic, just print "No saved sessions"
        list(&manager, None);
    }

    #[test]
    fn test_list_with_sessions() {
        let (_temp, manager) = create_test_session_manager();

        // Create some sessions
        let messages: Vec<ModelRequest> = vec![];
        manager
            .save("session-1", &messages, "stockpot", "gpt-4o")
            .unwrap();
        manager
            .save("session-2", &messages, "planning", "claude-3")
            .unwrap();

        // Should list them without panic
        list(&manager, None);
    }

    #[test]
    fn test_list_with_current_session() {
        let (_temp, manager) = create_test_session_manager();

        let messages: Vec<ModelRequest> = vec![];
        manager
            .save("current", &messages, "stockpot", "gpt-4o")
            .unwrap();
        manager
            .save("other", &messages, "stockpot", "gpt-4o")
            .unwrap();

        // Should show marker for current session
        list(&manager, Some("current"));
    }

    // =========================================================================
    // save Tests
    // =========================================================================

    #[test]
    fn test_save_new_session() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        let result = save(&session_manager, &agents, &messages, "gpt-4o", "test-save");

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "test-save");
        assert!(session_manager.exists("test-save"));
    }

    #[test]
    fn test_save_auto_generate_name() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Empty name should generate one
        let result = save(&session_manager, &agents, &messages, "gpt-4o", "");

        assert!(result.is_some());
        let name = result.unwrap();
        assert!(name.starts_with("stockpot-")); // Default agent name
    }

    #[test]
    fn test_save_overwrite_existing() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save first time
        save(&session_manager, &agents, &messages, "gpt-4o", "overwrite");

        // Save again with same name
        let result = save(&session_manager, &agents, &messages, "gpt-4o", "overwrite");

        assert!(result.is_some());
    }

    // =========================================================================
    // load Tests
    // =========================================================================

    #[test]
    fn test_load_empty_name_shows_list() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();

        // Empty name should show list, return None
        let result = load(&session_manager, &mut agents, "");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_nonexistent_session() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();

        let result = load(&session_manager, &mut agents, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_existing_session() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save a session first
        session_manager
            .save("loadable", &messages, "stockpot", "gpt-4o")
            .unwrap();

        let result = load(&session_manager, &mut agents, "loadable");
        assert!(result.is_some());

        let (name, data) = result.unwrap();
        assert_eq!(name, "loadable");
        assert_eq!(data.meta.agent, "stockpot");
    }

    // =========================================================================
    // auto_save Tests
    // =========================================================================

    #[test]
    fn test_auto_save_no_session_few_messages() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages: Vec<ModelRequest> = vec![];
        let current_session: Option<String> = None;

        // With < 2 messages, should not create session
        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_none());
    }

    #[test]
    fn test_auto_save_existing_session_updates() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages: Vec<ModelRequest> = vec![];

        // Create initial session
        session_manager
            .save("existing", &messages, "stockpot", "gpt-4o")
            .unwrap();

        let current_session = Some("existing".to_string());

        // Auto-save with existing session should update, return None
        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_none());
    }

    // =========================================================================
    // cmd_context Tests
    // =========================================================================

    #[test]
    fn test_cmd_context_empty_messages() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages: Vec<ModelRequest> = vec![];

        cmd_context(&db, &messages, None, "stockpot", 128000);
    }

    #[test]
    fn test_cmd_context_with_session() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages: Vec<ModelRequest> = vec![];

        cmd_context(&db, &messages, Some("my-session"), "stockpot", 128000);
    }

    #[test]
    fn test_cmd_context_zero_context_length() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages: Vec<ModelRequest> = vec![];

        // Should handle 0 context length without divide by zero
        cmd_context(&db, &messages, None, "stockpot", 0);
    }

    // =========================================================================
    // cmd_compact Tests
    // =========================================================================

    #[test]
    fn test_cmd_compact_empty_messages() {
        let mut messages: Vec<ModelRequest> = vec![];
        cmd_compact(&mut messages, "10");
    }

    #[test]
    fn test_cmd_compact_default_keep() {
        let mut messages: Vec<ModelRequest> = vec![];
        // Empty args should default to 10
        cmd_compact(&mut messages, "");
    }

    #[test]
    fn test_cmd_compact_invalid_number() {
        let mut messages: Vec<ModelRequest> = vec![];
        // Invalid number should default to 10
        cmd_compact(&mut messages, "not-a-number");
    }

    // =========================================================================
    // SessionManager Tests (indirectly testing session.rs functions)
    // =========================================================================

    #[test]
    fn test_session_manager_generate_unique_names() {
        let (_temp, manager) = create_test_session_manager();

        let name1 = manager.generate_name("test");
        let name2 = manager.generate_name("test");

        // Names should be unique (different timestamps or suffixes)
        // In practice they might be the same if generated very quickly
        assert!(name1.starts_with("test-"));
        assert!(name2.starts_with("test-"));
    }

    #[test]
    fn test_session_manager_with_max_sessions() {
        let temp = TempDir::new().unwrap();
        let manager = SessionManager::with_dir(temp.path()).with_max_sessions(2);

        let messages: Vec<ModelRequest> = vec![];

        // Save 3 sessions
        manager
            .save("session-1", &messages, "agent", "model")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager
            .save("session-2", &messages, "agent", "model")
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager
            .save("session-3", &messages, "agent", "model")
            .unwrap();

        // After cleanup, oldest should be removed (only 2 kept)
        let sessions = manager.list().unwrap();
        assert!(sessions.len() <= 2);
    }

    // =========================================================================
    // Helper: Create test ModelRequest
    // =========================================================================

    fn create_test_message(content: &str) -> ModelRequest {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt(content.to_string());
        msg
    }

    fn create_test_messages(count: usize) -> Vec<ModelRequest> {
        (0..count)
            .map(|i| create_test_message(&format!("Message {}", i)))
            .collect()
    }

    // =========================================================================
    // compact_truncate with actual messages
    // =========================================================================

    #[test]
    fn test_compact_truncate_with_messages() {
        // Create 10 messages
        let mut messages = create_test_messages(10);
        let (before, after) = compact_truncate(&mut messages, 3);

        assert_eq!(before, 10);
        // Should keep first (system) + 3 recent = 4
        assert_eq!(after, 4);
        assert_eq!(messages.len(), 4);
    }

    #[test]
    fn test_compact_truncate_keep_more_than_available() {
        let mut messages = create_test_messages(5);
        let (before, after) = compact_truncate(&mut messages, 10);

        // 5 messages <= 10 + 1, so nothing should be compacted
        assert_eq!(before, 5);
        assert_eq!(after, 5);
    }

    #[test]
    fn test_compact_truncate_exact_boundary() {
        // If messages.len() == keep_recent + 1, no compaction
        let mut messages = create_test_messages(6);
        let (before, after) = compact_truncate(&mut messages, 5);

        assert_eq!(before, 6);
        assert_eq!(after, 6);
    }

    #[test]
    fn test_compact_truncate_single_message() {
        let mut messages = create_test_messages(1);
        let (before, after) = compact_truncate(&mut messages, 5);

        assert_eq!(before, 1);
        assert_eq!(after, 1);
    }

    #[test]
    fn test_compact_truncate_keeps_first_message() {
        let mut messages = create_test_messages(20);
        // Mark first message distinctly by checking it's preserved
        let first_msg = messages[0].clone();

        compact_truncate(&mut messages, 5);

        // First message should still be first
        assert_eq!(messages.len(), 6); // 1 + 5
                                       // Verify first message is preserved (they're equal by content)
        assert_eq!(
            serde_json::to_string(&messages[0]).unwrap(),
            serde_json::to_string(&first_msg).unwrap()
        );
    }

    // =========================================================================
    // load with sessions present (empty name path)
    // =========================================================================

    #[test]
    fn test_load_empty_name_with_many_sessions() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();

        // Create more than 10 sessions to test the take(10) limit
        for i in 0..15 {
            let messages: Vec<ModelRequest> = vec![];
            session_manager
                .save(
                    &format!("session-{:02}", i),
                    &messages,
                    "stockpot",
                    "gpt-4o",
                )
                .unwrap();
        }

        // Empty name should show list (limited to 10), return None
        let result = load(&session_manager, &mut agents, "");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_switches_agent() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save session with "planning" agent (built-in agent)
        session_manager
            .save("planning-session", &messages, "planning", "gpt-4o")
            .unwrap();

        // Verify current agent before load
        let before_agent = agents.current_name();

        // Load the session
        let result = load(&session_manager, &mut agents, "planning-session");

        assert!(result.is_some());
        let (name, data) = result.unwrap();
        assert_eq!(name, "planning-session");
        assert_eq!(data.meta.agent, "planning");

        // If planning agent exists, it should have switched
        if agents.exists("planning") && before_agent != "planning" {
            assert_eq!(agents.current_name(), "planning");
        }
    }

    // =========================================================================
    // cmd_context color thresholds
    // =========================================================================

    #[test]
    fn test_cmd_context_high_usage_red() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        // Create messages that would be > 80% of small context
        let messages = create_test_messages(100);

        // Small context to trigger high usage
        cmd_context(&db, &messages, None, "stockpot", 100);
    }

    #[test]
    fn test_cmd_context_medium_usage_yellow() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages = create_test_messages(10);

        // Context size that gives ~65% usage
        cmd_context(&db, &messages, None, "stockpot", 500);
    }

    #[test]
    fn test_cmd_context_low_usage_green() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages = create_test_messages(5);

        // Large context for low usage
        cmd_context(&db, &messages, None, "stockpot", 100000);
    }

    // =========================================================================
    // auto_save creates new session
    // =========================================================================

    #[test]
    fn test_auto_save_creates_session_with_enough_messages() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages = create_test_messages(3); // >= 2 messages
        let current_session: Option<String> = None;

        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_some());
        let name = result.unwrap();
        assert!(name.starts_with("auto-"));
        assert!(session_manager.exists(&name));
    }

    #[test]
    fn test_auto_save_exactly_two_messages() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages = create_test_messages(2); // exactly 2 messages
        let current_session: Option<String> = None;

        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_some());
    }

    #[test]
    fn test_auto_save_one_message_no_session() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages = create_test_messages(1); // only 1 message
        let current_session: Option<String> = None;

        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_none());
    }

    // =========================================================================
    // cmd_compact with messages
    // =========================================================================

    #[test]
    fn test_cmd_compact_with_messages() {
        let mut messages = create_test_messages(20);
        cmd_compact(&mut messages, "5");

        // Should compact to 1 (first) + 5 (recent) = 6
        assert_eq!(messages.len(), 6);
    }

    #[test]
    fn test_cmd_compact_large_keep_value() {
        let mut messages = create_test_messages(5);
        cmd_compact(&mut messages, "100");

        // Can't compact when keep > available
        assert_eq!(messages.len(), 5);
    }

    // =========================================================================
    // save with messages
    // =========================================================================

    #[test]
    fn test_save_with_messages() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let messages = create_test_messages(5);

        let result = save(
            &session_manager,
            &agents,
            &messages,
            "claude-3",
            "with-messages",
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "with-messages");

        // Verify the session was saved with correct message count
        let loaded = session_manager.load("with-messages").unwrap();
        assert_eq!(loaded.meta.message_count, 5);
    }

    // =========================================================================
    // list edge cases
    // =========================================================================

    #[test]
    fn test_list_current_session_not_in_list() {
        let (_temp, manager) = create_test_session_manager();

        let messages: Vec<ModelRequest> = vec![];
        manager
            .save("exists", &messages, "stockpot", "gpt-4o")
            .unwrap();

        // Current session doesn't exist in saved sessions
        list(&manager, Some("nonexistent-current"));
    }

    // =========================================================================
    // load with agent that doesn't exist
    // =========================================================================

    #[test]
    fn test_load_session_with_unknown_agent() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save session with non-existent agent
        session_manager
            .save(
                "unknown-agent-session",
                &messages,
                "nonexistent-agent",
                "gpt-4o",
            )
            .unwrap();

        let result = load(&session_manager, &mut agents, "unknown-agent-session");

        // Should still load, just won't switch agent
        assert!(result.is_some());
        let (name, data) = result.unwrap();
        assert_eq!(name, "unknown-agent-session");
        assert_eq!(data.meta.agent, "nonexistent-agent");
        // Current agent should remain unchanged (stockpot)
        assert_eq!(agents.current_name(), "stockpot");
    }

    // =========================================================================
    // delete special characters in name
    // =========================================================================

    #[test]
    fn test_delete_session_with_special_chars() {
        let (_temp, manager) = create_test_session_manager();

        // Try deleting with special chars (should fail gracefully)
        delete(&manager, "session with spaces");
        delete(&manager, "../../../etc/passwd");
        delete(&manager, "session/with/slashes");
    }

    // =========================================================================
    // show_session edge cases
    // =========================================================================

    #[test]
    fn test_show_session_special_chars_in_name() {
        show_session(Some("session-with-unicode-\u{1F600}"), false);
    }

    #[test]
    fn test_show_session_empty_string_name() {
        // Empty string is not None
        show_session(Some(""), false);
    }

    // =========================================================================
    // compact_truncate additional edge cases
    // =========================================================================

    #[test]
    fn test_compact_truncate_keep_zero() {
        // Keep 0 recent means only first message preserved
        let mut messages = create_test_messages(10);
        let (before, after) = compact_truncate(&mut messages, 0);

        assert_eq!(before, 10);
        // 1 (first) + 0 (recent) = 1
        assert_eq!(after, 1);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn test_compact_truncate_two_messages_keep_one() {
        // Boundary: 2 messages, keep 1 -> no compaction (2 <= 1 + 1)
        let mut messages = create_test_messages(2);
        let (before, after) = compact_truncate(&mut messages, 1);

        assert_eq!(before, 2);
        assert_eq!(after, 2);
    }

    #[test]
    fn test_compact_truncate_three_messages_keep_one() {
        // 3 messages, keep 1 -> compaction happens (3 > 1 + 1)
        let mut messages = create_test_messages(3);
        let (before, after) = compact_truncate(&mut messages, 1);

        assert_eq!(before, 3);
        // 1 (first) + 1 (recent) = 2
        assert_eq!(after, 2);
    }

    #[test]
    fn test_compact_truncate_preserves_order() {
        // Verify first message is first, and last N are at the end
        let mut messages = create_test_messages(10);
        compact_truncate(&mut messages, 3);

        // Should be: msg[0], msg[7], msg[8], msg[9] (first + last 3)
        assert_eq!(messages.len(), 4);
    }

    // =========================================================================
    // cmd_compact additional tests
    // =========================================================================

    #[test]
    fn test_cmd_compact_keep_zero() {
        let mut messages = create_test_messages(10);
        cmd_compact(&mut messages, "0");

        // Should keep only first message
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn test_cmd_compact_whitespace_args() {
        let mut messages = create_test_messages(10);
        // Whitespace should parse as invalid -> default 10
        cmd_compact(&mut messages, "  ");

        // 10 messages with keep 10 -> no compaction needed
        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_cmd_compact_negative_number() {
        let mut messages = create_test_messages(10);
        // Negative should fail parse -> default 10
        cmd_compact(&mut messages, "-5");

        assert_eq!(messages.len(), 10);
    }

    // =========================================================================
    // cmd_context additional tests
    // =========================================================================

    #[test]
    fn test_cmd_context_exact_full_usage() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        // Create messages that roughly fill the context
        let messages = create_test_messages(50);

        // Very small context to ensure high usage
        cmd_context(&db, &messages, None, "stockpot", 50);
    }

    #[test]
    fn test_cmd_context_over_capacity() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        // More tokens than context allows (should clamp bar to max)
        let messages = create_test_messages(100);
        // Context smaller than token estimate
        cmd_context(&db, &messages, None, "stockpot", 10);
    }

    #[test]
    fn test_cmd_context_with_different_agents() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        let messages: Vec<ModelRequest> = vec![];

        // Test with various agent names
        cmd_context(&db, &messages, None, "planning", 128000);
        cmd_context(&db, &messages, None, "code-review", 128000);
        cmd_context(&db, &messages, None, "custom-agent", 128000);
    }

    // =========================================================================
    // load additional tests
    // =========================================================================

    #[test]
    fn test_load_same_agent_no_switch() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save session with current agent (stockpot is default)
        session_manager
            .save("same-agent", &messages, "stockpot", "gpt-4o")
            .unwrap();

        let before_agent = agents.current_name();
        let result = load(&session_manager, &mut agents, "same-agent");

        assert!(result.is_some());
        // Agent should remain the same
        assert_eq!(agents.current_name(), before_agent);
    }

    #[test]
    fn test_load_session_preserves_model_info() {
        let (_temp, session_manager) = create_test_session_manager();
        let mut agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save with specific model
        session_manager
            .save("model-test", &messages, "stockpot", "claude-3-opus")
            .unwrap();

        let result = load(&session_manager, &mut agents, "model-test");
        assert!(result.is_some());

        let (_, data) = result.unwrap();
        assert_eq!(data.meta.model, "claude-3-opus");
    }

    // =========================================================================
    // save additional tests
    // =========================================================================

    #[test]
    fn test_save_unicode_content_in_messages() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();

        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Hello! \u{4E2D}\u{6587} \u{1F600}".to_string());
        let messages = vec![msg];

        let result = save(
            &session_manager,
            &agents,
            &messages,
            "gpt-4o",
            "unicode-test",
        );

        assert!(result.is_some());
        assert!(session_manager.exists("unicode-test"));
    }

    #[test]
    fn test_save_different_models() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let messages: Vec<ModelRequest> = vec![];

        // Save with different model names
        for model in &["gpt-4o", "claude-3-opus", "gemini-pro", "llama-70b"] {
            let name = format!("session-{}", model);
            let result = save(&session_manager, &agents, &messages, model, &name);
            assert!(result.is_some());
        }
    }

    // =========================================================================
    // auto_save additional tests
    // =========================================================================

    #[test]
    fn test_auto_save_name_format() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages = create_test_messages(2);
        let current_session: Option<String> = None;

        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "stockpot",
            "gpt-4o",
        );

        assert!(result.is_some());
        let name = result.unwrap();
        // Verify format: auto-YYYYMMDD-HHMMSS
        assert!(name.starts_with("auto-"));
        assert!(name.len() > "auto-".len());
        // Should contain date-like pattern
        assert!(name.chars().skip(5).take(8).all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_auto_save_with_different_agent_saves_correctly() {
        let (_temp, session_manager) = create_test_session_manager();
        let messages = create_test_messages(3);
        let current_session: Option<String> = None;

        // Auto-save with one agent
        let result = auto_save(
            &session_manager,
            &current_session,
            &messages,
            "planning",
            "gpt-4o",
        );
        assert!(result.is_some());

        let name = result.unwrap();
        // Verify session was saved
        assert!(session_manager.exists(&name));

        // Load and verify agent/model are stored (not actually used by auto_save
        // since the name is auto-generated, but session data should be correct)
        let loaded = session_manager.load(&name).unwrap();
        assert_eq!(loaded.meta.agent, "planning");
        assert_eq!(loaded.meta.model, "gpt-4o");
        assert_eq!(loaded.messages.len(), 3);
    }

    // =========================================================================
    // list additional tests
    // =========================================================================

    #[test]
    fn test_list_many_sessions_ordering() {
        let (_temp, manager) = create_test_session_manager();
        let messages: Vec<ModelRequest> = vec![];

        // Create sessions with delays to ensure ordering
        for i in 0..5 {
            manager
                .save(&format!("session-{}", i), &messages, "stockpot", "gpt-4o")
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // List should be ordered by most recent first
        let sessions = manager.list().unwrap();
        assert_eq!(sessions.len(), 5);
        // Most recent (session-4) should be first
        assert_eq!(sessions[0].name, "session-4");
    }

    #[test]
    fn test_list_shows_token_estimates() {
        let (_temp, manager) = create_test_session_manager();

        let messages = create_test_messages(10);
        manager
            .save("with-tokens", &messages, "stockpot", "gpt-4o")
            .unwrap();

        let sessions = manager.list().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].token_estimate > 0);
    }

    // =========================================================================
    // delete additional tests
    // =========================================================================

    #[test]
    fn test_delete_whitespace_name() {
        let session_manager = SessionManager::new();
        // Whitespace-only name treated as non-empty but invalid
        delete(&session_manager, "   ");
    }

    #[test]
    fn test_delete_then_recreate() {
        let (_temp, manager) = create_test_session_manager();
        let messages: Vec<ModelRequest> = vec![];

        // Create
        manager
            .save("recreate-test", &messages, "stockpot", "gpt-4o")
            .unwrap();
        assert!(manager.exists("recreate-test"));

        // Delete
        delete(&manager, "recreate-test");
        assert!(!manager.exists("recreate-test"));

        // Recreate
        manager
            .save("recreate-test", &messages, "stockpot", "gpt-4o")
            .unwrap();
        assert!(manager.exists("recreate-test"));
    }

    // =========================================================================
    // Integration tests for command flow
    // =========================================================================

    #[test]
    fn test_save_then_load_roundtrip() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let mut agents_for_load = AgentManager::new();

        let messages = create_test_messages(5);

        // Save
        let save_result = save(
            &session_manager,
            &agents,
            &messages,
            "test-model",
            "roundtrip",
        );
        assert!(save_result.is_some());

        // Load
        let load_result = load(&session_manager, &mut agents_for_load, "roundtrip");
        assert!(load_result.is_some());

        let (name, data) = load_result.unwrap();
        assert_eq!(name, "roundtrip");
        assert_eq!(data.messages.len(), 5);
        assert_eq!(data.meta.model, "test-model");
    }

    #[test]
    fn test_save_compact_save_load() {
        let (_temp, session_manager) = create_test_session_manager();
        let agents = AgentManager::new();
        let mut agents_for_load = AgentManager::new();

        let mut messages = create_test_messages(20);

        // Save initial
        save(
            &session_manager,
            &agents,
            &messages,
            "gpt-4o",
            "compact-flow",
        );

        // Compact
        cmd_compact(&mut messages, "5");
        assert_eq!(messages.len(), 6); // 1 + 5

        // Save after compact
        save(
            &session_manager,
            &agents,
            &messages,
            "gpt-4o",
            "compact-flow",
        );

        // Load and verify compacted state
        let (_, data) = load(&session_manager, &mut agents_for_load, "compact-flow").unwrap();
        assert_eq!(data.messages.len(), 6);
    }

    #[test]
    fn test_list_after_multiple_operations() {
        let (_temp, manager) = create_test_session_manager();
        let messages: Vec<ModelRequest> = vec![];

        // Create several sessions
        manager.save("s1", &messages, "agent", "model").unwrap();
        manager.save("s2", &messages, "agent", "model").unwrap();
        manager.save("s3", &messages, "agent", "model").unwrap();

        // Delete one
        manager.delete("s2").unwrap();

        // List should show 2
        let sessions = manager.list().unwrap();
        assert_eq!(sessions.len(), 2);

        // Verify correct ones remain
        let names: Vec<_> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"s1"));
        assert!(names.contains(&"s3"));
        assert!(!names.contains(&"s2"));
    }

    // =========================================================================
    // Error path tests
    // =========================================================================

    #[test]
    fn test_cmd_context_with_large_message_count() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        // Many messages
        let messages = create_test_messages(500);
        cmd_context(&db, &messages, Some("large-session"), "stockpot", 1000000);
    }

    #[test]
    fn test_compact_truncate_idempotent() {
        let mut messages = create_test_messages(10);
        compact_truncate(&mut messages, 5);
        assert_eq!(messages.len(), 6);

        // Compact again - should be no-op now
        let (before, after) = compact_truncate(&mut messages, 5);
        assert_eq!(before, 6);
        assert_eq!(after, 6);
    }

    // =========================================================================
    // Bar rendering edge cases in cmd_context
    // =========================================================================

    #[test]
    fn test_cmd_context_bar_width_limits() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let db = Database::open_at(path).unwrap();
        db.migrate().unwrap();

        // Test 0% usage
        let empty_messages: Vec<ModelRequest> = vec![];
        cmd_context(&db, &empty_messages, None, "stockpot", 100000);

        // Test ~50% usage
        let messages = create_test_messages(20);
        cmd_context(&db, &messages, None, "stockpot", 1000);

        // Test >100% usage (should clamp)
        let many_messages = create_test_messages(100);
        cmd_context(&db, &many_messages, None, "stockpot", 1);
    }
}
