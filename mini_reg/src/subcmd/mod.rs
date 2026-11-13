use crate::{BrowserCommand, Commands};
use mini_reg::{
    browser::{Binding, Browser, Selectors},
    error::{invalid, Result},
    model::{Batch, EXAMPLE},
    store::Store,
};
use serde_json::{json, Value};
use std::{fs::OpenOptions, io::Write, path::Path};

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn execute(command: &Commands, db: &Path) -> Result<Value> {
    if let Commands::Init { output } = command {
        write_new(output, EXAMPLE.as_bytes())?;
        return Ok(json!({"manifest": output, "schema_version": 1}));
    }
    let mut store = Store::open(db)?;
    match command {
        Commands::Init { .. } => Err(invalid("Unexpected init dispatch / 命令分发不符")),
        Commands::Import { manifest } => Ok(serde_json::to_value(
            store.import(&Batch::read(manifest)?)?,
        )?),
        Commands::List { batch } => Ok(serde_json::to_value(store.list(batch.as_deref())?)?),
        Commands::Show { id } => {
            let job = store.job(id)?;
            Ok(json!({"portal": job.app.platform.portal(), "task": job}))
        }
        Commands::Open { id } => {
            let job = store.job(id)?;
            if job.phase == "confirmed" {
                return Err(invalid("Task already confirmed / 任务已确认"));
            }
            let browser = Browser::connect(store.setting("browser")?)?;
            let tab = browser.open(job.app.platform)?;
            store.transition(id, "opened", Some(&tab))?;
            Ok(
                json!({"id": id, "tab": tab, "phase": "opened", "next": "Log in and open the correct creation form / 登录后打开对应类型的创建表单"}),
            )
        }
        Commands::Fill { id, tab, apply } => {
            let job = store.job(id)?;
            if job.phase == "confirmed" {
                return Err(invalid("Task already confirmed / 任务已确认"));
            }
            let tab = tab
                .as_ref()
                .or(job.tab_id.as_ref())
                .ok_or_else(|| invalid("Run open or pass --tab / 请先 open 或用 --tab 指定页面"))?;
            let browser = Browser::connect(store.setting("browser")?)?;
            let key = format!("selectors_{}", job.app.platform.key());
            let selectors: Selectors = store.optional_setting(&key)?.unwrap_or_default();
            // Checkpoint BEFORE touching the page. A crash/error never looks like success.
            if *apply {
                store.transition(id, "needs_review", Some(tab))?;
            }
            let report = browser.form(&job.app, tab, &selectors, *apply)?;
            if *apply {
                store.transition(id, "waiting_human", Some(tab))?;
            }
            Ok(json!({"id": id, "kind": job.app.kind, "form": report, "platform_verified": false}))
        }
        Commands::Confirm { id, app_id } => {
            store.confirm(id, app_id)?;
            Ok(serde_json::to_value(store.job(id)?)?)
        }
        Commands::Export { output, batch } => {
            let jobs = store.list(batch.as_deref())?;
            let bytes = serde_json::to_vec_pretty(&json!({"schema_version": 1, "tasks": jobs}))?;
            write_new(output, &bytes)?;
            Ok(json!({"output": output, "tasks": jobs.len(), "contains_login_data": false}))
        }
        Commands::Browser {
            command:
                BrowserCommand::Bind {
                    user_data_dir,
                    profile_directory,
                },
        } => {
            let binding = Binding::from_profile(user_data_dir, profile_directory)?;
            let _browser = Browser::connect(binding.clone())?;
            store.save_setting("browser", &binding)?;
            Ok(
                json!({"bound": true, "enterprise_domain": "gamesci-lite.com", "port": binding.port}),
            )
        }
        Commands::Browser {
            command: BrowserCommand::Tabs,
        } => {
            let browser = Browser::connect(store.setting("browser")?)?;
            Ok(serde_json::to_value(browser.tabs()?)?)
        }
        Commands::Selectors {
            platform,
            name,
            summary,
        } => {
            for selector in [name, summary].into_iter().flatten() {
                if selector.trim().is_empty() || selector.len() > 512 || selector.contains('\n') {
                    return Err(invalid("Invalid selector / 表单选择器不符"));
                }
            }
            let selectors = Selectors {
                name: name.clone(),
                summary: summary.clone(),
            };
            store.save_setting(&format!("selectors_{}", platform.key()), &selectors)?;
            Ok(serde_json::to_value(selectors)?)
        }
    }
}
