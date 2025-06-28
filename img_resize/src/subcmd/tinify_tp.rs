use std::path::PathBuf;

use clap::ArgMatches;
use tinify::async_bin::Tinify;

use crate::{error::ReError, subcmd::SubExecutor};

pub struct TinifyExecutor {
    tp: PathBuf,
    tinify_inc: Tinify,
    out: Option<PathBuf>,
}

impl SubExecutor for TinifyExecutor {
    async fn exec(&self, m: &clap::ArgMatches) -> Result<(), crate::error::ReError> {
        let _ = &self.walk(&self.tp, Some(self.tp.clone())).await?;
        Ok(())
    }

    async fn single_tp(
        &self,
        path: &std::path::PathBuf,
        out: Option<std::path::PathBuf>,
    ) -> Result<(), crate::error::ReError> {
        log::info!("tinyfy api handle file: {}", path.display());
        let output_file_name = path.file_name().unwrap().to_str().unwrap();
        let c = self
            .tinify_inc
            .get_async_client()?
            .from_file(path)
            .await?
            .to_file(out.unwrap_or_default().join(output_file_name))
            .await?;
        Ok(c)
    }
}

pub async fn exec(m: &ArgMatches) -> Result<(), ReError> {
    let tinify_inc = Tinify::new().set_key("YXDshBjDdCFXnJPSwM8lFRvMhbBfDW5m");
    let opt_tp = m.get_one::<PathBuf>("path");
    let o = TinifyExecutor {
        tinify_inc: tinify_inc,
        tp: opt_tp.unwrap().to_owned(),
        out: Some(PathBuf::new()),
    };
    o.exec(m).await?;
    Ok(())
}
