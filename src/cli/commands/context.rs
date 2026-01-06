//! Context and model pin management commands.

use crate::config::Settings;
use crate::db::Database;
use serdes_ai_core::ModelRequest;

/// Truncate message history to last N messages.
pub fn truncate(messages: &mut Vec<ModelRequest>, args: &str) {
    let n: usize = args.parse().unwrap_or(10);

    if messages.len() <= n {
        println!(
            "  Context has {} messages (no truncation needed)",
            messages.len()
        );
        return;
    }

    let original_len = messages.len();
    let removed = original_len - n;

    // Keep last n messages
    *messages = messages.split_off(removed);

    println!(
        "✂️  Truncated context: {} → {} messages ({} removed)",
        original_len,
        messages.len(),
        removed
    );
}

/// Show context information.
pub fn show(
    db: &Database,
    messages: &[ModelRequest],
    current_session: Option<&str>,
    agent_name: &str,
) {
    let msg_count = messages.len();
    let token_estimate = estimate_tokens(messages);

    println!("\n\x1b[1m📊 Context Info:\x1b[0m\n");
    println!("  Messages: {}", msg_count);
    println!("  Estimated tokens: ~{}", token_estimate);

    if let Some(session) = current_session {
        println!("  Current session: {}", session);
    }

    // Check for pinned model in database
    let settings = Settings::new(db);
    if let Some(pinned) = settings.get_agent_pinned_model(agent_name) {
        println!("  Pinned model: {}", pinned);
    }

    // Show part breakdown
    if msg_count > 0 {
        let total_parts: usize = messages.iter().map(|msg| msg.parts.len()).sum();
        println!("  Total parts: {}", total_parts);
    }

    println!();
}

/// Pin a model to an agent (persisted to database).
///
/// If `is_current_agent` is true, also updates the current_model reference.
pub fn pin_model(
    db: &Database,
    current_model: &mut String,
    target_agent: &str,
    model: &str,
    is_current_agent: bool,
) {
    if model.is_empty() {
        println!("❌ Please specify a model: /pin <model>");
        println!("   Or: /pin <agent> <model>");
        println!("   Example: /pin gpt-4o");
        println!("   Example: /pin reviewer gpt-4o");
        return;
    }

    let settings = Settings::new(db);
    if let Err(e) = settings.set_agent_pinned_model(target_agent, model) {
        println!("❌ Failed to pin model: {}", e);
        return;
    }

    // Only update current_model if we're pinning to the current agent
    if is_current_agent {
        *current_model = model.to_string();
    }

    println!(
        "📌 Pinned \x1b[1;33m{}\x1b[0m to agent \x1b[1;36m{}\x1b[0m",
        model, target_agent
    );
}

/// Unpin the model from an agent (removes from database).
///
/// If `is_current_agent` is true, also resets current_model to default.
pub fn unpin_model(
    db: &Database,
    current_model: &mut String,
    target_agent: &str,
    is_current_agent: bool,
) {
    let settings = Settings::new(db);

    // Check if there's actually a pin to remove
    if settings.get_agent_pinned_model(target_agent).is_none() {
        println!("  No model pinned for agent {}", target_agent);
        return;
    }

    if let Err(e) = settings.clear_agent_pinned_model(target_agent) {
        println!("❌ Failed to unpin model: {}", e);
        return;
    }

    println!(
        "📌 Unpinned model from agent \x1b[1;36m{}\x1b[0m",
        target_agent
    );

    // Only reset current_model if we're unpinning the current agent
    if is_current_agent {
        *current_model = settings.model();
        println!("   Now using default: \x1b[1;33m{}\x1b[0m", current_model);
    }
}

/// List all agent model pins.
pub fn list_pins(db: &Database) {
    let settings = Settings::new(db);

    match settings.get_all_agent_pinned_models() {
        Ok(pins) if pins.is_empty() => {
            println!("\n  No model pins configured.");
            println!("  Use /pin <model> to pin a model to the current agent.");
            println!();
        }
        Ok(pins) => {
            println!("\n\x1b[1m📌 Agent Model Pins\x1b[0m\n");

            // Find max agent name length for alignment
            let max_len = pins.keys().map(|k| k.len()).max().unwrap_or(10);

            for (agent, model) in &pins {
                println!(
                    "  \x1b[36m{:width$}\x1b[0m → \x1b[33m{}\x1b[0m",
                    agent,
                    model,
                    width = max_len
                );
            }
            println!();
        }
        Err(e) => {
            println!("❌ Failed to list pins: {}", e);
        }
    }
}

/// Get the effective model for an agent (pinned or default).
pub fn get_effective_model(db: &Database, current_model: &str, agent_name: &str) -> String {
    let settings = Settings::new(db);
    settings
        .get_agent_pinned_model(agent_name)
        .unwrap_or_else(|| current_model.to_string())
}

/// Estimate tokens in messages.
fn estimate_tokens(messages: &[ModelRequest]) -> usize {
    let mut total = 0;
    for msg in messages {
        total += serde_json::to_string(msg)
            .map(|s| s.len() / 4)
            .unwrap_or(25);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // =========================================================================
    // Test Helpers
    // =========================================================================

    fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::open_at(db_path).unwrap();
        db.migrate().unwrap();
        (temp_dir, db)
    }

    // =========================================================================
    // get_effective_model Tests
    // =========================================================================

    #[test]
    fn test_get_effective_model_no_pin() {
        let (_temp, db) = setup_test_db();
        let result = get_effective_model(&db, "gpt-4o", "stockpot");
        assert_eq!(result, "gpt-4o");
    }

    #[test]
    fn test_get_effective_model_with_pin() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "claude-3-opus")
            .unwrap();

        let result = get_effective_model(&db, "gpt-4o", "stockpot");
        assert_eq!(result, "claude-3-opus");
    }

    #[test]
    fn test_get_effective_model_different_agent() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("planner", "claude-3-opus")
            .unwrap();

        // stockpot has no pin, should return default
        let result = get_effective_model(&db, "gpt-4o", "stockpot");
        assert_eq!(result, "gpt-4o");

        // planner has a pin
        let result = get_effective_model(&db, "gpt-4o", "planner");
        assert_eq!(result, "claude-3-opus");
    }

    // =========================================================================
    // pin_model Tests
    // =========================================================================

    #[test]
    fn test_pin_model_empty_model() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();
        pin_model(&db, &mut current, "stockpot", "", true);
        // Should not change anything when model is empty
        assert_eq!(current, "gpt-4o");
    }

    #[test]
    fn test_pin_model_current_agent() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();
        pin_model(&db, &mut current, "stockpot", "claude-3", true);
        assert_eq!(current, "claude-3");
    }

    #[test]
    fn test_pin_model_different_agent() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();
        pin_model(&db, &mut current, "planner", "claude-3", false);
        // Should not change current model for different agent
        assert_eq!(current, "gpt-4o");
    }

    // =========================================================================
    // unpin_model Tests
    // =========================================================================

    #[test]
    fn test_unpin_model_no_existing_pin() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();
        unpin_model(&db, &mut current, "stockpot", true);
        // Should not change anything
        assert_eq!(current, "gpt-4o");
    }

    #[test]
    fn test_unpin_model_with_existing_pin() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "claude-3")
            .unwrap();

        let mut current = "claude-3".to_string();
        unpin_model(&db, &mut current, "stockpot", true);

        // Should reset to default
        assert_eq!(current, "gpt-4o"); // default model
    }

    // =========================================================================
    // list_pins Tests
    // =========================================================================

    #[test]
    fn test_list_pins_empty() {
        let (_temp, db) = setup_test_db();
        // Just verify it doesn't panic
        list_pins(&db);
    }

    #[test]
    fn test_list_pins_with_pins() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "gpt-4o")
            .unwrap();
        settings
            .set_agent_pinned_model("planner", "claude-3")
            .unwrap();

        // Just verify it doesn't panic
        list_pins(&db);
    }

    // =========================================================================
    // estimate_tokens Tests
    // =========================================================================

    #[test]
    fn test_estimate_tokens_empty() {
        let messages: Vec<ModelRequest> = vec![];
        assert_eq!(estimate_tokens(&messages), 0);
    }

    #[test]
    fn test_estimate_tokens_single_message() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Hello, world!".to_string());
        let messages = vec![msg];

        let tokens = estimate_tokens(&messages);
        // Should be > 0 for non-empty message
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tokens_multiple_messages() {
        let mut msg1 = ModelRequest::new();
        msg1.add_user_prompt("First message".to_string());

        let mut msg2 = ModelRequest::new();
        msg2.add_user_prompt("Second message with more content".to_string());

        let messages = vec![msg1, msg2];
        let tokens = estimate_tokens(&messages);

        // Total should be greater than a single message estimate
        assert!(tokens > 0);
    }

    // =========================================================================
    // truncate Tests
    // =========================================================================

    #[test]
    fn test_truncate_no_truncation_needed() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("test".to_string());

        let mut messages = vec![msg.clone(); 5];
        truncate(&mut messages, "10");

        // Should not truncate - 5 messages <= 10
        assert_eq!(messages.len(), 5);
    }

    #[test]
    fn test_truncate_keeps_last_n() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..10 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        truncate(&mut messages, "3");

        // Should keep last 3 messages
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_truncate_default_to_10() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..20 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        // Invalid parse should default to 10
        truncate(&mut messages, "invalid");

        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_truncate_empty_args_defaults_to_10() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..15 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        truncate(&mut messages, "");

        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_truncate_exact_match() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("test".to_string());

        let mut messages = vec![msg.clone(); 5];
        truncate(&mut messages, "5");

        // Exactly 5 messages, n=5, no truncation
        assert_eq!(messages.len(), 5);
    }

    #[test]
    fn test_truncate_to_zero() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("test".to_string());

        let mut messages = vec![msg.clone(); 5];
        truncate(&mut messages, "0");

        // Should keep 0 messages
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn test_truncate_to_one() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..5 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        truncate(&mut messages, "1");

        // Should keep only the last message
        assert_eq!(messages.len(), 1);
    }

    // =========================================================================
    // show Tests
    // =========================================================================

    #[test]
    fn test_show_empty_messages() {
        let (_temp, db) = setup_test_db();
        let messages: Vec<ModelRequest> = vec![];

        // Just verify it doesn't panic
        show(&db, &messages, None, "stockpot");
    }

    #[test]
    fn test_show_with_messages() {
        let (_temp, db) = setup_test_db();

        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Hello".to_string());
        let messages = vec![msg];

        // Just verify it doesn't panic
        show(&db, &messages, None, "stockpot");
    }

    #[test]
    fn test_show_with_session() {
        let (_temp, db) = setup_test_db();
        let messages: Vec<ModelRequest> = vec![];

        // Just verify it doesn't panic
        show(&db, &messages, Some("test-session"), "stockpot");
    }

    #[test]
    fn test_show_with_pinned_model() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "gpt-4o")
            .unwrap();

        let messages: Vec<ModelRequest> = vec![];

        // Just verify it doesn't panic
        show(&db, &messages, None, "stockpot");
    }

    #[test]
    fn test_show_with_multiple_parts() {
        let (_temp, db) = setup_test_db();

        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Part 1".to_string());
        msg.add_user_prompt("Part 2".to_string());
        let messages = vec![msg];

        // Just verify it doesn't panic with multiple parts
        show(&db, &messages, None, "stockpot");
    }

    // =========================================================================
    // unpin_model Additional Tests
    // =========================================================================

    #[test]
    fn test_unpin_model_different_agent() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("planner", "claude-3")
            .unwrap();

        let mut current = "gpt-4o".to_string();
        // is_current_agent = false, so current should not be reset
        unpin_model(&db, &mut current, "planner", false);

        // current_model should remain unchanged
        assert_eq!(current, "gpt-4o");

        // But the pin should be removed
        assert!(settings.get_agent_pinned_model("planner").is_none());
    }

    // =========================================================================
    // pin_model Additional Tests
    // =========================================================================

    #[test]
    fn test_pin_model_persists_to_db() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();

        pin_model(&db, &mut current, "stockpot", "claude-3-opus", true);

        // Verify it's persisted
        let settings = Settings::new(&db);
        assert_eq!(
            settings.get_agent_pinned_model("stockpot"),
            Some("claude-3-opus".to_string())
        );
    }

    #[test]
    fn test_pin_model_overwrites_existing() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "old-model")
            .unwrap();

        let mut current = "gpt-4o".to_string();
        pin_model(&db, &mut current, "stockpot", "new-model", true);

        assert_eq!(current, "new-model");
        assert_eq!(
            settings.get_agent_pinned_model("stockpot"),
            Some("new-model".to_string())
        );
    }

    // =========================================================================
    // Additional Edge Case Tests
    // =========================================================================

    #[test]
    fn test_truncate_empty_messages() {
        let mut messages: Vec<ModelRequest> = vec![];
        truncate(&mut messages, "5");
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn test_truncate_negative_parsed_as_default() {
        // Negative numbers parse as error, defaults to 10
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..15 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }
        truncate(&mut messages, "-5");
        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_truncate_large_n_no_effect() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("test".to_string());
        let mut messages = vec![msg.clone(); 3];

        truncate(&mut messages, "1000");
        // n > len, no truncation
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_truncate_whitespace_args() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..15 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }
        // Whitespace should fail parse, default to 10
        truncate(&mut messages, "   ");
        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_truncate_float_parsed_as_invalid() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..15 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }
        // Float should fail usize parse, default to 10
        truncate(&mut messages, "5.5");
        assert_eq!(messages.len(), 10);
    }

    // =========================================================================
    // estimate_tokens Edge Cases
    // =========================================================================

    #[test]
    fn test_estimate_tokens_large_message() {
        let mut msg = ModelRequest::new();
        let large_content = "x".repeat(10000);
        msg.add_user_prompt(large_content);
        let messages = vec![msg];

        let tokens = estimate_tokens(&messages);
        // Should scale with content size (10000 chars / 4 ≈ 2500+ tokens)
        assert!(tokens > 2000);
    }

    #[test]
    fn test_estimate_tokens_message_with_multiple_parts() {
        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Part 1".to_string());
        msg.add_user_prompt("Part 2".to_string());
        msg.add_user_prompt("Part 3".to_string());
        let messages = vec![msg];

        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    // =========================================================================
    // get_effective_model Edge Cases
    // =========================================================================

    #[test]
    fn test_get_effective_model_empty_agent_name() {
        let (_temp, db) = setup_test_db();
        // Empty agent name should just return default
        let result = get_effective_model(&db, "gpt-4o", "");
        assert_eq!(result, "gpt-4o");
    }

    #[test]
    fn test_get_effective_model_special_chars_in_agent() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // Agent with special characters
        settings
            .set_agent_pinned_model("agent-with-dashes", "claude-3")
            .unwrap();

        let result = get_effective_model(&db, "gpt-4o", "agent-with-dashes");
        assert_eq!(result, "claude-3");
    }

    #[test]
    fn test_get_effective_model_unicode_agent_name() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // Unicode agent name
        settings
            .set_agent_pinned_model("代理人", "claude-3")
            .unwrap();

        let result = get_effective_model(&db, "gpt-4o", "代理人");
        assert_eq!(result, "claude-3");
    }

    // =========================================================================
    // pin_model Edge Cases
    // =========================================================================

    #[test]
    fn test_pin_model_whitespace_only() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();

        // Whitespace-only model name - treated as empty
        pin_model(&db, &mut current, "stockpot", "   ", true);

        // Since "   " is not empty, it will be pinned
        // This tests the actual behavior
        let settings = Settings::new(&db);
        assert_eq!(
            settings.get_agent_pinned_model("stockpot"),
            Some("   ".to_string())
        );
    }

    #[test]
    fn test_pin_model_very_long_name() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();

        let long_model = "a".repeat(1000);
        pin_model(&db, &mut current, "stockpot", &long_model, true);

        assert_eq!(current, long_model);
    }

    #[test]
    fn test_pin_model_special_chars() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();

        pin_model(
            &db,
            &mut current,
            "stockpot",
            "model/with:special@chars",
            true,
        );
        assert_eq!(current, "model/with:special@chars");

        let settings = Settings::new(&db);
        assert_eq!(
            settings.get_agent_pinned_model("stockpot"),
            Some("model/with:special@chars".to_string())
        );
    }

    // =========================================================================
    // unpin_model Edge Cases
    // =========================================================================

    #[test]
    fn test_unpin_nonexistent_agent() {
        let (_temp, db) = setup_test_db();
        let mut current = "gpt-4o".to_string();

        // Unpinning an agent that was never pinned
        unpin_model(&db, &mut current, "nonexistent-agent", true);

        // Should remain unchanged
        assert_eq!(current, "gpt-4o");
    }

    #[test]
    fn test_unpin_then_pin_same_agent() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // Pin
        settings
            .set_agent_pinned_model("stockpot", "model-a")
            .unwrap();

        // Unpin
        let mut current = "model-a".to_string();
        unpin_model(&db, &mut current, "stockpot", true);

        // Verify unpinned
        assert!(settings.get_agent_pinned_model("stockpot").is_none());

        // Pin again
        pin_model(&db, &mut current, "stockpot", "model-b", true);
        assert_eq!(
            settings.get_agent_pinned_model("stockpot"),
            Some("model-b".to_string())
        );
    }

    // =========================================================================
    // list_pins Edge Cases
    // =========================================================================

    #[test]
    fn test_list_pins_many_agents() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // Add many pins
        for i in 0..20 {
            settings
                .set_agent_pinned_model(&format!("agent-{}", i), &format!("model-{}", i))
                .unwrap();
        }

        // Should handle many pins without panic
        list_pins(&db);
    }

    #[test]
    fn test_list_pins_long_names() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        let long_agent = "a".repeat(100);
        let long_model = "m".repeat(100);
        settings
            .set_agent_pinned_model(&long_agent, &long_model)
            .unwrap();

        // Should handle long names without panic
        list_pins(&db);
    }

    // =========================================================================
    // show Edge Cases
    // =========================================================================

    #[test]
    fn test_show_many_messages() {
        let (_temp, db) = setup_test_db();

        let mut messages = Vec::new();
        for i in 0..100 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        // Should handle many messages without panic
        show(&db, &messages, None, "stockpot");
    }

    #[test]
    fn test_show_with_all_options() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);
        settings
            .set_agent_pinned_model("stockpot", "pinned-model")
            .unwrap();

        let mut msg = ModelRequest::new();
        msg.add_user_prompt("Test message".to_string());
        msg.add_user_prompt("Second part".to_string());
        let messages = vec![msg];

        // All options enabled
        show(&db, &messages, Some("test-session-id"), "stockpot");
    }

    #[test]
    fn test_show_empty_session_string() {
        let (_temp, db) = setup_test_db();
        let messages: Vec<ModelRequest> = vec![];

        // Empty string session (different from None)
        show(&db, &messages, Some(""), "stockpot");
    }

    // =========================================================================
    // Integration-style Tests
    // =========================================================================

    #[test]
    fn test_pin_unpin_cycle_multiple_agents() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // Pin multiple agents
        let mut current = "default".to_string();
        pin_model(&db, &mut current, "agent1", "model-a", false);
        pin_model(&db, &mut current, "agent2", "model-b", false);
        pin_model(&db, &mut current, "agent3", "model-c", false);

        // current shouldn't change since is_current_agent=false
        assert_eq!(current, "default");

        // All should be pinned
        assert_eq!(
            settings.get_agent_pinned_model("agent1"),
            Some("model-a".to_string())
        );
        assert_eq!(
            settings.get_agent_pinned_model("agent2"),
            Some("model-b".to_string())
        );
        assert_eq!(
            settings.get_agent_pinned_model("agent3"),
            Some("model-c".to_string())
        );

        // Unpin one
        unpin_model(&db, &mut current, "agent2", false);
        assert!(settings.get_agent_pinned_model("agent2").is_none());

        // Others still pinned
        assert!(settings.get_agent_pinned_model("agent1").is_some());
        assert!(settings.get_agent_pinned_model("agent3").is_some());
    }

    #[test]
    fn test_effective_model_consistency() {
        let (_temp, db) = setup_test_db();
        let settings = Settings::new(&db);

        // No pin - should return default
        assert_eq!(get_effective_model(&db, "default", "agent"), "default");

        // Add pin
        settings.set_agent_pinned_model("agent", "pinned").unwrap();
        assert_eq!(get_effective_model(&db, "default", "agent"), "pinned");

        // Different agent without pin
        assert_eq!(get_effective_model(&db, "default", "other"), "default");

        // Remove pin
        settings.clear_agent_pinned_model("agent").unwrap();
        assert_eq!(get_effective_model(&db, "default", "agent"), "default");
    }

    #[test]
    fn test_truncate_preserves_message_order() {
        let mut messages: Vec<ModelRequest> = Vec::new();
        for i in 0..10 {
            let mut msg = ModelRequest::new();
            msg.add_user_prompt(format!("Message {}", i));
            messages.push(msg);
        }

        truncate(&mut messages, "3");

        // Should keep last 3: messages 7, 8, 9
        assert_eq!(messages.len(), 3);

        // Verify order preserved (check first part of each message)
        let content_0 = serde_json::to_string(&messages[0]).unwrap();
        let content_1 = serde_json::to_string(&messages[1]).unwrap();
        let content_2 = serde_json::to_string(&messages[2]).unwrap();

        assert!(content_0.contains("Message 7"));
        assert!(content_1.contains("Message 8"));
        assert!(content_2.contains("Message 9"));
    }
}
