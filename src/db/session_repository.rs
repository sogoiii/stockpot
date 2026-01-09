//! SQLite-backed session persistence.
//!
//! This repository encapsulates all SQL operations for:
//! - Creating sessions and appending messages (`sessions`, `messages`)
//! - Tracking per-interface active sessions (`active_sessions`)
//! - Recording sub-agent invocations for observability (`sub_agent_invocations`)

use crate::db::Database;
use rusqlite::OptionalExtension;

/// Session persistence operations.
pub struct SessionRepository<'a> {
    db: &'a Database,
}

impl<'a> SessionRepository<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Create a new session for an agent, returns session_id
    pub fn create_session(&self, agent_name: &str) -> Result<i64, rusqlite::Error> {
        let name = format!("{}-{}", agent_name, uuid::Uuid::new_v4());

        self.db.conn().execute(
            "INSERT INTO sessions (name, agent_name, created_at, updated_at)
             VALUES (?, ?, unixepoch(), unixepoch())",
            rusqlite::params![name, agent_name],
        )?;

        Ok(self.db.conn().last_insert_rowid())
    }

    /// Add a message to a session (stores serialized ModelRequest as JSON)
    pub fn add_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
        token_count: Option<i64>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.db.conn();
        conn.execute_batch("BEGIN")?;

        let result = (|| {
            conn.execute(
                "INSERT INTO messages (session_id, role, content, token_count, created_at)
                 VALUES (?, ?, ?, ?, unixepoch())",
                rusqlite::params![session_id, role, content, token_count],
            )?;
            let message_id = conn.last_insert_rowid();

            conn.execute(
                "UPDATE sessions SET updated_at = unixepoch() WHERE id = ?",
                [session_id],
            )?;

            Ok(message_id)
        })();

        match result {
            Ok(message_id) => {
                conn.execute_batch("COMMIT")?;
                Ok(message_id)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Get all messages for a session
    pub fn get_messages(
        &self,
        session_id: i64,
    ) -> Result<Vec<crate::db::Message>, rusqlite::Error> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, session_id, role, content, token_count, created_at
             FROM messages
             WHERE session_id = ?
             ORDER BY id",
        )?;

        let rows = stmt.query_map([session_id], |row| {
            Ok(crate::db::Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        rows.collect()
    }

    /// Copy all messages from one session to a new session for a different agent
    /// Returns the new session_id
    pub fn copy_history_to_agent(
        &self,
        from_session_id: i64,
        to_agent: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.db.conn();
        conn.execute_batch("BEGIN")?;

        let result = (|| {
            let to_session_id = self.create_session(to_agent)?;

            conn.execute(
                "INSERT INTO messages (session_id, role, content, token_count, created_at)
                 SELECT ?, role, content, token_count, created_at
                 FROM messages
                 WHERE session_id = ?
                 ORDER BY id",
                rusqlite::params![to_session_id, from_session_id],
            )?;

            conn.execute(
                "UPDATE sessions SET updated_at = unixepoch() WHERE id = ?",
                [to_session_id],
            )?;

            Ok(to_session_id)
        })();

        match result {
            Ok(to_session_id) => {
                conn.execute_batch("COMMIT")?;
                Ok(to_session_id)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Delete a session and its messages (CASCADE handles messages)
    pub fn delete_session(&self, session_id: i64) -> Result<(), rusqlite::Error> {
        self.db
            .conn()
            .execute("DELETE FROM sessions WHERE id = ?", [session_id])?;
        Ok(())
    }

    /// Get or create active session for an interface
    pub fn get_active_session(&self, interface: &str) -> Result<Option<i64>, rusqlite::Error> {
        let session_id: Option<Option<i64>> = self
            .db
            .conn()
            .query_row(
                "SELECT session_id FROM active_sessions WHERE interface = ?",
                [interface],
                |row| row.get(0),
            )
            .optional()?;

        Ok(session_id.flatten())
    }

    /// Set the active session for an interface
    pub fn set_active_session(
        &self,
        interface: &str,
        agent_name: &str,
        session_id: i64,
    ) -> Result<(), rusqlite::Error> {
        self.db.conn().execute(
            "INSERT INTO active_sessions (interface, agent_name, session_id, created_at)
             VALUES (?, ?, ?, unixepoch())
             ON CONFLICT(interface) DO UPDATE SET
                agent_name = excluded.agent_name,
                session_id = excluded.session_id,
                created_at = excluded.created_at",
            rusqlite::params![interface, agent_name, session_id],
        )?;
        Ok(())
    }

    /// Clear all active sessions (called on startup)
    pub fn clear_active_sessions(&self) -> Result<(), rusqlite::Error> {
        self.db.conn().execute("DELETE FROM active_sessions", [])?;
        Ok(())
    }

    /// Record a sub-agent invocation
    pub fn record_sub_agent_invocation(
        &self,
        parent_session_id: Option<i64>,
        agent_name: &str,
        prompt: &str,
        response: Option<&str>,
        duration_ms: Option<i64>,
    ) -> Result<i64, rusqlite::Error> {
        self.db.conn().execute(
            "INSERT INTO sub_agent_invocations (
                parent_session_id,
                agent_name,
                prompt,
                response,
                duration_ms,
                created_at
             ) VALUES (?, ?, ?, ?, ?, unixepoch())",
            rusqlite::params![parent_session_id, agent_name, prompt, response, duration_ms],
        )?;

        Ok(self.db.conn().last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for session repository.
    //!
    //! Coverage:
    //! - Session CRUD operations
    //! - Message management
    //! - Active session tracking
    //! - Sub-agent invocation recording
    //! - History copying between agents

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
    // Task 3.1: Session CRUD Tests
    // =========================================================================

    #[test]
    fn test_create_session_returns_id() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id = repo.create_session("stockpot").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_create_session_unique_ids() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id1 = repo.create_session("stockpot").unwrap();
        let id2 = repo.create_session("stockpot").unwrap();

        assert_ne!(id1, id2, "Different sessions should have different IDs");
    }

    #[test]
    fn test_create_session_different_agents() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id1 = repo.create_session("stockpot").unwrap();
        let id2 = repo.create_session("explore").unwrap();
        let id3 = repo.create_session("planning").unwrap();

        assert!(id1 > 0);
        assert!(id2 > 0);
        assert!(id3 > 0);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_add_message_to_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let msg_id = repo
            .add_message(session_id, "user", "Hello!", Some(5))
            .unwrap();

        assert!(msg_id > 0);
    }

    #[test]
    fn test_add_multiple_messages() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let msg1 = repo
            .add_message(session_id, "user", "Hello!", None)
            .unwrap();
        let msg2 = repo
            .add_message(session_id, "assistant", "Hi there!", None)
            .unwrap();
        let msg3 = repo
            .add_message(session_id, "user", "How are you?", None)
            .unwrap();

        assert!(msg1 < msg2);
        assert!(msg2 < msg3);
    }

    #[test]
    fn test_get_messages_empty_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let messages = repo.get_messages(session_id).unwrap();

        assert!(messages.is_empty());
    }

    #[test]
    fn test_get_messages_ordered_by_id() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "First", None).unwrap();
        repo.add_message(session_id, "assistant", "Second", None)
            .unwrap();
        repo.add_message(session_id, "user", "Third", None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "First");
        assert_eq!(messages[1].content, "Second");
        assert_eq!(messages[2].content, "Third");
    }

    #[test]
    fn test_get_messages_preserves_role() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "User message", None)
            .unwrap();
        repo.add_message(session_id, "assistant", "Assistant message", None)
            .unwrap();
        repo.add_message(session_id, "system", "System message", None)
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();

        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "system");
    }

    #[test]
    fn test_get_messages_preserves_token_count() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "Short", Some(5))
            .unwrap();
        repo.add_message(session_id, "assistant", "Long message", Some(100))
            .unwrap();
        repo.add_message(session_id, "user", "No count", None)
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();

        assert_eq!(messages[0].token_count, Some(5));
        assert_eq!(messages[1].token_count, Some(100));
        assert_eq!(messages[2].token_count, None);
    }

    #[test]
    fn test_delete_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "Hello", None).unwrap();

        repo.delete_session(session_id).unwrap();

        // Messages should also be deleted (CASCADE)
        let messages = repo.get_messages(session_id).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_delete_nonexistent_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Should not error when deleting non-existent session
        repo.delete_session(99999).unwrap();
    }

    // =========================================================================
    // Task 3.2: Active Session Tests
    // =========================================================================

    #[test]
    fn test_set_active_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.set_active_session("cli", "stockpot", session_id)
            .unwrap();

        let active = repo.get_active_session("cli").unwrap();
        assert_eq!(active, Some(session_id));
    }

    #[test]
    fn test_get_active_session_none() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let active = repo.get_active_session("cli").unwrap();
        assert!(active.is_none());
    }

    #[test]
    fn test_set_active_session_overwrites() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session1 = repo.create_session("stockpot").unwrap();
        let session2 = repo.create_session("explore").unwrap();

        repo.set_active_session("cli", "stockpot", session1)
            .unwrap();
        repo.set_active_session("cli", "explore", session2).unwrap();

        let active = repo.get_active_session("cli").unwrap();
        assert_eq!(active, Some(session2));
    }

    #[test]
    fn test_multiple_interfaces_independent() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let cli_session = repo.create_session("stockpot").unwrap();
        let gui_session = repo.create_session("stockpot").unwrap();

        repo.set_active_session("cli", "stockpot", cli_session)
            .unwrap();
        repo.set_active_session("gui", "stockpot", gui_session)
            .unwrap();

        assert_eq!(repo.get_active_session("cli").unwrap(), Some(cli_session));
        assert_eq!(repo.get_active_session("gui").unwrap(), Some(gui_session));
    }

    #[test]
    fn test_clear_active_sessions() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session1 = repo.create_session("stockpot").unwrap();
        let session2 = repo.create_session("stockpot").unwrap();

        repo.set_active_session("cli", "stockpot", session1)
            .unwrap();
        repo.set_active_session("gui", "stockpot", session2)
            .unwrap();

        repo.clear_active_sessions().unwrap();

        assert!(repo.get_active_session("cli").unwrap().is_none());
        assert!(repo.get_active_session("gui").unwrap().is_none());
    }

    #[test]
    fn test_clear_active_sessions_empty() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Should not error when clearing empty table
        repo.clear_active_sessions().unwrap();
    }

    // =========================================================================
    // Task 3.3: Sub-Agent Invocation Tests
    // =========================================================================

    #[test]
    fn test_record_sub_agent_invocation() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let invocation_id = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "explore",
                "Find all Rust files",
                Some("Found 42 files"),
                Some(150),
            )
            .unwrap();

        assert!(invocation_id > 0);
    }

    #[test]
    fn test_record_sub_agent_invocation_without_parent() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let invocation_id = repo
            .record_sub_agent_invocation(
                None, // No parent session
                "explore",
                "Standalone query",
                Some("Result"),
                None,
            )
            .unwrap();

        assert!(invocation_id > 0);
    }

    #[test]
    fn test_record_sub_agent_invocation_without_response() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let invocation_id = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "planning",
                "Create a plan",
                None, // No response yet
                None,
            )
            .unwrap();

        assert!(invocation_id > 0);
    }

    #[test]
    fn test_record_multiple_sub_agent_invocations() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();

        let id1 = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "explore",
                "Find files",
                Some("Found files"),
                Some(100),
            )
            .unwrap();

        let id2 = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "rust-reviewer",
                "Review code",
                Some("LGTM"),
                Some(500),
            )
            .unwrap();

        assert!(id1 < id2);
    }

    // =========================================================================
    // Task 3.4: History Copy Tests
    // =========================================================================

    #[test]
    fn test_copy_history_to_agent() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Create source session with messages
        let source_session = repo.create_session("stockpot").unwrap();
        repo.add_message(source_session, "user", "Hello", Some(5))
            .unwrap();
        repo.add_message(source_session, "assistant", "Hi!", Some(3))
            .unwrap();

        // Copy to new agent
        let new_session = repo
            .copy_history_to_agent(source_session, "explore")
            .unwrap();

        // Verify new session has copied messages
        let messages = repo.get_messages(new_session).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].content, "Hi!");
    }

    #[test]
    fn test_copy_history_preserves_roles() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source_session = repo.create_session("stockpot").unwrap();
        repo.add_message(source_session, "user", "User msg", None)
            .unwrap();
        repo.add_message(source_session, "assistant", "Assistant msg", None)
            .unwrap();

        let new_session = repo
            .copy_history_to_agent(source_session, "planning")
            .unwrap();

        let messages = repo.get_messages(new_session).unwrap();
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn test_copy_history_empty_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source_session = repo.create_session("stockpot").unwrap();
        // No messages added

        let new_session = repo
            .copy_history_to_agent(source_session, "explore")
            .unwrap();

        let messages = repo.get_messages(new_session).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_copy_history_different_session_ids() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source_session = repo.create_session("stockpot").unwrap();
        repo.add_message(source_session, "user", "Test", None)
            .unwrap();

        let new_session = repo
            .copy_history_to_agent(source_session, "explore")
            .unwrap();

        assert_ne!(source_session, new_session);
    }

    // =========================================================================
    // Message Field Tests
    // =========================================================================

    #[test]
    fn test_message_has_created_at() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "Test", None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert!(messages[0].created_at > 0, "created_at should be set");
    }

    #[test]
    fn test_message_session_id_matches() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "Test", None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].session_id, session_id);
    }

    // =========================================================================
    // Edge Cases: Content and Character Handling
    // =========================================================================

    #[test]
    fn test_message_with_unicode_content() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let content = "Hello 世界! 🎉 Привет мир! مرحبا";
        repo.add_message(session_id, "user", content, None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].content, content);
    }

    #[test]
    fn test_message_with_special_characters() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let content = r#"Special chars: "quotes" 'apostrophe' \backslash\ <tag> & ampersand"#;
        repo.add_message(session_id, "user", content, None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].content, content);
    }

    #[test]
    fn test_message_with_multiline_content() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let content = "Line 1\nLine 2\r\nLine 3\n\tTabbed line\n    Spaced line";
        repo.add_message(session_id, "user", content, None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].content, content);
    }

    #[test]
    fn test_message_with_empty_content() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "", None).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].content, "");
    }

    #[test]
    fn test_message_with_very_large_content() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let large_content = "x".repeat(100_000);
        repo.add_message(session_id, "user", &large_content, None)
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].content.len(), 100_000);
    }

    #[test]
    fn test_session_with_unicode_agent_name() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Agent names with unicode should work
        let id = repo.create_session("探索者").unwrap();
        assert!(id > 0);
    }

    // =========================================================================
    // Foreign Key and Constraint Tests
    // =========================================================================

    #[test]
    fn test_add_message_to_nonexistent_session_fails() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Session ID 99999 doesn't exist
        let result = repo.add_message(99999, "user", "Hello", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_active_session_invalid_interface_fails() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        // "invalid" is not in CHECK constraint ('cli', 'tui', 'gui')
        let result = repo.set_active_session("invalid", "stockpot", session_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_active_session_with_tui_interface() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.set_active_session("tui", "stockpot", session_id)
            .unwrap();

        let active = repo.get_active_session("tui").unwrap();
        assert_eq!(active, Some(session_id));
    }

    #[test]
    fn test_set_active_session_with_gui_interface() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.set_active_session("gui", "stockpot", session_id)
            .unwrap();

        let active = repo.get_active_session("gui").unwrap();
        assert_eq!(active, Some(session_id));
    }

    #[test]
    fn test_set_active_session_nonexistent_session_fails() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // Session 99999 doesn't exist - foreign key violation
        let result = repo.set_active_session("cli", "stockpot", 99999);
        assert!(result.is_err());
    }

    // =========================================================================
    // Session Isolation Tests
    // =========================================================================

    #[test]
    fn test_messages_isolated_between_sessions() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session1 = repo.create_session("stockpot").unwrap();
        let session2 = repo.create_session("stockpot").unwrap();

        repo.add_message(session1, "user", "Session 1 message", None)
            .unwrap();
        repo.add_message(session2, "user", "Session 2 message", None)
            .unwrap();
        repo.add_message(session1, "assistant", "Session 1 reply", None)
            .unwrap();

        let messages1 = repo.get_messages(session1).unwrap();
        let messages2 = repo.get_messages(session2).unwrap();

        assert_eq!(messages1.len(), 2);
        assert_eq!(messages2.len(), 1);
        assert!(messages1.iter().all(|m| m.session_id == session1));
        assert!(messages2.iter().all(|m| m.session_id == session2));
    }

    #[test]
    fn test_get_messages_nonexistent_session_returns_empty() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let messages = repo.get_messages(99999).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_delete_session_removes_only_target() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session1 = repo.create_session("stockpot").unwrap();
        let session2 = repo.create_session("stockpot").unwrap();

        repo.add_message(session1, "user", "Msg 1", None).unwrap();
        repo.add_message(session2, "user", "Msg 2", None).unwrap();

        repo.delete_session(session1).unwrap();

        let messages1 = repo.get_messages(session1).unwrap();
        let messages2 = repo.get_messages(session2).unwrap();

        assert!(messages1.is_empty());
        assert_eq!(messages2.len(), 1);
    }

    #[test]
    fn test_delete_session_cascades_to_active_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.set_active_session("cli", "stockpot", session_id)
            .unwrap();

        // Verify active session exists
        assert_eq!(repo.get_active_session("cli").unwrap(), Some(session_id));

        // Delete the session
        repo.delete_session(session_id).unwrap();

        // Active session should be cleared (CASCADE)
        let active = repo.get_active_session("cli").unwrap();
        assert!(active.is_none());
    }

    // =========================================================================
    // Copy History Edge Cases
    // =========================================================================

    #[test]
    fn test_copy_history_preserves_token_counts() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source = repo.create_session("stockpot").unwrap();
        repo.add_message(source, "user", "Short", Some(5)).unwrap();
        repo.add_message(source, "assistant", "Long response", Some(150))
            .unwrap();
        repo.add_message(source, "user", "No count", None).unwrap();

        let dest = repo.copy_history_to_agent(source, "explore").unwrap();

        let messages = repo.get_messages(dest).unwrap();
        assert_eq!(messages[0].token_count, Some(5));
        assert_eq!(messages[1].token_count, Some(150));
        assert_eq!(messages[2].token_count, None);
    }

    #[test]
    fn test_copy_history_original_unchanged() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source = repo.create_session("stockpot").unwrap();
        repo.add_message(source, "user", "Original", Some(10))
            .unwrap();

        let dest = repo.copy_history_to_agent(source, "explore").unwrap();

        // Add message to dest only
        repo.add_message(dest, "assistant", "New in dest", None)
            .unwrap();

        // Source should still have only 1 message
        let source_messages = repo.get_messages(source).unwrap();
        assert_eq!(source_messages.len(), 1);
        assert_eq!(source_messages[0].content, "Original");
    }

    #[test]
    fn test_copy_history_from_nonexistent_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // This creates a new session but copies 0 messages
        let dest = repo.copy_history_to_agent(99999, "explore").unwrap();
        let messages = repo.get_messages(dest).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_copy_history_message_order_preserved() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source = repo.create_session("stockpot").unwrap();
        for i in 0..10 {
            repo.add_message(source, "user", &format!("Message {}", i), None)
                .unwrap();
        }

        let dest = repo.copy_history_to_agent(source, "explore").unwrap();
        let messages = repo.get_messages(dest).unwrap();

        assert_eq!(messages.len(), 10);
        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(msg.content, format!("Message {}", i));
        }
    }

    // =========================================================================
    // Sub-Agent Invocation Edge Cases
    // =========================================================================

    #[test]
    fn test_sub_agent_invocation_with_empty_prompt() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let id = repo
            .record_sub_agent_invocation(Some(session_id), "explore", "", None, None)
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_sub_agent_invocation_with_large_response() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let large_response = "x".repeat(50_000);
        let id = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "explore",
                "Query",
                Some(&large_response),
                Some(5000),
            )
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_sub_agent_invocation_with_zero_duration() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id = repo
            .record_sub_agent_invocation(None, "fast-agent", "Quick task", Some("Done"), Some(0))
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_sub_agent_invocation_deleted_parent_sets_null() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        let invocation_id = repo
            .record_sub_agent_invocation(
                Some(session_id),
                "explore",
                "Query",
                Some("Result"),
                Some(100),
            )
            .unwrap();

        // Delete the parent session
        repo.delete_session(session_id).unwrap();

        // Invocation should still exist (ON DELETE SET NULL)
        let count: i32 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sub_agent_invocations WHERE id = ?",
                [invocation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Parent should be NULL
        let parent: Option<i64> = db
            .conn()
            .query_row(
                "SELECT parent_session_id FROM sub_agent_invocations WHERE id = ?",
                [invocation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(parent.is_none());
    }

    // =========================================================================
    // Role Edge Cases
    // =========================================================================

    #[test]
    fn test_message_with_custom_role() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "tool", "Tool output", None)
            .unwrap();
        repo.add_message(session_id, "function", "Function result", None)
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].role, "tool");
        assert_eq!(messages[1].role, "function");
    }

    #[test]
    fn test_message_with_empty_role() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "", "Empty role message", None)
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].role, "");
    }

    // =========================================================================
    // Token Count Edge Cases
    // =========================================================================

    #[test]
    fn test_message_with_zero_token_count() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "", Some(0)).unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].token_count, Some(0));
    }

    #[test]
    fn test_message_with_large_token_count() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "assistant", "Large response", Some(i64::MAX))
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].token_count, Some(i64::MAX));
    }

    #[test]
    fn test_message_with_negative_token_count() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        // While unusual, the schema allows negative values
        let session_id = repo.create_session("stockpot").unwrap();
        repo.add_message(session_id, "user", "Test", Some(-1))
            .unwrap();

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages[0].token_count, Some(-1));
    }

    // =========================================================================
    // Sequential Operation Tests
    // =========================================================================

    #[test]
    fn test_many_messages_in_session() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let session_id = repo.create_session("stockpot").unwrap();

        for i in 0..100 {
            repo.add_message(session_id, "user", &format!("Message {}", i), Some(i))
                .unwrap();
        }

        let messages = repo.get_messages(session_id).unwrap();
        assert_eq!(messages.len(), 100);

        // Verify ordering
        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(msg.content, format!("Message {}", i));
            assert_eq!(msg.token_count, Some(i as i64));
        }
    }

    #[test]
    fn test_many_sessions() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let mut session_ids = Vec::new();
        for _ in 0..50 {
            session_ids.push(repo.create_session("stockpot").unwrap());
        }

        // All IDs should be unique
        let unique_count = session_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(unique_count, 50);
    }

    #[test]
    fn test_active_session_all_interfaces() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let cli_session = repo.create_session("stockpot").unwrap();
        let tui_session = repo.create_session("explore").unwrap();
        let gui_session = repo.create_session("planning").unwrap();

        repo.set_active_session("cli", "stockpot", cli_session)
            .unwrap();
        repo.set_active_session("tui", "explore", tui_session)
            .unwrap();
        repo.set_active_session("gui", "planning", gui_session)
            .unwrap();

        assert_eq!(repo.get_active_session("cli").unwrap(), Some(cli_session));
        assert_eq!(repo.get_active_session("tui").unwrap(), Some(tui_session));
        assert_eq!(repo.get_active_session("gui").unwrap(), Some(gui_session));

        repo.clear_active_sessions().unwrap();

        assert!(repo.get_active_session("cli").unwrap().is_none());
        assert!(repo.get_active_session("tui").unwrap().is_none());
        assert!(repo.get_active_session("gui").unwrap().is_none());
    }

    // =========================================================================
    // Agent Name Edge Cases
    // =========================================================================

    #[test]
    fn test_session_with_empty_agent_name() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id = repo.create_session("").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_session_with_special_agent_name() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let id = repo.create_session("agent/with-special.chars_123").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_copy_history_to_same_agent_name() {
        let (_temp, db) = setup_test_db();
        let repo = SessionRepository::new(&db);

        let source = repo.create_session("stockpot").unwrap();
        repo.add_message(source, "user", "Test", None).unwrap();

        // Copy to same agent type
        let dest = repo.copy_history_to_agent(source, "stockpot").unwrap();

        assert_ne!(source, dest);
        let messages = repo.get_messages(dest).unwrap();
        assert_eq!(messages.len(), 1);
    }
}
