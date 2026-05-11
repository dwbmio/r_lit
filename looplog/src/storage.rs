use crate::db::{CleanStats, LineRecord, NewRun, QueryFilter, RunRecord, SearchHit, SqliteStorage};
use crate::error::Result;
use crate::protocol::{AppendLine, FinishRunRequest};
use std::path::Path;

#[allow(dead_code)]
pub trait Storage {
    fn open(path: &Path) -> Result<Self>
    where
        Self: Sized;
    fn start_run(&mut self, run: NewRun) -> Result<String>;
    fn append_lines(&mut self, run_id: &str, lines: &[AppendLine]) -> Result<usize>;
    fn finish_run(&mut self, run_id: &str, finish: &FinishRunRequest) -> Result<()>;
    fn list_runs(&mut self, filter: &QueryFilter, limit: usize) -> Result<Vec<RunRecord>>;
    fn show_lines(
        &mut self,
        run_id: &str,
        tail: Option<usize>,
        stream: Option<&str>,
    ) -> Result<Vec<LineRecord>>;
    fn search(
        &mut self,
        pattern: &str,
        filter: &QueryFilter,
        limit: usize,
    ) -> Result<Vec<SearchHit>>;
    fn clean(&mut self, keep_hours: u64, vacuum: bool) -> Result<CleanStats>;
}

impl Storage for SqliteStorage {
    fn open(path: &Path) -> Result<Self> {
        SqliteStorage::open(path)
    }

    fn start_run(&mut self, run: NewRun) -> Result<String> {
        SqliteStorage::start_run(self, run)
    }

    fn append_lines(&mut self, run_id: &str, lines: &[AppendLine]) -> Result<usize> {
        SqliteStorage::append_lines(self, run_id, lines)
    }

    fn finish_run(&mut self, run_id: &str, finish: &FinishRunRequest) -> Result<()> {
        SqliteStorage::finish_run(self, run_id, finish)
    }

    fn list_runs(&mut self, filter: &QueryFilter, limit: usize) -> Result<Vec<RunRecord>> {
        SqliteStorage::list_runs(self, filter, limit)
    }

    fn show_lines(
        &mut self,
        run_id: &str,
        tail: Option<usize>,
        stream: Option<&str>,
    ) -> Result<Vec<LineRecord>> {
        SqliteStorage::show_lines(self, run_id, tail, stream)
    }

    fn search(
        &mut self,
        pattern: &str,
        filter: &QueryFilter,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        SqliteStorage::search(self, pattern, filter, limit)
    }

    fn clean(&mut self, keep_hours: u64, vacuum: bool) -> Result<CleanStats> {
        SqliteStorage::clean(self, keep_hours, vacuum)
    }
}
