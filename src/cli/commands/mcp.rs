//! MCP command handlers with interactive management.

use crate::mcp::{McpConfig, McpManager, McpServerEntry};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

/// Handle MCP subcommands.
pub async fn handle(manager: &McpManager, args: &str) {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("");
    let subargs = parts.get(1).copied().unwrap_or("");

    match subcommand {
        "" | "help" => show_help(),
        "list" | "ls" => list_servers(manager),
        "status" => show_status(manager).await,
        "start" => start_server(manager, subargs).await,
        "stop" => stop_server(manager, subargs).await,
        "restart" => restart_server(manager, subargs).await,
        "start-all" => start_all(manager).await,
        "stop-all" => stop_all(manager).await,
        "tools" => list_tools(manager).await,
        "add" => add_server_interactive(),
        "remove" | "rm" => remove_server(subargs),
        "enable" => toggle_server(subargs, true),
        "disable" => toggle_server(subargs, false),
        _ => println!("Unknown MCP command: {}. Try /mcp help", subcommand),
    }
}

fn show_help() {
    println!(
        "
\x1b[1m🔌 MCP Server Management\x1b[0m

  \x1b[36m/mcp list\x1b[0m              List configured servers
  \x1b[36m/mcp status\x1b[0m            Show running servers with tool counts
  \x1b[36m/mcp tools\x1b[0m             List tools from running servers
  \x1b[36m/mcp start [name]\x1b[0m      Start a server (interactive if no name)
  \x1b[36m/mcp stop [name]\x1b[0m       Stop a server (interactive if no name)
  \x1b[36m/mcp restart [name]\x1b[0m    Restart a server
  \x1b[36m/mcp start-all\x1b[0m         Start all enabled servers
  \x1b[36m/mcp stop-all\x1b[0m          Stop all servers
  \x1b[36m/mcp add\x1b[0m               Add new server (interactive wizard)
  \x1b[36m/mcp remove [name]\x1b[0m     Remove a server
  \x1b[36m/mcp enable <name>\x1b[0m     Enable a server
  \x1b[36m/mcp disable <name>\x1b[0m    Disable a server

\x1b[2mConfig: ~/.stockpot/mcp_servers.json\x1b[0m
"
    );
}

fn list_servers(manager: &McpManager) {
    let config = manager.config();

    if config.servers.is_empty() {
        println!("\n  No MCP servers configured.");
        println!("  Use \x1b[36m/mcp add\x1b[0m to add one.\n");
        return;
    }

    println!("\n\x1b[1m📋 Configured MCP Servers\x1b[0m\n");

    for (name, entry) in &config.servers {
        let status = if entry.enabled { "✓" } else { "○" };
        let status_color = if entry.enabled { "32" } else { "90" };

        println!(
            "  \x1b[{}m{}\x1b[0m \x1b[1;36m{}\x1b[0m",
            status_color, status, name
        );
        println!(
            "    \x1b[2m{} {}\x1b[0m",
            entry.command,
            entry.args.join(" ")
        );

        if let Some(ref desc) = entry.description {
            println!("    \x1b[2;3m{}\x1b[0m", desc);
        }

        if !entry.env.is_empty() {
            let env_keys: Vec<_> = entry.env.keys().collect();
            println!(
                "    \x1b[2menv: {}\x1b[0m",
                env_keys
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    println!();
}

async fn show_status(manager: &McpManager) {
    let running = manager.running_servers().await;
    let config = manager.config();

    println!("\n\x1b[1m📊 MCP Server Status\x1b[0m\n");

    if running.is_empty() {
        println!("  No servers running.");
        let enabled: Vec<_> = config.enabled_servers().collect();
        if !enabled.is_empty() {
            let names: Vec<_> = enabled.iter().map(|(n, _)| n.as_str()).collect();
            println!("  \x1b[2mEnabled servers: {}\x1b[0m", names.join(", "));
            println!("  \x1b[2mUse /mcp start-all to start them\x1b[0m");
        }
        println!();
        return;
    }

    // Get all tools for counts
    let tools = manager.list_all_tools().await;

    for name in &running {
        print!("  \x1b[32m●\x1b[0m \x1b[1m{}\x1b[0m", name);
        if let Some(server_tools) = tools.get(name.as_str()) {
            print!(" ({} tools)", server_tools.len());
        }
        println!();
    }

    // Show stopped but enabled
    let stopped: Vec<_> = config
        .enabled_servers()
        .filter(|(n, _)| !running.contains(&n.to_string()))
        .collect();

    if !stopped.is_empty() {
        println!();
        for (name, _) in stopped {
            println!("  \x1b[90m○\x1b[0m \x1b[2m{}\x1b[0m (stopped)", name);
        }
    }
    println!();
}

async fn start_server(manager: &McpManager, name: &str) {
    if name.is_empty() {
        // Interactive selection
        let config = manager.config();
        let servers: Vec<_> = config.servers.keys().cloned().collect();

        if servers.is_empty() {
            println!("No servers configured. Use /mcp add first.");
            return;
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select server to start")
            .items(&servers)
            .interact_opt();

        match selection {
            Ok(Some(idx)) => do_start(manager, &servers[idx]).await,
            _ => println!("Cancelled."),
        }
    } else {
        do_start(manager, name).await;
    }
}

async fn do_start(manager: &McpManager, name: &str) {
    match manager.start_server(name).await {
        Ok(()) => println!("✅ Started: {}", name),
        Err(e) => println!("❌ Failed to start {}: {}", name, e),
    }
}

async fn stop_server(manager: &McpManager, name: &str) {
    if name.is_empty() {
        let running = manager.running_servers().await;

        if running.is_empty() {
            println!("No servers running.");
            return;
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select server to stop")
            .items(&running)
            .interact_opt();

        match selection {
            Ok(Some(idx)) => do_stop(manager, &running[idx]).await,
            _ => println!("Cancelled."),
        }
    } else {
        do_stop(manager, name).await;
    }
}

async fn do_stop(manager: &McpManager, name: &str) {
    match manager.stop_server(name).await {
        Ok(()) => println!("⏹️  Stopped: {}", name),
        Err(e) => println!("❌ Failed to stop {}: {}", name, e),
    }
}

async fn restart_server(manager: &McpManager, name: &str) {
    if name.is_empty() {
        let running = manager.running_servers().await;
        if running.is_empty() {
            println!("No servers running to restart.");
            return;
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select server to restart")
            .items(&running)
            .interact_opt();

        match selection {
            Ok(Some(idx)) => do_restart(manager, &running[idx]).await,
            _ => println!("Cancelled."),
        }
    } else {
        do_restart(manager, name).await;
    }
}

async fn do_restart(manager: &McpManager, name: &str) {
    println!("🔄 Restarting {}...", name);
    let _ = manager.stop_server(name).await;
    match manager.start_server(name).await {
        Ok(()) => println!("✅ Restarted: {}", name),
        Err(e) => println!("❌ Failed to restart {}: {}", name, e),
    }
}

async fn start_all(manager: &McpManager) {
    let enabled: Vec<_> = manager
        .config()
        .enabled_servers()
        .map(|(n, _)| n.clone())
        .collect();

    if enabled.is_empty() {
        println!("No enabled servers to start.");
        return;
    }

    println!("🔌 Starting {} server(s)...", enabled.len());

    match manager.start_all().await {
        Ok(()) => {
            let running = manager.running_servers().await;
            println!("✅ Running: {}", running.join(", "));
        }
        Err(e) => println!("⚠️  Some servers failed: {}", e),
    }
}

async fn stop_all(manager: &McpManager) {
    let running = manager.running_servers().await;

    if running.is_empty() {
        println!("No servers running.");
        return;
    }

    println!("⏹️  Stopping {} server(s)...", running.len());

    match manager.stop_all().await {
        Ok(()) => println!("✅ All servers stopped"),
        Err(e) => println!("⚠️  Error stopping servers: {}", e),
    }
}

async fn list_tools(manager: &McpManager) {
    let running = manager.running_servers().await;

    if running.is_empty() {
        println!("\n  No MCP servers running.");
        println!("  Use /mcp start to start servers.\n");
        return;
    }

    println!("\n\x1b[1m🔧 MCP Tools\x1b[0m\n");

    let all_tools = manager.list_all_tools().await;

    for (server_name, tools) in all_tools {
        println!(
            "  \x1b[1;36m{}\x1b[0m ({} tools):",
            server_name,
            tools.len()
        );
        for tool in tools {
            let desc = tool.description.as_deref().unwrap_or("");
            println!("    • \x1b[1m{}\x1b[0m", tool.name);
            if !desc.is_empty() {
                let short = if desc.len() > 60 {
                    format!("{}...", &desc[..57])
                } else {
                    desc.to_string()
                };
                println!("      \x1b[2m{}\x1b[0m", short);
            }
        }
        println!();
    }
}

fn add_server_interactive() {
    println!("\n\x1b[1m➕ Add MCP Server\x1b[0m\n");

    // Server name
    let name: String = match Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Server name (e.g., 'github', 'filesystem')")
        .interact_text()
    {
        Ok(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            println!("Cancelled.");
            return;
        }
    };

    // Check if exists
    let config = McpConfig::load_or_default();
    if config.has_server(&name) {
        println!(
            "❌ Server '{}' already exists. Use /mcp remove first.",
            name
        );
        return;
    }

    // Command
    let command: String = match Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Command (e.g., 'npx', 'uvx', 'python')")
        .interact_text()
    {
        Ok(c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => {
            println!("Cancelled.");
            return;
        }
    };

    // Arguments
    let args_str: String = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Arguments (space-separated)")
        .default(String::new())
        .allow_empty(true)
        .interact_text()
        .unwrap_or_default();

    let args: Vec<String> = if args_str.trim().is_empty() {
        vec![]
    } else {
        args_str.split_whitespace().map(String::from).collect()
    };

    // Description
    let description: String = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Description (optional)")
        .default(String::new())
        .allow_empty(true)
        .interact_text()
        .unwrap_or_default();

    // Environment variables
    let mut env = std::collections::HashMap::new();
    if Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Add environment variables?")
        .default(false)
        .interact()
        .unwrap_or(false)
    {
        loop {
            let key: String = Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Env var name (empty to finish)")
                .default(String::new())
                .allow_empty(true)
                .interact_text()
                .unwrap_or_default();

            if key.trim().is_empty() {
                break;
            }

            let value: String = Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Value for {} (use $VAR for env ref)", key))
                .interact_text()
                .unwrap_or_default();

            env.insert(key.trim().to_string(), value);
        }
    }

    // Build entry
    let mut entry = McpServerEntry::new(command).with_args(args);
    if !description.trim().is_empty() {
        entry = entry.with_description(description.trim());
    }
    entry.env = env;

    // Save
    let mut config = McpConfig::load_or_default();
    config.add_server(&name, entry);

    match config.save_default() {
        Ok(()) => {
            println!("\n✅ Added server: \x1b[1m{}\x1b[0m", name);
            println!("   Use \x1b[36m/mcp start {}\x1b[0m to start it\n", name);
        }
        Err(e) => println!("❌ Failed to save config: {}", e),
    }
}

fn remove_server(name: &str) {
    if name.is_empty() {
        let config = McpConfig::load_or_default();
        let servers: Vec<_> = config.servers.keys().cloned().collect();

        if servers.is_empty() {
            println!("No servers configured.");
            return;
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select server to remove")
            .items(&servers)
            .interact_opt();

        match selection {
            Ok(Some(idx)) => do_remove_server(&servers[idx]),
            _ => println!("Cancelled."),
        }
    } else {
        do_remove_server(name);
    }
}

fn do_remove_server(name: &str) {
    let mut config = McpConfig::load_or_default();

    if config.remove_server(name).is_some() {
        match config.save_default() {
            Ok(()) => println!("🗑️  Removed server: {}", name),
            Err(e) => println!("❌ Failed to save: {}", e),
        }
    } else {
        println!("Server not found: {}", name);
    }
}

fn toggle_server(name: &str, enable: bool) {
    let action = if enable { "enable" } else { "disable" };

    if name.is_empty() {
        println!("Usage: /mcp {} <name>", action);
        return;
    }

    let mut config = McpConfig::load_or_default();

    if let Some(entry) = config.servers.get_mut(name) {
        entry.enabled = enable;
        match config.save_default() {
            Ok(()) => {
                let status = if enable {
                    "✓ Enabled"
                } else {
                    "○ Disabled"
                };
                println!("{} server: {}", status, name);
            }
            Err(e) => println!("❌ Failed to save: {}", e),
        }
    } else {
        println!("Server not found: {}", name);
    }
}

// =========================================================================
// Utility functions for testability
// =========================================================================

/// Parse MCP subcommand from arguments.
/// Returns (subcommand, remaining_args).
pub fn parse_mcp_args(args: &str) -> (&str, &str) {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("");
    let subargs = parts.get(1).copied().unwrap_or("");
    (subcommand, subargs)
}

/// Check if a subcommand is valid.
pub fn is_valid_subcommand(cmd: &str) -> bool {
    matches!(
        cmd,
        "" | "help"
            | "list"
            | "ls"
            | "status"
            | "start"
            | "stop"
            | "restart"
            | "start-all"
            | "stop-all"
            | "tools"
            | "add"
            | "remove"
            | "rm"
            | "enable"
            | "disable"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // =========================================================================
    // parse_mcp_args Tests
    // =========================================================================

    #[test]
    fn test_parse_mcp_args_empty() {
        let (cmd, args) = parse_mcp_args("");
        assert_eq!(cmd, "");
        assert_eq!(args, "");
    }

    #[test]
    fn test_parse_mcp_args_command_only() {
        let (cmd, args) = parse_mcp_args("list");
        assert_eq!(cmd, "list");
        assert_eq!(args, "");
    }

    #[test]
    fn test_parse_mcp_args_with_args() {
        let (cmd, args) = parse_mcp_args("start myserver");
        assert_eq!(cmd, "start");
        assert_eq!(args, "myserver");
    }

    #[test]
    fn test_parse_mcp_args_multiple_args() {
        let (cmd, args) = parse_mcp_args("start server1 server2");
        assert_eq!(cmd, "start");
        assert_eq!(args, "server1 server2");
    }

    // =========================================================================
    // is_valid_subcommand Tests
    // =========================================================================

    #[test]
    fn test_is_valid_subcommand_valid() {
        assert!(is_valid_subcommand(""));
        assert!(is_valid_subcommand("help"));
        assert!(is_valid_subcommand("list"));
        assert!(is_valid_subcommand("ls"));
        assert!(is_valid_subcommand("status"));
        assert!(is_valid_subcommand("start"));
        assert!(is_valid_subcommand("stop"));
        assert!(is_valid_subcommand("restart"));
        assert!(is_valid_subcommand("start-all"));
        assert!(is_valid_subcommand("stop-all"));
        assert!(is_valid_subcommand("tools"));
        assert!(is_valid_subcommand("add"));
        assert!(is_valid_subcommand("remove"));
        assert!(is_valid_subcommand("rm"));
        assert!(is_valid_subcommand("enable"));
        assert!(is_valid_subcommand("disable"));
    }

    #[test]
    fn test_is_valid_subcommand_invalid() {
        assert!(!is_valid_subcommand("invalid"));
        assert!(!is_valid_subcommand("LIST")); // case sensitive
        assert!(!is_valid_subcommand("delete"));
        assert!(!is_valid_subcommand("create"));
    }

    // =========================================================================
    // show_help Tests
    // =========================================================================

    #[test]
    fn test_show_help_no_panic() {
        show_help();
    }

    // =========================================================================
    // list_servers Tests
    // =========================================================================

    #[test]
    fn test_list_servers_with_config() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp_servers.json");

        // Create a config with some servers
        let mut config = McpConfig::new();
        config.add_server(
            "test-server",
            McpServerEntry::new("npx")
                .with_args(vec!["-y".to_string(), "test".to_string()])
                .with_description("Test server"),
        );

        let mut disabled = McpServerEntry::new("python");
        disabled.enabled = false;
        config.add_server("disabled-server", disabled);

        config.save_to_path(&path).unwrap();

        // Create manager with this config
        let manager = McpManager::with_config(config);

        // Should not panic
        list_servers(&manager);
    }

    #[test]
    fn test_list_servers_empty() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        list_servers(&manager);
    }

    // =========================================================================
    // McpServerEntry Builder Tests (used in add_server_interactive)
    // =========================================================================

    #[test]
    fn test_mcp_server_entry_builder() {
        let entry = McpServerEntry::new("npx")
            .with_args(vec!["-y".to_string(), "@mcp/server".to_string()])
            .with_env("API_KEY", "secret")
            .with_description("A test server");

        assert_eq!(entry.command, "npx");
        assert_eq!(entry.args.len(), 2);
        assert_eq!(entry.env.get("API_KEY"), Some(&"secret".to_string()));
        assert_eq!(entry.description, Some("A test server".to_string()));
        assert!(entry.enabled);
    }

    #[test]
    fn test_mcp_server_entry_defaults() {
        let entry = McpServerEntry::new("python");

        assert_eq!(entry.command, "python");
        assert!(entry.args.is_empty());
        assert!(entry.env.is_empty());
        assert!(entry.description.is_none());
        assert!(entry.enabled);
    }

    // =========================================================================
    // McpConfig Tests (used in toggle_server, remove_server)
    // =========================================================================

    #[test]
    fn test_toggle_server_enable() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");

        // Create config with disabled server
        let mut config = McpConfig::new();
        let mut entry = McpServerEntry::new("cmd");
        entry.enabled = false;
        config.add_server("test", entry);
        config.save_to_path(&path).unwrap();

        // Verify initial state
        assert!(!config.get_server("test").unwrap().enabled);

        // Enable it
        if let Some(entry) = config.servers.get_mut("test") {
            entry.enabled = true;
        }

        assert!(config.get_server("test").unwrap().enabled);
    }

    #[test]
    fn test_toggle_server_disable() {
        let mut config = McpConfig::new();
        config.add_server("test", McpServerEntry::new("cmd"));

        assert!(config.get_server("test").unwrap().enabled);

        if let Some(entry) = config.servers.get_mut("test") {
            entry.enabled = false;
        }

        assert!(!config.get_server("test").unwrap().enabled);
    }

    #[test]
    fn test_remove_server_logic() {
        let mut config = McpConfig::new();
        config.add_server("to-remove", McpServerEntry::new("cmd"));

        assert!(config.has_server("to-remove"));

        let removed = config.remove_server("to-remove");
        assert!(removed.is_some());
        assert!(!config.has_server("to-remove"));
    }

    #[test]
    fn test_remove_nonexistent_server() {
        let mut config = McpConfig::new();
        let removed = config.remove_server("nonexistent");
        assert!(removed.is_none());
    }

    // =========================================================================
    // Integration-style tests (testing logic flow)
    // =========================================================================

    #[test]
    fn test_toggle_server_empty_name() {
        // Should just print usage, not panic
        toggle_server("", true);
        toggle_server("", false);
    }

    #[test]
    fn test_toggle_server_nonexistent() {
        // Should print "Server not found", not panic
        toggle_server("nonexistent-server-12345", true);
    }

    // =========================================================================
    // Async function tests (require tokio runtime)
    // =========================================================================

    #[tokio::test]
    async fn test_show_status_no_servers() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        show_status(&manager).await;
    }

    #[tokio::test]
    async fn test_start_all_no_enabled() {
        let mut config = McpConfig::new();
        let mut disabled = McpServerEntry::new("cmd");
        disabled.enabled = false;
        config.add_server("disabled", disabled);

        let manager = McpManager::with_config(config);
        start_all(&manager).await;
    }

    #[tokio::test]
    async fn test_stop_all_no_running() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        stop_all(&manager).await;
    }

    #[tokio::test]
    async fn test_list_tools_no_servers() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        list_tools(&manager).await;
    }

    // =========================================================================
    // parse_mcp_args Edge Cases
    // =========================================================================

    #[test]
    fn test_parse_mcp_args_whitespace_only() {
        let (cmd, args) = parse_mcp_args("   ");
        // splitn splits on first space, leaving empty first part
        assert_eq!(cmd, "");
        assert_eq!(args, "  "); // remaining spaces after first split
    }

    #[test]
    fn test_parse_mcp_args_preserves_arg_spaces() {
        let (cmd, args) = parse_mcp_args("start  server  name");
        assert_eq!(cmd, "start");
        assert_eq!(args, " server  name"); // leading space preserved
    }

    #[test]
    fn test_parse_mcp_args_single_space() {
        let (cmd, args) = parse_mcp_args(" ");
        assert_eq!(cmd, "");
        assert_eq!(args, "");
    }

    #[test]
    fn test_parse_mcp_args_tab_separator() {
        let (cmd, args) = parse_mcp_args("list\tserver");
        // tabs are not split on
        assert_eq!(cmd, "list\tserver");
        assert_eq!(args, "");
    }

    // =========================================================================
    // is_valid_subcommand Additional Tests
    // =========================================================================

    #[test]
    fn test_is_valid_subcommand_all_aliases() {
        // Test ls is alias for list
        assert!(is_valid_subcommand("ls"));
        assert!(is_valid_subcommand("list"));
        // Test rm is alias for remove
        assert!(is_valid_subcommand("rm"));
        assert!(is_valid_subcommand("remove"));
    }

    #[test]
    fn test_is_valid_subcommand_similar_but_invalid() {
        assert!(!is_valid_subcommand("start-server"));
        assert!(!is_valid_subcommand("startall"));
        assert!(!is_valid_subcommand("stop-"));
        assert!(!is_valid_subcommand("-start"));
        assert!(!is_valid_subcommand("stat")); // not "status"
    }

    #[test]
    fn test_is_valid_subcommand_with_whitespace() {
        assert!(!is_valid_subcommand(" list"));
        assert!(!is_valid_subcommand("list "));
        assert!(!is_valid_subcommand(" "));
    }

    // =========================================================================
    // list_servers with Various Config States
    // =========================================================================

    #[test]
    fn test_list_servers_with_env_vars() {
        let mut config = McpConfig::new();
        config.add_server(
            "test",
            McpServerEntry::new("npx")
                .with_args(vec!["-y".to_string(), "@mcp/server".to_string()])
                .with_env("API_KEY", "secret")
                .with_env("DEBUG", "true"),
        );

        let manager = McpManager::with_config(config);
        // Just verify no panic - env keys should be displayed
        list_servers(&manager);
    }

    #[test]
    fn test_list_servers_mixed_enabled_disabled() {
        let mut config = McpConfig::new();

        config.add_server("enabled1", McpServerEntry::new("cmd1"));
        config.add_server(
            "enabled2",
            McpServerEntry::new("cmd2").with_description("Has description"),
        );

        let mut disabled = McpServerEntry::new("cmd3");
        disabled.enabled = false;
        config.add_server("disabled1", disabled);

        let manager = McpManager::with_config(config);
        list_servers(&manager);
    }

    #[test]
    fn test_list_servers_long_args() {
        let mut config = McpConfig::new();
        config.add_server(
            "long-args",
            McpServerEntry::new("python")
                .with_args(vec![
                    "-m".to_string(),
                    "mcp_server".to_string(),
                    "--config".to_string(),
                    "/very/long/path/to/config.json".to_string(),
                    "--verbose".to_string(),
                    "--debug".to_string(),
                ])
                .with_description("Server with many arguments"),
        );

        let manager = McpManager::with_config(config);
        list_servers(&manager);
    }

    // =========================================================================
    // show_status Edge Cases
    // =========================================================================

    #[tokio::test]
    async fn test_show_status_with_enabled_servers_not_running() {
        let mut config = McpConfig::new();
        config.add_server("server1", McpServerEntry::new("cmd1"));
        config.add_server("server2", McpServerEntry::new("cmd2"));

        let manager = McpManager::with_config(config);
        // Servers configured but not started
        show_status(&manager).await;
    }

    #[tokio::test]
    async fn test_show_status_only_disabled_servers() {
        let mut config = McpConfig::new();

        let mut disabled = McpServerEntry::new("cmd");
        disabled.enabled = false;
        config.add_server("disabled", disabled);

        let manager = McpManager::with_config(config);
        show_status(&manager).await;
    }

    // =========================================================================
    // do_start / do_stop / do_restart Tests
    // =========================================================================

    #[tokio::test]
    async fn test_do_start_nonexistent_server() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        // Should print error, not panic
        do_start(&manager, "nonexistent").await;
    }

    #[tokio::test]
    async fn test_do_stop_nonexistent_server() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        // Should print error, not panic
        do_stop(&manager, "nonexistent").await;
    }

    #[tokio::test]
    async fn test_do_restart_nonexistent_server() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        // Should print error, not panic
        do_restart(&manager, "nonexistent").await;
    }

    #[tokio::test]
    async fn test_restart_server_empty_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        // Empty name with no running servers should show appropriate message
        restart_server(&manager, "").await;
    }

    // =========================================================================
    // start_all / stop_all Edge Cases
    // =========================================================================

    #[tokio::test]
    async fn test_start_all_all_disabled() {
        let mut config = McpConfig::new();

        let mut d1 = McpServerEntry::new("cmd1");
        d1.enabled = false;
        config.add_server("d1", d1);

        let mut d2 = McpServerEntry::new("cmd2");
        d2.enabled = false;
        config.add_server("d2", d2);

        let manager = McpManager::with_config(config);
        start_all(&manager).await;
    }

    #[tokio::test]
    async fn test_stop_all_none_running() {
        let mut config = McpConfig::new();
        config.add_server("s1", McpServerEntry::new("cmd"));

        let manager = McpManager::with_config(config);
        stop_all(&manager).await;
    }

    // =========================================================================
    // toggle_server Tests
    // =========================================================================

    #[test]
    fn test_toggle_server_enable_empty_name() {
        toggle_server("", true);
    }

    #[test]
    fn test_toggle_server_disable_empty_name() {
        toggle_server("", false);
    }

    #[test]
    fn test_toggle_server_nonexistent_enable() {
        toggle_server("definitely_not_a_server_12345", true);
    }

    #[test]
    fn test_toggle_server_nonexistent_disable() {
        toggle_server("definitely_not_a_server_67890", false);
    }

    // =========================================================================
    // do_remove_server Tests
    // =========================================================================

    #[test]
    fn test_do_remove_server_nonexistent() {
        // Should print "Server not found", not panic
        do_remove_server("nonexistent_server_xyz");
    }

    // =========================================================================
    // handle() Dispatch Tests (integration-style)
    // =========================================================================

    #[tokio::test]
    async fn test_handle_empty_shows_help() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "").await;
    }

    #[tokio::test]
    async fn test_handle_help_command() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "help").await;
    }

    #[tokio::test]
    async fn test_handle_list_command() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "list").await;
    }

    #[tokio::test]
    async fn test_handle_ls_alias() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "ls").await;
    }

    #[tokio::test]
    async fn test_handle_status_command() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "status").await;
    }

    #[tokio::test]
    async fn test_handle_tools_command() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "tools").await;
    }

    #[tokio::test]
    async fn test_handle_start_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "start nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_stop_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "stop nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_restart_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "restart nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_start_all() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "start-all").await;
    }

    #[tokio::test]
    async fn test_handle_stop_all() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "stop-all").await;
    }

    #[tokio::test]
    async fn test_handle_remove_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "remove nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_rm_alias() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "rm nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_enable_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "enable nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_disable_with_name() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "disable nonexistent").await;
    }

    #[tokio::test]
    async fn test_handle_unknown_command() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "unknown_cmd").await;
    }

    #[tokio::test]
    async fn test_handle_unknown_command_with_args() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "badcmd some args here").await;
    }

    // =========================================================================
    // list_tools Edge Cases
    // =========================================================================

    #[tokio::test]
    async fn test_list_tools_with_config_but_no_running() {
        let mut config = McpConfig::new();
        config.add_server("test", McpServerEntry::new("cmd"));

        let manager = McpManager::with_config(config);
        list_tools(&manager).await;
    }

    // =========================================================================
    // Server Entry Comprehensive Builder Tests
    // =========================================================================

    #[test]
    fn test_mcp_server_entry_empty_args() {
        let entry = McpServerEntry::new("cmd").with_args(vec![]);
        assert!(entry.args.is_empty());
    }

    #[test]
    fn test_mcp_server_entry_empty_description() {
        let entry = McpServerEntry::new("cmd").with_description("");
        assert_eq!(entry.description, Some("".to_string()));
    }

    #[test]
    fn test_mcp_server_entry_unicode_values() {
        let entry = McpServerEntry::new("コマンド")
            .with_args(vec!["引数".to_string()])
            .with_env("キー", "値")
            .with_description("説明文");

        assert_eq!(entry.command, "コマンド");
        assert_eq!(entry.args[0], "引数");
        assert_eq!(entry.env.get("キー"), Some(&"値".to_string()));
        assert_eq!(entry.description, Some("説明文".to_string()));
    }

    #[test]
    fn test_mcp_server_entry_special_chars() {
        let entry = McpServerEntry::new("cmd")
            .with_args(vec!["--flag=value with spaces".to_string()])
            .with_env("KEY", "value\twith\ttabs")
            .with_description("Line1\nLine2");

        assert!(entry.args[0].contains("spaces"));
        assert!(entry.env.get("KEY").unwrap().contains('\t'));
        assert!(entry.description.as_ref().unwrap().contains('\n'));
    }

    // =========================================================================
    // McpConfig Additional Tests
    // =========================================================================

    #[test]
    fn test_config_enabled_servers_empty() {
        let config = McpConfig::new();
        let enabled: Vec<_> = config.enabled_servers().collect();
        assert!(enabled.is_empty());
    }

    #[test]
    fn test_config_get_server_returns_correct_entry() {
        let mut config = McpConfig::new();
        config.add_server(
            "myserver",
            McpServerEntry::new("mycmd")
                .with_args(vec!["arg1".to_string()])
                .with_description("My server"),
        );

        let server = config.get_server("myserver").unwrap();
        assert_eq!(server.command, "mycmd");
        assert_eq!(server.args.len(), 1);
        assert_eq!(server.description, Some("My server".to_string()));
    }

    #[test]
    fn test_config_has_server_case_sensitive() {
        let mut config = McpConfig::new();
        config.add_server("MyServer", McpServerEntry::new("cmd"));

        assert!(config.has_server("MyServer"));
        assert!(!config.has_server("myserver"));
        assert!(!config.has_server("MYSERVER"));
    }

    #[test]
    fn test_config_add_server_overwrites() {
        let mut config = McpConfig::new();
        config.add_server("test", McpServerEntry::new("cmd1"));
        config.add_server("test", McpServerEntry::new("cmd2"));

        let server = config.get_server("test").unwrap();
        assert_eq!(server.command, "cmd2");
        assert_eq!(config.servers.len(), 1);
    }

    // =========================================================================
    // handle() with Configured Servers
    // =========================================================================

    #[tokio::test]
    async fn test_handle_list_with_servers() {
        let mut config = McpConfig::new();
        config.add_server(
            "server1",
            McpServerEntry::new("npx")
                .with_args(vec!["-y".to_string(), "@mcp/test".to_string()])
                .with_description("Test MCP server"),
        );

        let mut disabled = McpServerEntry::new("python");
        disabled.enabled = false;
        config.add_server("server2", disabled);

        let manager = McpManager::with_config(config);
        handle(&manager, "list").await;
    }

    #[tokio::test]
    async fn test_handle_status_with_configured_servers() {
        let mut config = McpConfig::new();
        config.add_server("s1", McpServerEntry::new("cmd1"));
        config.add_server("s2", McpServerEntry::new("cmd2"));

        let manager = McpManager::with_config(config);
        handle(&manager, "status").await;
    }

    // =========================================================================
    // Command Parsing with Various Inputs
    // =========================================================================

    #[test]
    fn test_parse_mcp_args_long_command() {
        let (cmd, args) = parse_mcp_args(
            "start this-is-a-very-long-server-name-that-might-be-too-long-for-normal-use",
        );
        assert_eq!(cmd, "start");
        assert_eq!(
            args,
            "this-is-a-very-long-server-name-that-might-be-too-long-for-normal-use"
        );
    }

    #[test]
    fn test_parse_mcp_args_special_chars_in_args() {
        let (cmd, args) = parse_mcp_args("start server@host:8080");
        assert_eq!(cmd, "start");
        assert_eq!(args, "server@host:8080");
    }

    #[test]
    fn test_parse_mcp_args_quoted_content() {
        // Quotes are not specially handled - just passed through
        let (cmd, args) = parse_mcp_args("start \"my server\"");
        assert_eq!(cmd, "start");
        assert_eq!(args, "\"my server\"");
    }

    // =========================================================================
    // Ensure No Panics on Edge Cases
    // =========================================================================

    #[tokio::test]
    async fn test_no_panic_on_empty_config_all_commands() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);

        // Test all commands with empty config
        handle(&manager, "").await;
        handle(&manager, "help").await;
        handle(&manager, "list").await;
        handle(&manager, "ls").await;
        handle(&manager, "status").await;
        handle(&manager, "tools").await;
        handle(&manager, "start-all").await;
        handle(&manager, "stop-all").await;
        handle(&manager, "start test").await;
        handle(&manager, "stop test").await;
        handle(&manager, "restart test").await;
        handle(&manager, "remove test").await;
        handle(&manager, "rm test").await;
        handle(&manager, "enable test").await;
        handle(&manager, "disable test").await;
    }

    #[test]
    fn test_no_panic_on_toggle_with_empty_name() {
        toggle_server("", true);
        toggle_server("", false);
    }

    #[test]
    fn test_no_panic_do_remove_empty_name() {
        do_remove_server("");
    }

    // =========================================================================
    // Subcommand Validation Exhaustive
    // =========================================================================

    #[test]
    fn test_is_valid_subcommand_exhaustive_invalid() {
        let invalid_commands = [
            "HELP",
            "Help",
            "LIST",
            "List",
            "STATUS",
            "Status",
            "START",
            "STOP",
            "RESTART",
            "ADD",
            "REMOVE",
            "ENABLE",
            "DISABLE",
            "TOOLS",
            "start_all",
            "stop_all",
            "startAll",
            "stopAll",
            "liist",
            "staart",
            "stopp",
            "",    // empty IS valid (shows help)
            "   ", // whitespace NOT valid
        ];

        // Note: "" is valid (maps to help), so we skip it
        for cmd in &invalid_commands[..invalid_commands.len() - 2] {
            if *cmd != "" {
                assert!(
                    !is_valid_subcommand(cmd) || *cmd == "",
                    "'{}' should be invalid",
                    cmd
                );
            }
        }
    }

    // =========================================================================
    // enabled_servers Iterator Tests
    // =========================================================================

    #[test]
    fn test_enabled_servers_returns_only_enabled() {
        let mut config = McpConfig::new();

        config.add_server("e1", McpServerEntry::new("cmd1"));
        config.add_server("e2", McpServerEntry::new("cmd2"));

        let mut d1 = McpServerEntry::new("cmd3");
        d1.enabled = false;
        config.add_server("d1", d1);

        let enabled: Vec<_> = config.enabled_servers().collect();
        assert_eq!(enabled.len(), 2);

        let names: Vec<_> = enabled.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"e1"));
        assert!(names.contains(&"e2"));
        assert!(!names.contains(&"d1"));
    }

    #[test]
    fn test_enabled_servers_all_disabled() {
        let mut config = McpConfig::new();

        for i in 0..3 {
            let mut entry = McpServerEntry::new("cmd");
            entry.enabled = false;
            config.add_server(&format!("server{}", i), entry);
        }

        let enabled: Vec<_> = config.enabled_servers().collect();
        assert!(enabled.is_empty());
    }

    #[test]
    fn test_enabled_servers_all_enabled() {
        let mut config = McpConfig::new();

        for i in 0..3 {
            config.add_server(&format!("server{}", i), McpServerEntry::new("cmd"));
        }

        let enabled: Vec<_> = config.enabled_servers().collect();
        assert_eq!(enabled.len(), 3);
    }

    // =========================================================================
    // remove_server Returns Removed Entry
    // =========================================================================

    #[test]
    fn test_remove_server_returns_entry() {
        let mut config = McpConfig::new();
        config.add_server(
            "test",
            McpServerEntry::new("original_cmd")
                .with_args(vec!["arg1".to_string()])
                .with_description("My desc"),
        );

        let removed = config.remove_server("test");
        assert!(removed.is_some());

        let entry = removed.unwrap();
        assert_eq!(entry.command, "original_cmd");
        assert_eq!(entry.args, vec!["arg1"]);
        assert_eq!(entry.description, Some("My desc".to_string()));
    }

    // =========================================================================
    // Multiple Environment Variables
    // =========================================================================

    #[test]
    fn test_mcp_server_entry_multiple_env_vars() {
        let entry = McpServerEntry::new("cmd")
            .with_env("VAR1", "value1")
            .with_env("VAR2", "value2")
            .with_env("VAR3", "value3");

        assert_eq!(entry.env.len(), 3);
        assert_eq!(entry.env.get("VAR1"), Some(&"value1".to_string()));
        assert_eq!(entry.env.get("VAR2"), Some(&"value2".to_string()));
        assert_eq!(entry.env.get("VAR3"), Some(&"value3".to_string()));
    }

    #[test]
    fn test_mcp_server_entry_env_var_overwrite() {
        let entry = McpServerEntry::new("cmd")
            .with_env("KEY", "first")
            .with_env("KEY", "second");

        assert_eq!(entry.env.len(), 1);
        assert_eq!(entry.env.get("KEY"), Some(&"second".to_string()));
    }

    #[test]
    fn test_mcp_server_entry_env_empty_key_value() {
        let entry = McpServerEntry::new("cmd")
            .with_env("", "value")
            .with_env("key", "");

        assert_eq!(entry.env.get(""), Some(&"value".to_string()));
        assert_eq!(entry.env.get("key"), Some(&"".to_string()));
    }

    // =========================================================================
    // handle() Argument Spacing Variations
    // =========================================================================

    #[tokio::test]
    async fn test_handle_extra_spaces_between_cmd_and_arg() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        // Extra spaces preserved in args (not trimmed)
        handle(&manager, "start  server").await;
    }

    #[tokio::test]
    async fn test_handle_trailing_spaces() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "list   ").await;
    }

    #[tokio::test]
    async fn test_handle_arg_with_equals() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);
        handle(&manager, "enable name=value").await;
    }

    // =========================================================================
    // Config Persistence Round-Trip
    // =========================================================================

    #[test]
    fn test_config_save_and_load_roundtrip() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test_mcp.json");

        let mut config = McpConfig::new();
        config.add_server(
            "server1",
            McpServerEntry::new("npx")
                .with_args(vec!["-y".to_string(), "@mcp/test".to_string()])
                .with_env("API_KEY", "secret123")
                .with_description("Test server"),
        );

        let mut disabled = McpServerEntry::new("python");
        disabled.enabled = false;
        config.add_server("server2", disabled);

        config.save_to_path(&path).unwrap();

        let loaded = McpConfig::load_from_path(&path).unwrap();

        assert_eq!(loaded.servers.len(), 2);

        let s1 = loaded.get_server("server1").unwrap();
        assert_eq!(s1.command, "npx");
        assert_eq!(s1.args, vec!["-y", "@mcp/test"]);
        assert_eq!(s1.env.get("API_KEY"), Some(&"secret123".to_string()));
        assert_eq!(s1.description, Some("Test server".to_string()));
        assert!(s1.enabled);

        let s2 = loaded.get_server("server2").unwrap();
        assert_eq!(s2.command, "python");
        assert!(!s2.enabled);
    }

    #[test]
    fn test_config_load_nonexistent_returns_error() {
        use std::path::Path;
        let result = McpConfig::load_from_path(Path::new("/nonexistent/path/mcp.json"));
        assert!(result.is_err());
    }

    // =========================================================================
    // parse_mcp_args Boundary Cases
    // =========================================================================

    #[test]
    fn test_parse_mcp_args_newline_in_args() {
        let (cmd, args) = parse_mcp_args("start server\nname");
        assert_eq!(cmd, "start");
        assert_eq!(args, "server\nname");
    }

    #[test]
    fn test_parse_mcp_args_unicode_command() {
        let (cmd, args) = parse_mcp_args("日本語 argument");
        assert_eq!(cmd, "日本語");
        assert_eq!(args, "argument");
    }

    #[test]
    fn test_parse_mcp_args_only_spaces_after_command() {
        let (cmd, args) = parse_mcp_args("list    ");
        assert_eq!(cmd, "list");
        assert_eq!(args, "   ");
    }

    // =========================================================================
    // Server Name Edge Cases
    // =========================================================================

    #[test]
    fn test_server_names_with_special_chars() {
        let mut config = McpConfig::new();

        config.add_server("server-with-dashes", McpServerEntry::new("cmd"));
        config.add_server("server_with_underscores", McpServerEntry::new("cmd"));
        config.add_server("server.with.dots", McpServerEntry::new("cmd"));
        config.add_server("server123", McpServerEntry::new("cmd"));

        assert!(config.has_server("server-with-dashes"));
        assert!(config.has_server("server_with_underscores"));
        assert!(config.has_server("server.with.dots"));
        assert!(config.has_server("server123"));
    }

    #[test]
    fn test_server_name_empty_string() {
        let mut config = McpConfig::new();
        config.add_server("", McpServerEntry::new("cmd"));

        // Empty string is technically a valid key
        assert!(config.has_server(""));
    }

    // =========================================================================
    // Chained Builder Methods
    // =========================================================================

    #[test]
    fn test_mcp_server_entry_full_chain() {
        let entry = McpServerEntry::new("node")
            .with_args(vec![
                "/path/to/script.js".to_string(),
                "--port".to_string(),
                "3000".to_string(),
            ])
            .with_env("NODE_ENV", "production")
            .with_env("DEBUG", "mcp:*")
            .with_description("Production MCP server");

        assert_eq!(entry.command, "node");
        assert_eq!(entry.args.len(), 3);
        assert_eq!(entry.env.len(), 2);
        assert!(entry.enabled);
        assert!(entry.description.is_some());
    }

    // =========================================================================
    // handle() with Matching Server Exists
    // =========================================================================

    #[tokio::test]
    async fn test_handle_enable_existing_server() {
        let mut config = McpConfig::new();
        let mut entry = McpServerEntry::new("cmd");
        entry.enabled = false;
        config.add_server("myserver", entry);

        let manager = McpManager::with_config(config);
        // Note: toggle_server reads/writes from global config, not manager's config
        // This tests the code path, not the actual state change
        handle(&manager, "enable myserver").await;
    }

    #[tokio::test]
    async fn test_handle_disable_existing_server() {
        let mut config = McpConfig::new();
        config.add_server("myserver", McpServerEntry::new("cmd"));

        let manager = McpManager::with_config(config);
        handle(&manager, "disable myserver").await;
    }

    // =========================================================================
    // Concurrent Access Safety (basic)
    // =========================================================================

    #[tokio::test]
    async fn test_multiple_status_calls_no_panic() {
        let config = McpConfig::new();
        let manager = McpManager::with_config(config);

        // Multiple concurrent calls should not panic
        let h1 = show_status(&manager);
        let h2 = show_status(&manager);
        let h3 = show_status(&manager);

        tokio::join!(h1, h2, h3);
    }

    // =========================================================================
    // Subcommand Dispatch Correctness
    // =========================================================================

    #[test]
    fn test_subcommand_alias_consistency() {
        // Verify all documented aliases are valid
        assert!(is_valid_subcommand("list"));
        assert!(is_valid_subcommand("ls")); // alias

        assert!(is_valid_subcommand("remove"));
        assert!(is_valid_subcommand("rm")); // alias

        // Verify help variants
        assert!(is_valid_subcommand(""));
        assert!(is_valid_subcommand("help"));
    }

    #[test]
    fn test_all_valid_subcommands_count() {
        let valid = [
            "",
            "help",
            "list",
            "ls",
            "status",
            "start",
            "stop",
            "restart",
            "start-all",
            "stop-all",
            "tools",
            "add",
            "remove",
            "rm",
            "enable",
            "disable",
        ];

        for cmd in valid {
            assert!(is_valid_subcommand(cmd), "Expected '{}' to be valid", cmd);
        }

        // Total unique commands (excluding aliases for counting)
        assert_eq!(valid.len(), 16);
    }

    // =========================================================================
    // Args Parsing for Complex Inputs
    // =========================================================================

    #[test]
    fn test_parse_mcp_args_path_like_arg() {
        let (cmd, args) = parse_mcp_args("start /path/to/server");
        assert_eq!(cmd, "start");
        assert_eq!(args, "/path/to/server");
    }

    #[test]
    fn test_parse_mcp_args_url_like_arg() {
        let (cmd, args) = parse_mcp_args("start http://localhost:8080");
        assert_eq!(cmd, "start");
        assert_eq!(args, "http://localhost:8080");
    }

    #[test]
    fn test_parse_mcp_args_json_like_arg() {
        let (cmd, args) = parse_mcp_args("start {\"key\":\"value\"}");
        assert_eq!(cmd, "start");
        assert_eq!(args, "{\"key\":\"value\"}");
    }
}
