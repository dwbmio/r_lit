use crate::db::{CleanStats, LineRecord, RunRecord, SearchHit};
use crate::error::Result;
use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn print_runs(runs: &[RunRecord]) {
    for run in runs {
        println!(
            "{}  {}  tag={} appid={} page={} lines={} status={}",
            run.started_at,
            short_id(&run.id),
            run.tag.as_deref().unwrap_or("-"),
            run.appid.as_deref().unwrap_or("-"),
            run.page.as_deref().unwrap_or("-"),
            run.line_count,
            run.status
        );
    }
}

pub fn print_lines(lines: &[LineRecord]) {
    for line in lines {
        println!("{} [{} {}] {}", line.ts, line.stream, line.level, line.text);
    }
}

pub fn print_hits(hits: &[SearchHit]) {
    for hit in hits {
        println!(
            "{} {} appid={} page={} [{} {}] {}",
            hit.line.ts,
            short_id(&hit.run.id),
            hit.run.appid.as_deref().unwrap_or("-"),
            hit.run.page.as_deref().unwrap_or("-"),
            hit.line.stream,
            hit.line.level,
            hit.line.text
        );
    }
}

pub fn print_clean(stats: &CleanStats) {
    println!(
        "deleted {} run(s), {} line(s); keep_hours={}",
        stats.deleted_runs, stats.deleted_lines, stats.keep_hours
    );
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
