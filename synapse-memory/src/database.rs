use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct MemoryDatabase {
    conn: Connection,
}

impl MemoryDatabase {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS experiences (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                state_features TEXT NOT NULL,
                action TEXT NOT NULL,
                reward REAL NOT NULL,
                next_state_features TEXT NOT NULL,
                description TEXT,
                tags TEXT
            );

            CREATE TABLE IF NOT EXISTS patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                pattern_data TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0.0,
                times_matched INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_type TEXT NOT NULL,
                content TEXT NOT NULL,
                importance REAL NOT NULL DEFAULT 0.5,
                access_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                last_accessed TEXT
            );

            CREATE TABLE IF NOT EXISTS knowledge (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                category TEXT,
                confidence REAL NOT NULL DEFAULT 1.0,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    pub fn store_experience(
        &self,
        state_features: &[f64],
        action: &str,
        reward: f64,
        next_state_features: &[f64],
        description: &str,
        tags: &[String],
    ) -> Result<i64> {
        let state_json = serde_json::to_string(state_features)?;
        let next_state_json = serde_json::to_string(next_state_features)?;
        let tags_json = serde_json::to_string(tags)?;

        self.conn.execute(
            "INSERT INTO experiences (timestamp, state_features, action, reward, next_state_features, description, tags)
             VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6)",
            params![state_json, action, reward, next_state_json, description, tags_json],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn store_pattern(
        &self,
        name: &str,
        pattern_data: &str,
        confidence: f64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT OR REPLACE INTO patterns (name, pattern_data, confidence, times_matched, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, datetime('now'), datetime('now'))",
            params![name, pattern_data, confidence],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn store_knowledge(
        &self,
        key: &str,
        value: &str,
        category: Option<&str>,
        source: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT OR REPLACE INTO knowledge (key, value, category, confidence, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1.0, ?4, datetime('now'), datetime('now'))",
            params![key, value, category, source],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn store_memory(
        &self,
        memory_type: &str,
        content: &str,
        importance: f64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO memories (memory_type, content, importance, access_count, created_at, last_accessed)
             VALUES (?1, ?2, ?3, 0, datetime('now'), datetime('now'))",
            params![memory_type, content, importance],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_recent_experiences(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT description, action, reward FROM experiences ORDER BY id DESC LIMIT ?1",
        )?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let desc: String = row.get(0)?;
                let action: String = row.get(1)?;
                let reward: f64 = row.get(2)?;
                Ok(format!("[{}] {} (reward: {:.2})", action, desc, reward))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn get_knowledge(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM knowledge WHERE key = ?1")?;

        let result = stmt
            .query_row(params![key], |row| {
                let val: String = row.get(0)?;
                Ok(val)
            })
            .optional()?;

        Ok(result)
    }

    pub fn search_knowledge(&self, category: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM knowledge WHERE category = ?1")?;

        let rows = stmt
            .query_map(params![category], |row| {
                let key: String = row.get(0)?;
                let val: String = row.get(1)?;
                Ok((key, val))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn count_experiences(&self) -> Result<usize> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM experiences", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn count_knowledge(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM knowledge", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

use rusqlite::OptionalExtension;
