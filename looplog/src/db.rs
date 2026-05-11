use crate::error::{Error, Result};
use crate::meta;
use crate::protocol::{AppendLine, FinishRunRequest};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

pub const DEFAULT_KEEP_HOURS: u64 = 24;

#[derive(Debug, Clone)]
pub struct NewRun {
    pub tag: Option<String>,
    pub source: Option<String>,
    pub cwd: Option<String>,
    pub argv: Vec<String>,
    pub client_id: Option<String>,
    pub kind: Option<String>,
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    pub run_id: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<String>,
    pub appid: Option<String>,
    pub project: Option<String>,
    pub page: Option<String>,
    pub session: Option<String>,
    pub trace: Option<String>,
    pub since_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub tag: Option<String>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub project_path: Option<String>,
    pub appid: Option<String>,
    pub page: Option<String>,
    pub session: Option<String>,
    pub trace_id: Option<String>,
    pub line_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineRecord {
    pub run_id: String,
    pub seq: i64,
    pub ts: String,
    pub stream: String,
    pub level: String,
    pub event: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub run: RunRecord,
    pub line: LineRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanStats {
    pub deleted_runs: usize,
    pub deleted_lines: usize,
    pub keep_hours: u64,
}

pub struct SqliteStorage {
    conn: Connection,
    fts_available: bool,
}

impl SqliteStorage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut storage = Self {
            conn,
            fts_available: false,
        };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                status TEXT NOT NULL,
                exit_code INTEGER,
                cwd TEXT,
                argv_json TEXT NOT NULL,
                tag TEXT,
                source TEXT,
                client_id TEXT,
                kind TEXT,
                project_path TEXT,
                appid TEXT,
                page TEXT,
                session TEXT,
                trace_id TEXT,
                line_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS log_lines (
                run_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                ts_ms INTEGER NOT NULL,
                stream TEXT NOT NULL,
                level TEXT NOT NULL,
                event TEXT,
                text TEXT NOT NULL,
                meta_json TEXT NOT NULL,
                PRIMARY KEY (run_id, seq),
                FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS run_meta (
                run_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value_json TEXT NOT NULL,
                PRIMARY KEY (run_id, key),
                FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_runs_started ON runs(started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_runs_tag_started ON runs(tag, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_runs_kind_appid_started ON runs(kind, appid, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_runs_page_started ON runs(page, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_lines_run_seq ON log_lines(run_id, seq);
            "#,
        )?;

        self.fts_available = self
            .conn
            .execute(
                "CREATE VIRTUAL TABLE IF NOT EXISTS log_fts USING fts5(run_id UNINDEXED, seq UNINDEXED, text)",
                [],
            )
            .is_ok();
        Ok(())
    }

    pub fn start_run(&mut self, run: NewRun) -> Result<String> {
        self.clean(DEFAULT_KEEP_HOURS, false)?;

        let id = Uuid::new_v4().to_string();
        let meta = meta::normalize_map(&run.meta);
        let common = meta::extract_common(run.kind.as_deref(), &meta);
        let argv_json = serde_json::to_string(&run.argv)?;

        let tx = self.conn.transaction()?;
        tx.execute(
            r#"
            INSERT INTO runs (
                id, started_at_ms, status, cwd, argv_json, tag, source, client_id,
                kind, project_path, appid, page, session, trace_id
            )
            VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                id,
                now_ms(),
                run.cwd,
                argv_json,
                run.tag,
                run.source,
                run.client_id,
                common.kind,
                common.project_path,
                common.appid,
                common.page,
                common.session,
                common.trace_id,
            ],
        )?;
        for (key, value) in meta {
            tx.execute(
                "INSERT OR REPLACE INTO run_meta (run_id, key, value_json) VALUES (?1, ?2, ?3)",
                params![id, key, serde_json::to_string(&value)?],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn append_lines(&mut self, run_id: &str, lines: &[AppendLine]) -> Result<usize> {
        if lines.is_empty() {
            return Ok(0);
        }

        let mut next_seq: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM log_lines WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )?;

        let fts_available = self.fts_available;
        let tx = self.conn.transaction()?;
        for line in lines {
            let ts_ms = match line.ts.as_deref() {
                Some(ts) => parse_rfc3339_ms(ts)?,
                None => now_ms(),
            };
            let stream = line.stream.as_deref().unwrap_or("stdout");
            let level = line.level.as_deref().unwrap_or("info");
            let meta_json = serde_json::to_string(&line.meta)?;
            tx.execute(
                r#"
                INSERT INTO log_lines (run_id, seq, ts_ms, stream, level, event, text, meta_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![run_id, next_seq, ts_ms, stream, level, line.event, line.text, meta_json],
            )?;
            if fts_available {
                tx.execute(
                    "INSERT INTO log_fts (run_id, seq, text) VALUES (?1, ?2, ?3)",
                    params![run_id, next_seq, line.text],
                )?;
            }
            next_seq += 1;
        }
        tx.execute(
            "UPDATE runs SET line_count = line_count + ?2 WHERE id = ?1",
            params![run_id, lines.len() as i64],
        )?;
        tx.commit()?;
        Ok(lines.len())
    }

    pub fn finish_run(&mut self, run_id: &str, finish: &FinishRunRequest) -> Result<()> {
        let status = finish.status.as_deref().unwrap_or("ok");
        self.conn.execute(
            "UPDATE runs SET ended_at_ms = ?2, status = ?3, exit_code = ?4 WHERE id = ?1",
            params![run_id, now_ms(), status, finish.exit_code],
        )?;
        Ok(())
    }

    pub fn list_runs(&mut self, filter: &QueryFilter, limit: usize) -> Result<Vec<RunRecord>> {
        self.clean(DEFAULT_KEEP_HOURS, false)?;
        let (where_sql, mut values) = filter_sql(filter, "runs");
        values.push(SqlValue::Integer(limit as i64));
        let sql = format!(
            "SELECT id, started_at_ms, ended_at_ms, status, exit_code, tag, source, kind, \
             project_path, appid, page, session, trace_id, line_count FROM runs {where_sql} \
             ORDER BY started_at_ms DESC LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), row_to_run)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn show_lines(
        &mut self,
        run_id: &str,
        tail: Option<usize>,
        stream: Option<&str>,
    ) -> Result<Vec<LineRecord>> {
        self.clean(DEFAULT_KEEP_HOURS, false)?;
        let mut values = vec![SqlValue::Text(run_id.to_string())];
        let stream_clause = if let Some(stream) = stream {
            values.push(SqlValue::Text(stream.to_string()));
            " AND stream = ?"
        } else {
            ""
        };

        let sql = if let Some(tail) = tail {
            values.push(SqlValue::Integer(tail as i64));
            format!(
                "SELECT * FROM (SELECT run_id, seq, ts_ms, stream, level, event, text FROM log_lines \
                 WHERE run_id = ? {stream_clause} ORDER BY seq DESC LIMIT ?) ORDER BY seq ASC"
            )
        } else {
            format!(
                "SELECT run_id, seq, ts_ms, stream, level, event, text FROM log_lines \
                 WHERE run_id = ? {stream_clause} ORDER BY seq ASC"
            )
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), row_to_line)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn search(
        &mut self,
        pattern: &str,
        filter: &QueryFilter,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        self.clean(DEFAULT_KEEP_HOURS, false)?;
        let mut local_filter = filter.clone();
        let (where_sql, mut values) = filter_sql(&local_filter, "r");
        let terms = pattern
            .split('|')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let terms = if terms.is_empty() {
            vec![pattern]
        } else {
            terms
        };
        let mut term_clauses = Vec::new();
        for term in terms {
            term_clauses.push("l.text LIKE ?");
            values.push(SqlValue::Text(format!("%{}%", term)));
        }
        let term_sql = term_clauses.join(" OR ");
        let combined_where = if where_sql.is_empty() {
            format!("WHERE ({term_sql})")
        } else {
            format!("{where_sql} AND ({term_sql})")
        };
        values.push(SqlValue::Integer(limit as i64));

        let sql = format!(
            "SELECT r.id, r.started_at_ms, r.ended_at_ms, r.status, r.exit_code, r.tag, r.source, \
             r.kind, r.project_path, r.appid, r.page, r.session, r.trace_id, r.line_count, \
             l.run_id, l.seq, l.ts_ms, l.stream, l.level, l.event, l.text \
             FROM log_lines l JOIN runs r ON r.id = l.run_id {combined_where} \
             ORDER BY l.ts_ms DESC LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), |row| {
            Ok(SearchHit {
                run: row_to_run_offset(row, 0)?,
                line: row_to_line_offset(row, 14)?,
            })
        })?;
        local_filter.run_id.take();
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn clean(&mut self, keep_hours: u64, vacuum: bool) -> Result<CleanStats> {
        let cutoff = now_ms() - (keep_hours as i64 * 60 * 60 * 1000);
        let run_ids = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM runs WHERE COALESCE(ended_at_ms, started_at_ms) < ?1")?;
            let rows = stmt.query_map([cutoff], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut deleted_lines = 0usize;
        let fts_available = self.fts_available;
        let tx = self.conn.transaction()?;
        for run_id in &run_ids {
            deleted_lines += tx.execute("DELETE FROM log_lines WHERE run_id = ?1", [run_id])?;
            tx.execute("DELETE FROM run_meta WHERE run_id = ?1", [run_id])?;
            if fts_available {
                let _ = tx.execute("DELETE FROM log_fts WHERE run_id = ?1", [run_id]);
            }
            tx.execute("DELETE FROM runs WHERE id = ?1", [run_id])?;
        }
        tx.commit()?;

        if vacuum {
            self.conn.execute_batch("VACUUM")?;
        }

        Ok(CleanStats {
            deleted_runs: run_ids.len(),
            deleted_lines,
            keep_hours,
        })
    }
}

fn filter_sql(filter: &QueryFilter, table: &str) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    let mut add_text = |column: &str, value: &Option<String>| {
        if let Some(value) = value {
            clauses.push(format!("{table}.{column} = ?"));
            values.push(SqlValue::Text(value.clone()));
        }
    };
    add_text("id", &filter.run_id);
    add_text("tag", &filter.tag);
    add_text("kind", &filter.kind);
    add_text("appid", &filter.appid);
    add_text("project_path", &filter.project);
    add_text("page", &filter.page);
    add_text("session", &filter.session);
    add_text("trace_id", &filter.trace);
    if let Some(since_ms) = filter.since_ms {
        clauses.push(format!("{table}.started_at_ms >= ?"));
        values.push(SqlValue::Integer(since_ms));
    }

    if clauses.is_empty() {
        (String::new(), values)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), values)
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    row_to_run_offset(row, 0)
}

fn row_to_run_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<RunRecord> {
    let started_ms: i64 = row.get(offset + 1)?;
    let ended_ms: Option<i64> = row.get(offset + 2)?;
    Ok(RunRecord {
        id: row.get(offset)?,
        started_at: format_ms(started_ms),
        ended_at: ended_ms.map(format_ms),
        status: row.get(offset + 3)?,
        exit_code: row.get(offset + 4)?,
        tag: row.get(offset + 5)?,
        source: row.get(offset + 6)?,
        kind: row.get(offset + 7)?,
        project_path: row.get(offset + 8)?,
        appid: row.get(offset + 9)?,
        page: row.get(offset + 10)?,
        session: row.get(offset + 11)?,
        trace_id: row.get(offset + 12)?,
        line_count: row.get(offset + 13)?,
    })
}

fn row_to_line(row: &rusqlite::Row<'_>) -> rusqlite::Result<LineRecord> {
    row_to_line_offset(row, 0)
}

fn row_to_line_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<LineRecord> {
    let ts_ms: i64 = row.get(offset + 2)?;
    Ok(LineRecord {
        run_id: row.get(offset)?,
        seq: row.get(offset + 1)?,
        ts: format_ms(ts_ms),
        stream: row.get(offset + 3)?,
        level: row.get(offset + 4)?,
        event: row.get(offset + 5)?,
        text: row.get(offset + 6)?,
    })
}

pub fn now_ms() -> i64 {
    let now = OffsetDateTime::now_utc();
    now.unix_timestamp() * 1000 + i64::from(now.millisecond())
}

pub fn parse_rfc3339_ms(input: &str) -> Result<i64> {
    let dt = OffsetDateTime::parse(input, &Rfc3339)
        .map_err(|e| Error::TimeParse(format!("{} ({})", input, e)))?;
    Ok(dt.unix_timestamp() * 1000 + i64::from(dt.millisecond()))
}

pub fn format_ms(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) as i128 * 1_000_000) as i64;
    let dt = OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(nanos);
    dt.format(&Rfc3339).unwrap_or_else(|_| seconds.to_string())
}

pub fn parse_since(input: &str) -> Result<i64> {
    if let Ok(duration) = humantime::parse_duration(input) {
        return Ok(now_ms() - duration.as_millis() as i64);
    }
    parse_rfc3339_ms(input)
}

pub fn default_db_path() -> std::path::PathBuf {
    if let Some(state) = dirs::state_dir() {
        return state.join("looplog").join("looplog.db");
    }
    if let Some(data) = dirs::data_dir() {
        return data.join("looplog").join("looplog.db");
    }
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("looplog.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn stores_and_queries_wechat_meta() {
        let tmp = NamedTempFile::new().unwrap();
        let mut db = SqliteStorage::open(tmp.path()).unwrap();
        let mut meta = BTreeMap::new();
        meta.insert("appid".to_string(), Value::String("wx123".to_string()));
        meta.insert(
            "page".to_string(),
            Value::String("pages/index/index".to_string()),
        );
        let run_id = db
            .start_run(NewRun {
                tag: Some("wx-console".to_string()),
                source: Some("wechat-devtools".to_string()),
                cwd: None,
                argv: vec![],
                client_id: None,
                kind: Some("wechat_miniprogram".to_string()),
                meta,
            })
            .unwrap();
        db.append_lines(
            &run_id,
            &[AppendLine {
                level: Some("error".to_string()),
                text: "TypeError: boom".to_string(),
                ..Default::default()
            }],
        )
        .unwrap();

        let hits = db
            .search(
                "TypeError",
                &QueryFilter {
                    appid: Some("wx123".to_string()),
                    ..Default::default()
                },
                10,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].run.id, run_id);
    }
}
