use std::{path::Path, time::Duration};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use uuid::Uuid;

use crate::models::{
    ConversationPage, ConversationSummary, DialoguePreferences, MemoryEntry, MemoryInput, Settings,
    SourceImage, Term, TermInput, TranslationRequest, TranslationResult,
};

// Increment when translation instructions or memory semantics change.
const MEMORY_PROTOCOL_VERSION: &str = "v2";

#[cfg(target_os = "macos")]
const DEFAULT_CAPTURE_SHORTCUT: &str = "Cmd+Shift+2";
#[cfg(not(target_os = "macos"))]
const DEFAULT_CAPTURE_SHORTCUT: &str = "Ctrl+Alt+T";

pub struct Database {
    connection: Connection,
}

fn db_error(_: impl std::fmt::Display) -> String {
    "无法操作本地数据库，请检查数据目录是否可读写。".into()
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(db_error)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(db_error)?;
        if version > 2 {
            return Err("本地数据由更新版本创建，请使用更新版本的软件打开。".into());
        }
        let transaction = connection.unchecked_transaction().map_err(db_error)?;
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    content TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS history (
                    id TEXT PRIMARY KEY,
                    text TEXT NOT NULL,
                    source_language TEXT NOT NULL,
                    target_language TEXT NOT NULL,
                    style TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    translation TEXT NOT NULL,
                    detected_language TEXT NOT NULL,
                    origin TEXT NOT NULL,
                    verified INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    source_image TEXT
                );
                CREATE INDEX IF NOT EXISTS history_session ON history(session_id);
                CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    source_text TEXT NOT NULL,
                    translation TEXT NOT NULL,
                    source_language TEXT NOT NULL,
                    target_language TEXT NOT NULL,
                    style TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    model TEXT NOT NULL,
                    glossary_revision INTEGER NOT NULL,
                    protocol_version TEXT NOT NULL,
                    UNIQUE(source_text, source_language, target_language, style,
                           model, glossary_revision, protocol_version)
                );
                CREATE TABLE IF NOT EXISTS terms (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    source_language TEXT NOT NULL,
                    target_language TEXT NOT NULL,
                    UNIQUE(source, source_language, target_language)
                );",
            )
            .map_err(db_error)?;
        // Inspect the actual column as well as the version so interrupted legacy setup
        // and databases that already gained this nullable column can be reopened safely.
        let has_source_image: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('history') WHERE name = 'source_image')",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !has_source_image {
            transaction
                .execute("ALTER TABLE history ADD COLUMN source_image TEXT", [])
                .map_err(db_error)?;
        }
        transaction
            .execute_batch("PRAGMA user_version = 2;")
            .map_err(db_error)?;
        let default_settings = serde_json::to_string(&Settings::default()).map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO settings(singleton, content) VALUES(1, ?1)",
                params![default_settings],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value) VALUES('session_id', ?1)",
                params![Uuid::new_v4().to_string()],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value) VALUES('glossary_revision', '0')",
                [],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value) VALUES('context_id', ?1)",
                params![Uuid::new_v4().to_string()],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value) VALUES('context_after_rowid', '0')",
                [],
            )
            .map_err(db_error)?;
        transaction.execute(
            "INSERT INTO metadata(key, value)
             VALUES('history_sequence', CAST((SELECT COALESCE(MAX(rowid), 0) FROM history) AS TEXT))
             ON CONFLICT(key) DO UPDATE SET value =
                 CAST(MAX(CAST(metadata.value AS INTEGER), CAST(excluded.value AS INTEGER)) AS TEXT)",
            [],
        ).map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value)
             SELECT 'context_id:' || session.value, context.value FROM metadata AS session
             JOIN metadata AS context ON context.key = 'context_id'
             WHERE session.key = 'session_id'",
                [],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO metadata(key, value)
             SELECT 'context_after_rowid:' || session.value, context.value FROM metadata AS session
             JOIN metadata AS context ON context.key = 'context_after_rowid'
             WHERE session.key = 'session_id'",
                [],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(Self { connection })
    }

    pub fn settings(&self) -> Result<Settings, String> {
        let content: String = self
            .connection
            .query_row(
                "SELECT content FROM settings WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        serde_json::from_str(&content).map_err(|_| "本地设置无法读取，请检查数据文件。".into())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let content = serde_json::to_string(settings).map_err(db_error)?;
        self.connection
            .execute(
                "UPDATE settings SET content = ?1 WHERE singleton = 1",
                params![content],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn session_id(&self) -> Result<String, String> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'session_id'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    pub fn capture_shortcut(&self) -> Result<String, String> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'capture_shortcut'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)
            .map(|value| value.unwrap_or_else(|| DEFAULT_CAPTURE_SHORTCUT.into()))
    }

    pub fn save_capture_shortcut(&self, shortcut: &str) -> Result<(), String> {
        self.connection.execute(
            "INSERT INTO metadata(key, value) VALUES('capture_shortcut', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![shortcut],
        ).map_err(db_error)?;
        Ok(())
    }

    fn metadata(&self, key: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)
    }
    fn set_metadata(&self, key: &str, value: &str) -> Result<(), String> {
        self.connection.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value]).map_err(db_error)?;
        Ok(())
    }
    pub fn workspace_mode_for(&self, session: &str) -> Result<String, String> {
        Ok(if self
            .metadata(&format!("workspace_mode:{session}"))?
            .as_deref()
            == Some("dialogue")
        {
            "dialogue"
        } else {
            "standard"
        }
        .into())
    }
    pub fn workspace_mode(&self) -> Result<String, String> {
        self.workspace_mode_for(&self.session_id()?)
    }
    pub fn dialogue_preferences(&self, session: &str) -> Result<DialoguePreferences, String> {
        self.metadata(&format!("dialogue_preferences:{session}"))?
            .map(|value| serde_json::from_str(&value).map_err(db_error))
            .unwrap_or_else(|| Ok(DialoguePreferences::default()))
    }
    pub fn save_dialogue_preferences(
        &self,
        session: &str,
        preferences: &DialoguePreferences,
    ) -> Result<(), String> {
        self.set_metadata(
            &format!("dialogue_preferences:{session}"),
            &serde_json::to_string(preferences).map_err(db_error)?,
        )
    }
    pub fn switch_workspace(&self, mode: &str) -> Result<String, String> {
        if !matches!(mode, "standard" | "dialogue") {
            return Err("请选择有效的翻译模式。".into());
        }
        let previous = self.session_id()?;
        let previous_mode = self.workspace_mode()?;
        self.set_metadata(&format!("workspace_session:{previous_mode}"), &previous)?;
        if previous_mode == mode {
            return Ok(previous);
        }
        if let Some(session) = self.metadata(&format!("workspace_session:{mode}"))? {
            if self.metadata(&format!("context_id:{session}"))?.is_some()
                || self.has_session(&session)?
            {
                self.open_session(&session)?;
                return Ok(session);
            }
        }
        self.new_session_in_mode(mode)
    }
    pub fn new_session(&self) -> Result<String, String> {
        self.new_session_in_mode(&self.workspace_mode()?)
    }
    pub fn new_session_in_mode(&self, mode: &str) -> Result<String, String> {
        if !matches!(mode, "standard" | "dialogue") {
            return Err("请选择有效的翻译模式。".into());
        }
        let session_id = Uuid::new_v4().to_string();
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        persist_context_in(&transaction)?;
        transaction
            .execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'session_id'",
                params![session_id],
            )
            .map_err(db_error)?;
        reset_context_in(&transaction)?;
        transaction.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("workspace_mode:{session_id}"),mode]).map_err(db_error)?;
        transaction.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("workspace_session:{mode}"),session_id]).map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(session_id)
    }

    pub fn context_id(&self) -> Result<String, String> {
        self.connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'context_id'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    pub fn reset_context(&self) -> Result<String, String> {
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        let context_id = reset_context_in(&transaction)?;
        transaction.commit().map_err(db_error)?;
        Ok(context_id)
    }

    pub fn reset_all_contexts(&self) -> Result<String, String> {
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        let current_session: String = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'session_id'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let session_ids: Vec<String> = {
            let mut statement = transaction
                .prepare(
                    "SELECT DISTINCT session_id FROM history
                 UNION SELECT value FROM metadata WHERE key = 'session_id'
                 UNION SELECT substr(key,12) FROM metadata WHERE instr(key,'context_id:')=1",
                )
                .map_err(db_error)?;
            let rows = statement
                .query_map([], |row| row.get(0))
                .map_err(db_error)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_error)?
        };
        let context_id = reset_context_in(&transaction)?;
        let boundary: String = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'context_after_rowid'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        for session_id in session_ids {
            if session_id == current_session {
                continue;
            }
            transaction
                .execute(
                    "INSERT INTO metadata(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![
                        format!("context_id:{session_id}"),
                        Uuid::new_v4().to_string()
                    ],
                )
                .map_err(db_error)?;
            transaction
                .execute(
                    "INSERT INTO metadata(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![format!("context_after_rowid:{session_id}"), boundary],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)?;
        Ok(context_id)
    }

    pub fn conversations(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ConversationPage, String> {
        let limit = if limit == 0 { 40 } else { limit.min(100) };
        let offset = i64::try_from(offset).unwrap_or(i64::MAX);
        let mut statement = self
            .connection
            .prepare(
                "WITH sessions AS (
                 SELECT session_id, MIN(rowid) AS first_row, MAX(rowid) AS last_row,
                        COUNT(*) AS turn_count
                 FROM history GROUP BY session_id
             )
             SELECT sessions.session_id, first.text, first.source_image, latest.translation,
                    sessions.turn_count, first.created_at, latest.created_at,
                    COALESCE((SELECT value FROM metadata WHERE key = 'workspace_mode:' || sessions.session_id), 'standard')
             FROM sessions
             JOIN history AS first ON first.rowid = sessions.first_row
             JOIN history AS latest ON latest.rowid = sessions.last_row
             WHERE ?1 = '' OR EXISTS (
                 SELECT 1 FROM history AS matched
                 WHERE matched.session_id = sessions.session_id
                   AND (instr(lower(matched.text), lower(?1)) > 0
                        OR instr(lower(matched.translation), lower(?1)) > 0)
             )
             ORDER BY sessions.last_row DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![query.trim(), (limit + 1) as i64, offset], |row| {
                let text: String = row.get(1)?;
                let image = source_image_column(row, 2)?;
                let title = text
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .or_else(|| image.as_ref().map(|image| image.name.trim()))
                    .filter(|title| !title.is_empty())
                    .unwrap_or("图片翻译");
                let translation: String = row.get(3)?;
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    title: title.chars().take(60).collect(),
                    preview: translation.chars().take(120).collect(),
                    turn_count: row.get::<_, i64>(4)? as usize,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    mode: row.get(7)?,
                })
            })
            .map_err(db_error)?;
        let mut conversations = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        let has_more = conversations.len() > limit;
        conversations.truncate(limit);
        Ok(ConversationPage {
            conversations,
            has_more,
        })
    }

    pub fn has_session(&self, session_id: &str) -> Result<bool, String> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history WHERE session_id = ?1)",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    pub fn open_session(&self, session_id: &str) -> Result<(), String> {
        let previous_session = self.session_id()?;
        let previous_mode = self.workspace_mode()?;
        let mode = self.workspace_mode_for(session_id)?;
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history WHERE session_id = ?1) OR EXISTS(SELECT 1 FROM metadata WHERE key = 'context_id:' || ?1)",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err("这份翻译记录已不存在，请刷新历史记录。".into());
        }
        persist_context_in(&transaction)?;
        transaction
            .execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'session_id'",
                params![session_id],
            )
            .map_err(db_error)?;
        let context_id: String = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![format!("context_id:{session_id}")],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let context_boundary: String = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![format!("context_after_rowid:{session_id}")],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or_else(|| "0".into());
        transaction
            .execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'context_id'",
                params![context_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'context_after_rowid'",
                params![context_boundary],
            )
            .map_err(db_error)?;
        persist_context_in(&transaction)?;
        for (key, value) in [
            (
                format!("workspace_session:{previous_mode}"),
                previous_session,
            ),
            (format!("workspace_session:{mode}"), session_id.to_owned()),
        ] {
            transaction.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value]).map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), String> {
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM history WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM metadata WHERE key IN (?1, ?2, ?3, ?4) OR (instr(key,'workspace_session:')=1 AND value=?5)",
                params![
                    format!("context_id:{session_id}"),
                    format!("context_after_rowid:{session_id}"),
                    format!("workspace_mode:{session_id}"),
                    format!("dialogue_preferences:{session_id}"),
                    session_id,
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub fn history(&self) -> Result<Vec<TranslationResult>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, text, source_language, target_language, style, session_id, translation,
                             detected_language, origin, verified, created_at, source_image
                      FROM history ORDER BY rowid DESC LIMIT 200",
            )
            .map_err(db_error)?;
        let rows = statement.query_map([], history_row).map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
    }

    pub fn append_history(&self, result: &TranslationResult) -> Result<(), String> {
        let source_image = result
            .source_image
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(db_error)?;
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        // SQLite's implicit rowid can be reused after deleting the newest record.
        // Context boundaries rely on insertion order, so keep a persistent high-water mark.
        let next_rowid: i64 = transaction.query_row(
            "SELECT MAX((SELECT COALESCE(MAX(rowid), 0) FROM history),
                        CAST((SELECT value FROM metadata WHERE key = 'history_sequence') AS INTEGER)) + 1",
            [], |row| row.get(0),
        ).map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO history
                (id, text, source_language, target_language, style, session_id, translation,
                 detected_language, origin, verified, created_at, source_image, rowid)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    result.id,
                    result.text,
                    result.source_language,
                    result.target_language,
                    result.style,
                    result.session_id,
                    result.translation,
                    result.detected_language,
                    result.origin,
                    result.verified,
                    result.created_at,
                    source_image,
                    next_rowid,
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE metadata SET value = ?1 WHERE key = 'history_sequence'",
                params![next_rowid.to_string()],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub fn delete_history(&self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(db_error)?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM history", [])
            .map_err(db_error)?;
        Ok(())
    }

    pub fn session_turns(&self, session_id: &str) -> Result<Vec<TranslationResult>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, text, source_language, target_language, style, session_id, translation,
                        detected_language, origin, verified, created_at, source_image
                 FROM history WHERE session_id = ?1 ORDER BY rowid ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![session_id], history_row)
            .map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
    }

    pub fn recent_context(&self, session_id: &str) -> Result<Vec<TranslationResult>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, text, source_language, target_language, style, session_id, translation,
                        detected_language, origin, verified, created_at, source_image
                 FROM history WHERE session_id = ?1
                   AND rowid > (SELECT CAST(value AS INTEGER) FROM metadata
                                WHERE key = 'context_after_rowid')
                 ORDER BY rowid DESC LIMIT 12",
            )
            .map_err(db_error)?;
        let mut results = statement
            .query_map(params![session_id], history_row)
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        results.reverse();
        Ok(results)
    }

    pub fn memories(&self) -> Result<Vec<MemoryEntry>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, source_text, translation, source_language, target_language, style,
                    created_at, model, glossary_revision
             FROM memories ORDER BY created_at DESC, rowid DESC LIMIT 500",
            )
            .map_err(db_error)?;
        let rows = statement.query_map([], memory_row).map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
    }

    pub fn confirm_memory(&self, input: &MemoryInput, settings: &Settings) -> Result<(), String> {
        self.connection.execute(
            "INSERT INTO memories(id, source_text, translation, source_language, target_language,
                                  style, created_at, model, glossary_revision, protocol_version)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                    (SELECT CAST(value AS INTEGER) FROM metadata WHERE key = 'glossary_revision'), ?9)
             ON CONFLICT(source_text, source_language, target_language, style, model,
                         glossary_revision, protocol_version)
             DO UPDATE SET translation = excluded.translation, created_at = excluded.created_at",
            params![Uuid::new_v4().to_string(), input.source_text.trim(), input.translation.trim(),
                    input.source_language, input.target_language, input.style,
                    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true), settings.model,
                    MEMORY_PROTOCOL_VERSION],
        ).map_err(db_error)?;
        Ok(())
    }

    pub fn find_memory(
        &self,
        request: &TranslationRequest,
        settings: &Settings,
    ) -> Result<Option<MemoryEntry>, String> {
        self.connection
            .query_row(
                "SELECT id, source_text, translation, source_language, target_language, style,
                    created_at, model, glossary_revision
             FROM memories WHERE source_text = ?1 AND source_language = ?2 AND target_language = ?3
               AND style = ?4 AND model = ?5 AND protocol_version = ?6
               AND glossary_revision =
                   (SELECT CAST(value AS INTEGER) FROM metadata WHERE key = 'glossary_revision')
             LIMIT 1",
                params![
                    request.text.trim(),
                    request.source_language,
                    request.target_language,
                    request.style,
                    settings.model,
                    MEMORY_PROTOCOL_VERSION
                ],
                memory_row,
            )
            .optional()
            .map_err(db_error)
    }

    pub fn delete_memory(&self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .map_err(db_error)?;
        Ok(())
    }

    pub fn clear_memories(&self) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM memories", [])
            .map_err(db_error)?;
        Ok(())
    }

    pub fn terms(&self) -> Result<Vec<Term>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, source, target, source_language, target_language
             FROM terms ORDER BY source COLLATE NOCASE, id",
            )
            .map_err(db_error)?;
        let rows = statement.query_map([], term_row).map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_error)
    }

    pub fn matching_terms(&self, request: &TranslationRequest) -> Result<Vec<Term>, String> {
        let text = request.text.to_lowercase();
        let mut matches: Vec<Term> = self
            .terms()?
            .into_iter()
            .filter(|term| {
                term.target_language == request.target_language
                    && (term.source_language == request.source_language
                        || term.source_language == "auto"
                        || request.source_language == "auto")
                    && !term.source.is_empty()
                    && text.contains(&term.source.to_lowercase())
            })
            .collect();
        // Prefer specific phrases over fragments when the request matches many terms.
        matches.sort_by_key(|term| std::cmp::Reverse(term.source.chars().count()));
        matches.truncate(24);
        Ok(matches)
    }

    pub fn upsert_term(&self, input: &TermInput) -> Result<(), String> {
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        transaction.execute(
            "INSERT INTO terms(id, source, target, source_language, target_language)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source, source_language, target_language) DO UPDATE SET target = excluded.target",
            params![Uuid::new_v4().to_string(), input.source.trim(), input.target.trim(),
                    input.source_language, input.target_language],
        ).map_err(db_error)?;
        transaction
            .execute(
                "UPDATE metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
             WHERE key = 'glossary_revision'",
                [],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub fn delete_term(&self, id: &str) -> Result<(), String> {
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        let deleted = transaction
            .execute("DELETE FROM terms WHERE id = ?1", params![id])
            .map_err(db_error)?;
        if deleted > 0 {
            transaction
                .execute(
                    "UPDATE metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
                 WHERE key = 'glossary_revision'",
                    [],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)
    }
}

fn reset_context_in(transaction: &Transaction<'_>) -> Result<String, String> {
    let context_id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "UPDATE metadata SET value = ?1 WHERE key = 'context_id'",
            params![context_id],
        )
        .map_err(db_error)?;
    transaction
        .execute(
            "UPDATE metadata SET value =
                 CAST((SELECT COALESCE(MAX(rowid), 0) FROM history) AS TEXT)
             WHERE key = 'context_after_rowid'",
            [],
        )
        .map_err(db_error)?;
    persist_context_in(transaction)?;
    Ok(context_id)
}

fn persist_context_in(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO metadata(key, value)
         SELECT 'context_id:' || session.value, context.value FROM metadata AS session
         JOIN metadata AS context ON context.key = 'context_id'
         WHERE session.key = 'session_id'
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(db_error)?;
    transaction
        .execute(
            "INSERT INTO metadata(key, value)
         SELECT 'context_after_rowid:' || session.value, context.value FROM metadata AS session
         JOIN metadata AS context ON context.key = 'context_after_rowid'
         WHERE session.key = 'session_id'
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(db_error)?;
    Ok(())
}

fn history_row(row: &Row<'_>) -> rusqlite::Result<TranslationResult> {
    Ok(TranslationResult {
        id: row.get(0)?,
        text: row.get(1)?,
        source_language: row.get(2)?,
        target_language: row.get(3)?,
        style: row.get(4)?,
        session_id: row.get(5)?,
        translation: row.get(6)?,
        detected_language: row.get(7)?,
        origin: row.get(8)?,
        verified: row.get(9)?,
        created_at: row.get(10)?,
        source_image: source_image_column(row, 11)?,
    })
}

fn source_image_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<SourceImage>> {
    let content: Option<String> = row.get(column)?;
    content
        .map(|content| {
            serde_json::from_str(&content).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn memory_row(row: &Row<'_>) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get(0)?,
        source_text: row.get(1)?,
        translation: row.get(2)?,
        source_language: row.get(3)?,
        target_language: row.get(4)?,
        style: row.get(5)?,
        created_at: row.get(6)?,
        model: row.get(7)?,
        glossary_revision: row.get(8)?,
    })
}

fn term_row(row: &Row<'_>) -> rusqlite::Result<Term> {
    Ok(Term {
        id: row.get(0)?,
        source: row.get(1)?,
        target: row.get(2)?,
        source_language: row.get(3)?,
        target_language: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_modes_preferences_and_history_survive_reopening() {
        let directory = std::env::temp_dir().join(format!("moyu-dialogue-db-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("test.sqlite3");
        let (standard, dialogue);
        {
            let db = Database::open(&path).unwrap();
            standard = db.session_id().unwrap();
            db.append_history(&history_result(1, &standard)).unwrap();
            assert_eq!(db.workspace_mode().unwrap(), "standard");
            dialogue = db.switch_workspace("dialogue").unwrap();
            assert_ne!(dialogue, standard);
            assert_eq!(db.workspace_mode().unwrap(), "dialogue");
            let preferences = DialoguePreferences {
                last_peer_language: "de".into(),
                last_direction: "outbound".into(),
                ..DialoguePreferences::default()
            };
            db.save_dialogue_preferences(&dialogue, &preferences)
                .unwrap();
            db.append_history(&history_result(2, &dialogue)).unwrap();
            assert_eq!(db.switch_workspace("standard").unwrap(), standard);
            assert_eq!(db.session_turns(&standard).unwrap().len(), 1);
            assert_eq!(db.switch_workspace("dialogue").unwrap(), dialogue);
            assert_eq!(db.dialogue_preferences(&dialogue).unwrap(), preferences);
            assert_eq!(
                db.conversations("", 0, 40).unwrap().conversations[0].mode,
                "dialogue"
            );
        }
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.workspace_mode().unwrap(), "dialogue");
            assert_eq!(
                db.dialogue_preferences(&dialogue)
                    .unwrap()
                    .last_peer_language,
                "de"
            );
            assert_eq!(db.switch_workspace("standard").unwrap(), standard);
            db.delete_session(&dialogue).unwrap();
            assert_ne!(db.switch_workspace("dialogue").unwrap(), dialogue);
            assert_eq!(
                db.dialogue_preferences(&db.session_id().unwrap()).unwrap(),
                DialoguePreferences::default()
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn empty_workspaces_can_switch_and_global_reset_covers_inactive_sessions() {
        let db = database();
        let standard = db.session_id().unwrap();
        let old_standard_context = db.context_id().unwrap();
        let dialogue = db.switch_workspace("dialogue").unwrap();
        let old_dialogue_context = db.context_id().unwrap();
        assert_eq!(db.switch_workspace("standard").unwrap(), standard);
        db.reset_all_contexts().unwrap();
        assert_ne!(db.context_id().unwrap(), old_standard_context);
        assert_eq!(db.switch_workspace("dialogue").unwrap(), dialogue);
        assert_ne!(db.context_id().unwrap(), old_dialogue_context);
        assert!(db.switch_workspace("invalid").is_err());
    }

    #[test]
    fn capture_shortcut_persists_without_changing_translation_or_context() {
        let directory = std::env::temp_dir().join(format!("moyu-hotkey-db-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("test.sqlite3");
        let (session, context);
        {
            let db = Database::open(&path).unwrap();
            session = db.session_id().unwrap();
            context = db.context_id().unwrap();
            assert_eq!(db.capture_shortcut().unwrap(), DEFAULT_CAPTURE_SHORTCUT);
            db.save_capture_shortcut("Ctrl+Shift+Q").unwrap();
            assert_eq!(db.settings().unwrap(), Settings::default());
            assert_eq!(db.session_id().unwrap(), session);
            assert_eq!(db.context_id().unwrap(), context);
        }
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.capture_shortcut().unwrap(), "Ctrl+Shift+Q");
            db.save_capture_shortcut("").unwrap();
        }
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.capture_shortcut().unwrap(), "");
            assert_eq!(db.session_id().unwrap(), session);
            assert_eq!(db.context_id().unwrap(), context);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn database() -> Database {
        Database::open(Path::new(":memory:")).unwrap()
    }

    fn request(text: &str, session_id: &str) -> TranslationRequest {
        TranslationRequest {
            text: text.into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
            session_id: session_id.into(),
            batch_id: None,
            source_image: None,
        }
    }

    fn input(text: &str) -> MemoryInput {
        MemoryInput {
            source_text: text.into(),
            translation: "你好".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
        }
    }

    fn history_result(index: usize, session_id: &str) -> TranslationResult {
        TranslationResult {
            id: format!("result-{index}"),
            text: index.to_string(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
            style: "natural".into(),
            session_id: session_id.into(),
            translation: "译文".into(),
            detected_language: "en".into(),
            origin: "model".into(),
            verified: true,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            source_image: None,
        }
    }

    #[test]
    fn settings_and_session_survive_reopening() {
        let directory = std::env::temp_dir().join(format!("moyu-db-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("test.sqlite3");
        let expected_session;
        let expected_context;
        let expected_settings = Settings {
            model: "deepseek-v4-pro".into(),
            use_context: false,
            ..Settings::default()
        };
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.settings().unwrap(), Settings::default());
            db.save_settings(&expected_settings).unwrap();
            let previous_session = db.session_id().unwrap();
            let previous_context = db.context_id().unwrap();
            expected_session = db.new_session().unwrap();
            assert_ne!(expected_session, previous_session);
            assert_ne!(db.context_id().unwrap(), previous_context);
            db.append_history(&history_result(1, &expected_session))
                .unwrap();
            expected_context = db.reset_context().unwrap();
            db.append_history(&history_result(2, &expected_session))
                .unwrap();
            let another_session = db.new_session().unwrap();
            db.append_history(&history_result(3, &another_session))
                .unwrap();
            db.open_session(&expected_session).unwrap();
        }
        {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.settings().unwrap(), expected_settings);
            assert_eq!(db.session_id().unwrap(), expected_session);
            assert_eq!(db.context_id().unwrap(), expected_context);
            assert_eq!(db.session_turns(&expected_session).unwrap().len(), 2);
            let context = db.recent_context(&expected_session).unwrap();
            assert_eq!(context.len(), 1);
            assert_eq!(context[0].text, "2");
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn version_one_migration_preserves_history_settings_and_context() {
        let directory = std::env::temp_dir().join(format!("moyu-db-migration-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("legacy.sqlite3");
        let expected_settings = Settings {
            use_context: false,
            ..Settings::default()
        };
        {
            let legacy = Connection::open(&path).unwrap();
            legacy.execute_batch(
                "CREATE TABLE history (
                    id TEXT PRIMARY KEY, text TEXT NOT NULL, source_language TEXT NOT NULL,
                    target_language TEXT NOT NULL, style TEXT NOT NULL, session_id TEXT NOT NULL,
                    translation TEXT NOT NULL, detected_language TEXT NOT NULL, origin TEXT NOT NULL,
                    verified INTEGER NOT NULL, created_at TEXT NOT NULL
                 );
                 INSERT INTO history VALUES('legacy-1', 'old source', 'en', 'zh-CN', 'natural',
                    'legacy-session', '旧译文', 'en', 'model', 1, '2026-01-01T00:00:00.000Z');
                 CREATE TABLE settings (singleton INTEGER PRIMARY KEY, content TEXT NOT NULL);
                 CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO metadata VALUES('session_id', 'legacy-session');
                 INSERT INTO metadata VALUES('context_id', 'legacy-context');
                 INSERT INTO metadata VALUES('context_after_rowid', '1');
                 PRAGMA user_version = 1;",
            ).unwrap();
            legacy
                .execute(
                    "INSERT INTO settings VALUES(1, ?1)",
                    params![serde_json::to_string(&expected_settings).unwrap()],
                )
                .unwrap();
        }
        for _ in 0..2 {
            let db = Database::open(&path).unwrap();
            assert_eq!(db.settings().unwrap(), expected_settings);
            assert_eq!(db.session_id().unwrap(), "legacy-session");
            assert_eq!(db.context_id().unwrap(), "legacy-context");
            let turns = db.session_turns("legacy-session").unwrap();
            assert_eq!(turns.len(), 1);
            assert_eq!(turns[0].text, "old source");
            assert_eq!(turns[0].translation, "旧译文");
            assert!(turns[0].source_image.is_none());
            assert!(db.recent_context("legacy-session").unwrap().is_empty());
            let version: i64 = db
                .connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 2);
            let snapshot: String = db
                .connection
                .query_row(
                    "SELECT value FROM metadata WHERE key = 'context_after_rowid:legacy-session'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(snapshot, "1");
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn source_image_round_trips_in_history_transcript_and_context() {
        let db = database();
        let mut result = history_result(1, "image-session");
        result.text.clear();
        result.source_image = Some(SourceImage {
            id: "image-1".into(),
            name: "屏幕截图.png".into(),
            mime_type: "image/png".into(),
            thumbnail_data_url: "data:image/png;base64,aGVsbG8=".into(),
        });
        db.append_history(&result).unwrap();
        assert_eq!(db.history().unwrap(), vec![result.clone()]);
        assert_eq!(
            db.session_turns("image-session").unwrap(),
            vec![result.clone()]
        );
        assert_eq!(db.recent_context("image-session").unwrap(), vec![result]);
        let summaries = db.conversations("", 0, 40).unwrap();
        assert_eq!(summaries.conversations[0].title, "屏幕截图.png");
    }

    #[test]
    fn conversations_paginate_search_full_sessions_and_reopen_or_delete_in_isolation() {
        let db = database();
        for index in 0..45 {
            let mut result = history_result(index, &format!("session-{index}"));
            result.text = format!("Material {index}\nsecond line");
            result.translation = format!("preview {index}");
            if index == 0 {
                result.text = "Needle '; OR 1=1 --\nsecond line".into();
            }
            if index == 1 {
                result.text = "长".repeat(70);
                result.translation = "译".repeat(130);
            }
            db.append_history(&result).unwrap();
        }
        let mut latest = history_result(100, "session-0");
        latest.translation = "most recent translation".into();
        latest.created_at = "2026-01-02T00:00:00.000Z".into();
        db.append_history(&latest).unwrap();
        let first_page = db.conversations("", 0, 40).unwrap();
        assert_eq!(first_page.conversations.len(), 40);
        assert!(first_page.has_more);
        assert_eq!(first_page.conversations[0].id, "session-0");
        assert_eq!(first_page.conversations[0].turn_count, 2);
        assert_eq!(
            first_page.conversations[0].preview,
            "most recent translation"
        );
        assert_eq!(
            first_page.conversations[0].created_at,
            "2026-01-01T00:00:00.000Z"
        );
        assert_eq!(
            first_page.conversations[0].updated_at,
            "2026-01-02T00:00:00.000Z"
        );
        let second_page = db.conversations("", 40, 40).unwrap();
        assert_eq!(second_page.conversations.len(), 5);
        assert!(!second_page.has_more);
        let all = db.conversations("", 0, 500).unwrap();
        assert_eq!(all.conversations.len(), 45);
        assert!(!all.has_more);
        let long = all
            .conversations
            .iter()
            .find(|item| item.id == "session-1")
            .unwrap();
        assert_eq!(long.title.chars().count(), 60);
        assert_eq!(long.preview.chars().count(), 120);
        assert_eq!(db.conversations("", 0, 0).unwrap().conversations.len(), 40);
        assert!(db
            .conversations("", usize::MAX, 40)
            .unwrap()
            .conversations
            .is_empty());
        let matching = db.conversations("needle", 0, 40).unwrap();
        assert_eq!(matching.conversations.len(), 1);
        assert_eq!(matching.conversations[0].turn_count, 2);
        assert_eq!(matching.conversations[0].preview, "most recent translation");
        assert_eq!(
            db.conversations("MOST RECENT", 0, 40)
                .unwrap()
                .conversations
                .len(),
            1
        );
        assert_eq!(
            db.conversations("'; OR 1=1 --", 0, 40)
                .unwrap()
                .conversations
                .len(),
            1
        );
        assert!(db
            .conversations("%", 0, 40)
            .unwrap()
            .conversations
            .is_empty());
        assert!(db.has_session("session-0").unwrap());
        assert!(!db.has_session("unknown").unwrap());
        let before = db.session_id().unwrap();
        assert!(db.open_session("unknown").is_err());
        assert_eq!(db.session_id().unwrap(), before);
        db.open_session("session-0").unwrap();
        assert_eq!(db.session_id().unwrap(), "session-0");
        assert_eq!(db.session_turns("session-0").unwrap().len(), 2);
        assert_eq!(db.recent_context("session-0").unwrap().len(), 2);
        db.delete_session("' OR 1=1 --").unwrap();
        assert_eq!(
            db.conversations("", 0, 100).unwrap().conversations.len(),
            45
        );
        db.delete_session("session-44").unwrap();
        assert!(!db.has_session("session-44").unwrap());
        assert_eq!(db.session_turns("session-0").unwrap().len(), 2);
        db.delete_session("session-0").unwrap();
        assert!(!db.has_session("session-0").unwrap());
        assert!(db.session_turns("session-0").unwrap().is_empty());
    }

    #[test]
    fn conversation_switching_restores_each_context_reset_boundary() {
        let db = database();
        let session_a = db.session_id().unwrap();
        db.append_history(&history_result(1, &session_a)).unwrap();
        let a_context_id = db.reset_context().unwrap();
        db.append_history(&history_result(2, &session_a)).unwrap();
        let session_b = db.new_session().unwrap();
        let b_context_id = db.context_id().unwrap();
        db.append_history(&history_result(3, &session_b)).unwrap();
        db.open_session(&session_a).unwrap();
        assert_eq!(db.context_id().unwrap(), a_context_id);
        assert_eq!(db.session_turns(&session_a).unwrap().len(), 2);
        let resumed_a = db.recent_context(&session_a).unwrap();
        assert_eq!(resumed_a.len(), 1);
        assert_eq!(resumed_a[0].text, "2");
        db.open_session(&session_b).unwrap();
        assert_eq!(db.context_id().unwrap(), b_context_id);
        let resumed_b = db.recent_context(&session_b).unwrap();
        assert_eq!(resumed_b.len(), 1);
        assert_eq!(resumed_b[0].text, "3");
    }

    #[test]
    fn global_context_reset_invalidates_other_sessions_without_deleting_history() {
        let db = database();
        let session_a = db.session_id().unwrap();
        let old_a_context = db.context_id().unwrap();
        db.append_history(&history_result(1, &session_a)).unwrap();
        let session_b = db.new_session().unwrap();
        let old_b_context = db.context_id().unwrap();
        db.append_history(&history_result(2, &session_b)).unwrap();
        let new_b_context = db.reset_all_contexts().unwrap();
        assert_ne!(new_b_context, old_b_context);
        assert_eq!(db.context_id().unwrap(), new_b_context);
        assert!(db.recent_context(&session_b).unwrap().is_empty());
        db.open_session(&session_a).unwrap();
        assert_ne!(db.context_id().unwrap(), old_a_context);
        assert!(db.recent_context(&session_a).unwrap().is_empty());
        assert_eq!(db.session_turns(&session_a).unwrap().len(), 1);
        assert_eq!(db.session_turns(&session_b).unwrap().len(), 1);
        db.append_history(&history_result(3, &session_a)).unwrap();
        assert_eq!(db.recent_context(&session_a).unwrap()[0].text, "3");
    }

    #[test]
    fn deleting_newest_other_session_cannot_reuse_rowid_before_a_context_boundary() {
        let db = database();
        let session_a = db.session_id().unwrap();
        db.append_history(&history_result(1, &session_a)).unwrap();
        let session_b = db.new_session().unwrap();
        db.append_history(&history_result(2, &session_b)).unwrap();
        db.open_session(&session_a).unwrap();
        db.reset_context().unwrap();
        db.delete_session(&session_b).unwrap();
        db.append_history(&history_result(3, &session_a)).unwrap();
        let recent = db.recent_context(&session_a).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].text, "3");
        assert_eq!(db.session_turns(&session_a).unwrap().len(), 2);
        let rowid: i64 = db
            .connection
            .query_row(
                "SELECT rowid FROM history WHERE id = 'result-3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rowid, 3);
        db.clear_history().unwrap();
        db.reset_context().unwrap();
        db.append_history(&history_result(4, &session_a)).unwrap();
        let next: i64 = db
            .connection
            .query_row("SELECT rowid FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next, 4);
        assert_eq!(db.recent_context(&session_a).unwrap()[0].text, "4");
    }

    #[test]
    fn context_reset_preserves_material_and_only_includes_new_turns() {
        let db = database();
        let session_id = db.session_id().unwrap();
        let original_context = db.context_id().unwrap();
        db.append_history(&history_result(1, &session_id)).unwrap();
        db.append_history(&history_result(2, &session_id)).unwrap();
        let transcript = db.session_turns(&session_id).unwrap();
        let next_context = db.reset_context().unwrap();
        assert_ne!(next_context, original_context);
        assert_eq!(db.context_id().unwrap(), next_context);
        assert_eq!(db.session_id().unwrap(), session_id);
        assert_eq!(db.session_turns(&session_id).unwrap(), transcript);
        assert!(db.recent_context(&session_id).unwrap().is_empty());
        db.append_history(&history_result(3, &session_id)).unwrap();
        db.append_history(&history_result(4, "another-material"))
            .unwrap();
        let context = db.recent_context(&session_id).unwrap();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].text, "3");
        assert_eq!(db.session_turns(&session_id).unwrap().len(), 3);
        db.clear_history().unwrap();
        db.reset_context().unwrap();
        db.append_history(&history_result(5, &session_id)).unwrap();
        assert_eq!(db.recent_context(&session_id).unwrap()[0].text, "5");
    }

    #[test]
    fn exact_memory_requires_same_style_model_direction_and_glossary() {
        let db = database();
        let settings = Settings::default();
        db.confirm_memory(&input(" hello "), &settings).unwrap();
        let original = request("\nhello  ", "session");
        assert!(db.find_memory(&original, &settings).unwrap().is_some());
        let mut changed = original.clone();
        changed.style = "formal".into();
        assert!(db.find_memory(&changed, &settings).unwrap().is_none());
        changed = original.clone();
        changed.source_language = "zh-CN".into();
        changed.target_language = "en".into();
        assert!(db.find_memory(&changed, &settings).unwrap().is_none());
        let other_model = Settings {
            model: "deepseek-v4-pro".into(),
            ..settings.clone()
        };
        assert!(db.find_memory(&original, &other_model).unwrap().is_none());
        let term = TermInput {
            source: "hello".into(),
            target: "您好".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
        };
        db.upsert_term(&term).unwrap();
        assert!(db.find_memory(&original, &settings).unwrap().is_none());
        db.confirm_memory(&input("hello"), &settings).unwrap();
        assert_eq!(
            db.find_memory(&original, &settings)
                .unwrap()
                .unwrap()
                .glossary_revision,
            1
        );
        db.delete_term(&db.terms().unwrap()[0].id).unwrap();
        assert!(db.find_memory(&original, &settings).unwrap().is_none());
    }

    #[test]
    fn untrusted_text_stays_data_and_term_matching_respects_direction() {
        let db = database();
        let attack = "'; DROP TABLE memories; --";
        db.confirm_memory(&input(attack), &Settings::default())
            .unwrap();
        assert!(db
            .find_memory(&request(attack, "session"), &Settings::default())
            .unwrap()
            .is_some());
        let term = TermInput {
            source: "API".into(),
            target: "接口".into(),
            source_language: "en".into(),
            target_language: "zh-CN".into(),
        };
        db.upsert_term(&term).unwrap();
        assert_eq!(
            db.matching_terms(&request("an api call", "session"))
                .unwrap()
                .len(),
            1
        );
        let mut reversed = request("an API call", "session");
        reversed.target_language = "ja".into();
        assert!(db.matching_terms(&reversed).unwrap().is_empty());
        db.delete_memory("' OR 1=1 --").unwrap();
        assert_eq!(db.memories().unwrap().len(), 1);
    }

    #[test]
    fn context_is_bounded_while_full_session_history_is_retained() {
        let db = database();
        for index in 0..1005 {
            db.append_history(&history_result(
                index,
                if index % 2 == 0 { "a" } else { "b" },
            ))
            .unwrap();
        }
        assert_eq!(db.history().unwrap().len(), 200);
        assert_eq!(db.history().unwrap()[0].text, "1004");
        let count: i64 = db
            .connection
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1005);
        let turns = db.session_turns("a").unwrap();
        assert_eq!(turns.len(), 503);
        assert!(turns.iter().all(|row| row.session_id == "a"));
        assert_eq!(
            turns.iter().map(|row| row.text.clone()).collect::<Vec<_>>(),
            (0..1005)
                .step_by(2)
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
        );
        assert!(db.session_turns("unknown").unwrap().is_empty());
        let recent = db.recent_context("a").unwrap();
        assert_eq!(recent.len(), 12);
        assert!(recent.iter().all(|row| row.session_id == "a"));
        assert_eq!(
            recent
                .iter()
                .map(|row| row.text.clone())
                .collect::<Vec<_>>(),
            (982..1005)
                .step_by(2)
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
        );
        assert!(db.recent_context("unknown").unwrap().is_empty());
        for index in 1005..1510 {
            db.append_history(&history_result(index, "a")).unwrap();
        }
        let full_session = db.session_turns("a").unwrap();
        assert_eq!(full_session.len(), 1008);
        assert_eq!(full_session.first().unwrap().text, "0");
        assert_eq!(full_session.last().unwrap().text, "1509");
        db.clear_history().unwrap();
        assert!(db.session_turns("a").unwrap().is_empty());
        assert!(db.recent_context("a").unwrap().is_empty());
    }
}
