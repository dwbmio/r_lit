use std::path::PathBuf;

use serde::Serialize;
use tinify::async_bin::Tinify;
use walkdir::WalkDir;

use crate::error::ReError;

#[derive(Serialize)]
struct TinifyResult {
    file: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub struct TinifyExecutor {
    tp: PathBuf,
    tinify_inc: Tinify,
    out: Option<PathBuf>,
    json_output: bool,
}

impl TinifyExecutor {
    async fn single_tp(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<TinifyResult, ReError> {
        if !self.json_output {
            log::info!("tinyfy api handle file: {}", path.display());
        }

        let output_file_name = path.file_name().unwrap().to_str().unwrap();
        match self
            .tinify_inc
            .get_async_client()?
            .from_file(path)
            .await
        {
            Ok(mut client) => {
                match client
                    .to_file(out.unwrap_or_default().join(output_file_name))
                    .await
                {
                    Ok(_) => Ok(TinifyResult {
                        file: path.display().to_string(),
                        status: "success".to_string(),
                        error: None,
                    }),
                    Err(e) => Ok(TinifyResult {
                        file: path.display().to_string(),
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    }),
                }
            }
            Err(e) => Ok(TinifyResult {
                file: path.display().to_string(),
                status: "failed".to_string(),
                error: Some(e.to_string()),
            }),
        }
    }

    async fn walk(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<Vec<TinifyResult>, ReError> {
        let walker = WalkDir::new(path).into_iter();
        if !self.json_output {
            log::debug!("start walk dir :{}...", path.display());
        }

        let mut results = Vec::new();
        for entry in walker.filter_entry(|e| !Self::is_hidden(e)) {
            if !self.json_output {
                log::debug!("entry:{:?}", entry);
            }
            let entry = entry?;
            if entry.path().is_file() {
                let result = self.single_tp(&entry.path().to_path_buf(), out.clone()).await?;
                results.push(result);
            }
        }
        Ok(results)
    }

    fn is_hidden(entry: &walkdir::DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    async fn exec_tinify(&self) -> Result<(), ReError> {
        if !self.tp.exists() {
            return Err(ReError::CustomError("path not exists!".to_string()));
        }

        let results = match self.tp.is_file() {
            true => vec![self.single_tp(&self.tp, self.out.to_owned()).await?],
            false => self.walk(&self.tp, self.out.to_owned()).await?,
        };

        if self.json_output {
            let summary = serde_json::json!({
                "total": results.len(),
                "results": results
            });
            println!("{}", serde_json::to_string(&summary)?);
        }

        Ok(())
    }
}

pub async fn exec(path: &PathBuf, _do_size_perf: bool, json_output: bool) -> Result<(), ReError> {
    let tinify_inc = Tinify::new().set_key("YXDshBjDdCFXnJPSwM8lFRvMhbBfDW5m");
    let executor = TinifyExecutor {
        tinify_inc,
        tp: path.clone(),
        out: Some(PathBuf::new()),
        json_output,
    };
    executor.exec_tinify().await?;
    Ok(())
}
