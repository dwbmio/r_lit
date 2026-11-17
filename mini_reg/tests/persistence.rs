use mini_reg::{
    browser::{checked_portal, checked_websocket, Binding},
    model::{Batch, Platform, EXAMPLE},
    store::Store,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, std::io::Error> {
        let base = std::env::var_os("KIROCREW_SCRATCH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-state"));
        let path = base.join(format!(
            "mini-reg-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
    fn db(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn batch() -> Result<Batch, toml::de::Error> {
    toml::from_str(EXAMPLE)
}

#[test]
fn repeated_import_preserves_confirmation_after_restart() -> TestResult {
    let fixture = Fixture::new()?;
    let batch = batch()?;
    {
        let mut store = Store::open(&fixture.db())?;
        assert_eq!(store.import(&batch)?.inserted, 4);
        store.confirm("wechat-demo", "wx0123456789abcdef")?;
    }
    let mut store = Store::open(&fixture.db())?;
    let report = store.import(&batch)?;
    assert_eq!(report.inserted, 0);
    assert_eq!(report.unchanged, 4);
    let job = store.job("wechat-demo")?;
    assert_eq!(job.phase, "confirmed");
    assert_eq!(job.evidence.as_deref(), Some("user_confirmation"));
    assert!(!job.platform_verified);
    assert!(store.transition("wechat-demo", "opened", None).is_err());
    assert!(store.confirm("wechat-demo", "wx1111111111111111").is_err());
    Ok(())
}

#[test]
fn conflicting_import_rolls_back_the_entire_batch() -> TestResult {
    let fixture = Fixture::new()?;
    let original = batch()?;
    let mut store = Store::open(&fixture.db())?;
    store.import(&original)?;
    let mut changed = original.clone();
    changed.apps[0].id = "new-task-before-conflict".into();
    changed.apps[1].name = "Changed existing data".into();
    assert!(store.import(&changed).is_err());
    assert!(store.job("new-task-before-conflict").is_err());
    assert_eq!(store.list(None)?.len(), 4);
    Ok(())
}

#[test]
fn interrupted_apply_remains_needs_review_after_restart() -> TestResult {
    let fixture = Fixture::new()?;
    {
        let mut store = Store::open(&fixture.db())?;
        store.import(&batch()?)?;
        store.transition("qq-demo", "opened", Some("portal-tab"))?;
        store.transition("qq-demo", "needs_review", None)?;
    }
    let store = Store::open(&fixture.db())?;
    let job = store.job("qq-demo")?;
    assert_eq!(job.phase, "needs_review");
    assert_eq!(job.tab_id.as_deref(), Some("portal-tab"));
    Ok(())
}

#[test]
fn app_id_cannot_be_reused_by_a_second_task() -> TestResult {
    let fixture = Fixture::new()?;
    let mut batch = batch()?;
    let mut second = batch.apps[0].clone();
    second.id = "wechat-second".into();
    second.name = "Another app".into();
    batch.apps.push(second);
    let mut store = Store::open(&fixture.db())?;
    store.import(&batch)?;
    store.confirm("wechat-demo", "wx0123456789abcdef")?;
    assert!(store
        .confirm("wechat-second", "wx0123456789abcdef")
        .is_err());
    assert_eq!(store.job("wechat-second")?.phase, "planned");
    Ok(())
}

#[test]
fn manifests_require_an_explicit_app_kind_and_reject_secret_fields() -> TestResult {
    let no_kind = EXAMPLE.replace("kind = \"mini_program\"", "");
    assert!(toml::from_str::<Batch>(&no_kind).is_err());
    let secret = EXAMPLE.replace(
        "schema_version = 1",
        "app_secret = \"do-not-print-this\"\nschema_version = 1",
    );
    let error = toml::from_str::<Batch>(&secret).expect_err("unknown credentials must be rejected");
    let safe = mini_reg::error::RegError::from(error).to_string();
    assert!(!safe.contains("do-not-print-this"));
    Ok(())
}

#[test]
fn origins_and_debugger_endpoints_are_strict() -> TestResult {
    for url in [
        "http://mp.weixin.qq.com/",
        "https://mp.weixin.qq.com.evil.test/",
        "https://q.qq.com/",
        "https://u:p@mp.weixin.qq.com/",
        "https://mp.weixin.qq.com:9443/",
    ] {
        assert!(checked_portal(url, Platform::Wechat).is_err());
    }
    assert!(checked_portal("https://mp.weixin.qq.com/", Platform::Wechat).is_ok());
    for url in [
        "ws://192.168.1.1:9222/devtools/page/a",
        "ws://127.0.0.1:9223/devtools/page/a",
        "wss://127.0.0.1:9222/devtools/page/a",
        "ws://127.0.0.1:9222/devtools/page/a?token=x",
    ] {
        assert!(checked_websocket(url, 9222).is_err());
    }
    assert!(checked_websocket("ws://127.0.0.1:9222/devtools/page/a", 9222).is_ok());
    Ok(())
}

fn profile(fixture: &Fixture, email: &str, domain: &str) -> TestResult {
    std::fs::create_dir_all(fixture.0.join("Company"))?;
    std::fs::write(
        fixture.0.join("Company/Preferences"),
        serde_json::to_vec(
            &serde_json::json!({"account_info": [{"email": email, "hosted_domain": domain}]}),
        )?,
    )?;
    std::fs::write(
        fixture.0.join("Local State"),
        r#"{"profile":{"last_used":"Company","info_cache":{"Company":{}}}}"#,
    )?;
    std::fs::write(
        fixture.0.join("DevToolsActivePort"),
        "9222\n/devtools/browser/process-1\n",
    )?;
    Ok(())
}

#[test]
fn refuses_personal_or_shared_profiles_and_changed_processes() -> TestResult {
    let fixture = Fixture::new()?;
    profile(&fixture, "person@gmail.com", "")?;
    assert!(Binding::from_profile(&fixture.0, "Company").is_err());
    profile(&fixture, "operator@gamesci-lite.com", "gamesci-lite.com")?;
    let binding = Binding::from_profile(&fixture.0, "Company")?;
    std::fs::write(
        fixture.0.join("DevToolsActivePort"),
        "9222\n/devtools/browser/process-2\n",
    )?;
    assert!(binding.verify().is_err());
    profile(&fixture, "operator@gamesci-lite.com", "gamesci-lite.com")?;
    std::fs::write(
        fixture.0.join("Local State"),
        r#"{"profile":{"last_used":"Company","info_cache":{"Company":{},"Personal":{}}}}"#,
    )?;
    assert!(Binding::from_profile(&fixture.0, "Company").is_err());
    assert!(Binding::from_profile(&fixture.0, "../Company").is_err());
    Ok(())
}

#[test]
fn cli_restart_and_export_keep_stdout_machine_readable() -> TestResult {
    let fixture = Fixture::new()?;
    let manifest = fixture.0.join("batch.toml");
    std::fs::write(&manifest, EXAMPLE)?;
    let run = |args: &[&str]| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mini_reg"))
            .arg("--db")
            .arg(fixture.db())
            .arg("--json")
            .args(args)
            .output()?;
        assert!(
            output.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    run(&["import", manifest.to_str().ok_or("non-UTF8 test path")?])?;
    run(&["confirm", "qq-demo", "--app-id", "1234567890"])?;
    assert_eq!(
        run(&["next"])?["result"]["task"]["app"]["id"],
        "alipay-demo"
    );
    run(&["confirm", "alipay-demo", "--app-id", "2026000000000001"])?;
    assert_eq!(
        run(&["next"])?["result"]["task"]["app"]["id"],
        "douyin-demo"
    );
    let list = run(&["list"])?;
    assert_eq!(
        list["result"]
            .as_array()
            .ok_or("result not an array")?
            .len(),
        4
    );
    let export = fixture.0.join("result.json");
    run(&["export", export.to_str().ok_or("non-UTF8 test path")?])?;
    let exported: serde_json::Value = serde_json::from_slice(&std::fs::read(export)?)?;
    assert_eq!(
        exported["tasks"]
            .as_array()
            .ok_or("tasks not an array")?
            .len(),
        4
    );
    assert!(!serde_json::to_string(&exported)?.contains("user_data_dir"));
    Ok(())
}
