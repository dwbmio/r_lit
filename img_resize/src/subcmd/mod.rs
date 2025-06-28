use std::path::PathBuf;

use walkdir::WalkDir;

use crate::error::ReError;

pub mod r_tp;
pub mod tinify_tp;

pub trait SubExecutor {
    async fn exec(&self, matches: &clap::ArgMatches) -> Result<(), crate::error::ReError>;
    async fn single_tp(
        &self,
        path: &PathBuf,
        out: Option<PathBuf>,
    ) -> Result<(), crate::error::ReError>;
    async fn walk(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<(), ReError> {
        let walker = WalkDir::new(path).into_iter();
        log::debug!("start walk dir :{}...", path.display());
        for entry in walker.filter_entry(|e| !Self::is_hidden(e)) {
            log::debug!("entry:{:?}", entry);
            let entry = entry?;
            if entry.path().is_file() {
                self.single_tp(&entry.path().to_path_buf(), out.clone())
                    .await?;
            }
        }
        Ok(())
    }

    fn is_hidden(entry: &walkdir::DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }
}
