use crate::{
    error::{invalid, Result},
    model::{App, Batch, Job},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{fs::OpenOptions, path::Path, time::Duration};

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub inserted: usize,
    pub unchanged: usize,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
                }
            }
        }
        if path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(invalid("Refusing a symlink database / 状态库不可为软链接"));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                tx.execute_batch(include_str!("schema.sql"))?;
            }
            1 => (),
            _ => return Err(invalid("Unsupported state version / 请使用匹配的新版程序")),
        }
        tx.commit()?;
        Ok(Self { conn })
    }

    pub fn import(&mut self, batch: &Batch) -> Result<ImportReport> {
        batch.validate()?;
        let tx = self.conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT company FROM batches WHERE id=?1",
                [&batch.id],
                |r| r.get(0),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|company| company != &batch.company)
        {
            return Err(invalid("Batch company changed / 已有批次的公司不可更改"));
        }
        tx.execute(
            "INSERT OR IGNORE INTO batches(id,company) VALUES(?1,?2)",
            params![batch.id, batch.company],
        )?;
        let mut report = ImportReport {
            inserted: 0,
            unchanged: 0,
        };
        for app in &batch.apps {
            let payload = serde_json::to_string(app)?;
            let previous: Option<(String, String)> = tx
                .query_row(
                    "SELECT batch_id,payload FROM jobs WHERE id=?1",
                    [&app.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match previous {
                Some((batch_id, old)) if batch_id == batch.id && old == payload => {
                    report.unchanged += 1;
                }
                Some(_) => {
                    return Err(invalid(
                        "Task already exists with different data / 同编号任务内容冲突，整批未导入",
                    ))
                }
                None => {
                    tx.execute(
                        "INSERT INTO jobs(id,batch_id,platform,payload) VALUES(?1,?2,?3,?4)",
                        params![app.id, batch.id, app.platform.key(), payload],
                    )?;
                    tx.execute(
                        "INSERT INTO events(job_id,action) VALUES(?1,'import')",
                        [&app.id],
                    )?;
                    report.inserted += 1;
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }

    pub fn list(&self, batch: Option<&str>) -> Result<Vec<Job>> {
        let mut stmt = self.conn.prepare(
            "SELECT j.batch_id,b.company,j.payload,j.phase,j.tab_id,j.app_id,j.evidence
             FROM jobs j JOIN batches b ON b.id=j.batch_id
             WHERE (?1 IS NULL OR j.batch_id=?1) ORDER BY j.batch_id,j.id",
        )?;
        let rows = stmt.query_map([batch], |r| {
            let payload: String = r.get(2)?;
            let app: App = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(Job {
                batch: r.get(0)?,
                company: r.get(1)?,
                app,
                phase: r.get(3)?,
                tab_id: r.get(4)?,
                app_id: r.get(5)?,
                evidence: r.get(6)?,
                platform_verified: false,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn job(&self, id: &str) -> Result<Job> {
        self.list(None)?
            .into_iter()
            .find(|j| j.app.id == id)
            .ok_or_else(|| invalid("Unknown task / 未找到任务"))
    }

    pub fn transition(&mut self, id: &str, phase: &str, tab: Option<&str>) -> Result<()> {
        if !["opened", "needs_review", "waiting_human"].contains(&phase) {
            return Err(invalid("Invalid transition / 状态转换不符"));
        }
        let tx = self.conn.transaction()?;
        let count = tx.execute(
            "UPDATE jobs SET phase=?1,tab_id=COALESCE(?2,tab_id),
             updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?3 AND phase!='confirmed'",
            params![phase, tab, id],
        )?;
        if count != 1 {
            return Err(invalid(
                "Task missing or already confirmed / 任务不存在或已确认",
            ));
        }
        tx.execute(
            "INSERT INTO events(job_id,action) VALUES(?1,?2)",
            params![id, phase],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn confirm(&mut self, id: &str, app_id: &str) -> Result<()> {
        let job = self.job(id)?;
        job.app.platform.validate_app_id(app_id)?;
        if job.phase == "confirmed" {
            return if job.app_id.as_deref() == Some(app_id) {
                Ok(())
            } else {
                Err(invalid(
                    "Confirmed AppID cannot change / 已确认的应用编号不可覆盖",
                ))
            };
        }
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE jobs SET phase='confirmed',app_id=?1,evidence='user_confirmation',
             updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?2 AND phase!='confirmed'",
            params![app_id, id],
        )?;
        if changed != 1 {
            return Err(invalid(
                "Task changed concurrently; read it again / 任务已被其他进程更改，请重新查看",
            ));
        }
        tx.execute(
            "INSERT INTO events(job_id,action) VALUES(?1,'user_confirmation')",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn save_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T> {
        self.optional_setting(key)?
            .ok_or_else(|| invalid("Run browser bind first / 请先绑定企业浏览器"))
    }

    pub fn optional_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }
}
