//! `__schema` export: walk a clap command tree and describe every visible
//! leaf subcommand as a JSON Schema (draft 2020-12) plus `x-cli` hints that
//! tell an executor how to turn a JSON object back into argv.

use crate::{Contract, ErrorKind, CONTRACT_VERSION, SCHEMA_COMMAND};
use clap::{Arg, ArgAction, Command};
use serde_json::{json, Map, Value};
use std::any::TypeId;

/// Arg ids that are part of the calling convention, not of a command's input.
const RESERVED: &[&str] = &["json", "help", "version"];

/// Build the schema document for `cmd` annotated with `contract`.
pub fn schema_for(contract: &Contract, cmd: &Command) -> Value {
    let mut cmd = cmd.clone();
    // Propagates global args into subcommands and fills in derived settings.
    cmd.build();

    let mut commands = Vec::new();
    collect(contract, &cmd, &mut Vec::new(), &mut commands);

    let exit_codes: Map<String, Value> = std::iter::once(("0".to_string(), json!("ok")))
        .chain(
            ErrorKind::all()
                .iter()
                .map(|k| (k.exit_code().to_string(), json!(k.code()))),
        )
        .collect();

    json!({
        "contract": CONTRACT_VERSION,
        "tool": contract.tool,
        "version": contract.version,
        "about": text(cmd.get_about()),
        "long_about": text(cmd.get_long_about()),
        "invocation": {
            "binary": contract.tool,
            "json_flag": "--json",
            "schema_command": SCHEMA_COMMAND,
            "stdout": "exactly one JSON envelope when --json is given",
            "stderr": "human-readable logs only",
            "interactive": false,
        },
        "exit_codes": exit_codes,
        "commands": commands,
    })
}

fn collect(contract: &Contract, cmd: &Command, path: &mut Vec<String>, out: &mut Vec<Value>) {
    let subs: Vec<&Command> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        .collect();
    // Only leaves are callable units; a tool without subcommands is its own leaf.
    if subs.is_empty() {
        out.push(describe(contract, cmd, path));
        return;
    }
    for s in subs {
        path.push(s.get_name().to_string());
        collect(contract, s, path, out);
        path.pop();
    }
}

fn visible(a: &Arg) -> bool {
    !a.is_hide_set() && !RESERVED.contains(&a.get_id().as_str())
}

fn describe(contract: &Contract, cmd: &Command, path: &[String]) -> Value {
    let joined = path.join(" ");
    let spec = contract.spec(&joined);
    let io_of = |id: &str| {
        spec.and_then(|s| s.io.iter().find(|(a, _)| *a == id).map(|(_, io)| io.as_str()))
    };

    let mut props = Map::new();
    let mut required = Vec::new();
    for a in cmd.get_arguments().filter(|a| visible(a)) {
        let id = a.get_id().as_str().to_string();
        let mut p = arg_schema(a);
        let mut xcli = Map::new();
        if let Some(i) = a.get_index() {
            xcli.insert("positional".into(), json!(i));
        }
        if let Some(l) = a.get_long() {
            xcli.insert("long".into(), json!(format!("--{}", l)));
        }
        if let Some(s) = a.get_short() {
            xcli.insert("short".into(), json!(format!("-{}", s)));
        }
        xcli.insert("action".into(), json!(action_name(a.get_action())));
        if let Some(names) = a.get_value_names() {
            let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            xcli.insert("value_name".into(), json!(names.join(" ")));
        }
        if let Some(io) = io_of(&id) {
            xcli.insert("io".into(), json!(io));
        }
        if let Some(env) = a.get_env() {
            xcli.insert("env".into(), json!(env.to_string_lossy()));
        }
        let conflicts: Vec<String> = cmd
            .get_arg_conflicts_with(a)
            .iter()
            .map(|c| c.get_id().as_str().to_string())
            .filter(|c| !RESERVED.contains(&c.as_str()))
            .collect();
        if !conflicts.is_empty() {
            xcli.insert("conflicts_with".into(), json!(conflicts));
        }
        if a.is_global_set() {
            xcli.insert("global".into(), json!(true));
        }
        p.insert("x-cli".into(), Value::Object(xcli));
        if a.is_required_set() {
            required.push(id.clone());
        }
        props.insert(id, Value::Object(p));
    }

    let mut input = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": props,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        input["required"] = json!(required);
    }

    let mut out = json!({
        "name": if joined.is_empty() { contract.tool.to_string() } else { joined.clone() },
        "argv_prefix": path,
        "about": text(cmd.get_about()),
        "input_schema": input,
    });
    if let Some(la) = text(cmd.get_long_about()) {
        out["long_about"] = json!(la);
    }
    if let Some(ah) = text(cmd.get_after_help()) {
        out["after_help"] = json!(ah);
    }
    match spec {
        Some(s) => {
            out["x-effects"] = json!(s.effects.as_str());
            out["x-env"] = json!(s.env);
            if let Some(stdin) = s.stdin {
                out["x-stdin"] = json!(stdin);
            }
            if !s.note.is_empty() {
                out["x-note"] = json!(s.note);
            }
        }
        None => {
            // Undeclared commands are flagged so a missing annotation is
            // visible (and caught by the contract test).
            out["x-effects"] = json!("undeclared");
            out["x-env"] = json!([]);
        }
    }
    out
}

fn arg_schema(a: &Arg) -> Map<String, Value> {
    let mut p = Map::new();
    let desc = text(a.get_long_help()).or_else(|| text(a.get_help()));
    if let Some(d) = desc {
        p.insert("description".into(), json!(d));
    }

    let multiple = matches!(a.get_action(), ArgAction::Append)
        || a.get_num_args().map(|r| r.max_values() > 1).unwrap_or(false);

    let item = match a.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => {
            p.insert("type".into(), json!("boolean"));
            let default = a
                .get_default_values()
                .first()
                .and_then(|d| d.to_str())
                .and_then(|d| d.parse::<bool>().ok())
                .unwrap_or(matches!(a.get_action(), ArgAction::SetFalse));
            p.insert("default".into(), json!(default));
            return p;
        }
        ArgAction::Count => {
            p.insert("type".into(), json!("integer"));
            p.insert("minimum".into(), json!(0));
            return p;
        }
        _ => value_type(a),
    };

    let defaults: Vec<Value> = a
        .get_default_values()
        .iter()
        .map(|d| typed_default(&d.to_string_lossy(), &item))
        .collect();

    if multiple {
        p.insert("type".into(), json!("array"));
        p.insert("items".into(), Value::Object(item));
        if !defaults.is_empty() {
            p.insert("default".into(), json!(defaults));
        }
    } else {
        for (k, v) in item {
            p.insert(k, v);
        }
        if let Some(d) = defaults.into_iter().next() {
            p.insert("default".into(), d);
        }
    }
    p
}

fn value_type(a: &Arg) -> Map<String, Value> {
    let mut m = Map::new();
    let pv: Vec<String> = a
        .get_possible_values()
        .iter()
        .filter(|v| !v.is_hide_set())
        .map(|v| v.get_name().to_string())
        .collect();
    if !pv.is_empty() {
        m.insert("type".into(), json!("string"));
        m.insert("enum".into(), json!(pv));
        return m;
    }
    let tid = a.get_value_parser().type_id();
    let is = |t: TypeId| tid == t;
    let unsigned = [
        TypeId::of::<u8>(),
        TypeId::of::<u16>(),
        TypeId::of::<u32>(),
        TypeId::of::<u64>(),
        TypeId::of::<usize>(),
    ];
    let signed = [
        TypeId::of::<i8>(),
        TypeId::of::<i16>(),
        TypeId::of::<i32>(),
        TypeId::of::<i64>(),
        TypeId::of::<isize>(),
    ];
    if unsigned.iter().any(|t| is(*t)) {
        m.insert("type".into(), json!("integer"));
        m.insert("minimum".into(), json!(0));
        if is(TypeId::of::<u8>()) {
            m.insert("maximum".into(), json!(255));
        }
    } else if signed.iter().any(|t| is(*t)) {
        m.insert("type".into(), json!("integer"));
    } else if is(TypeId::of::<f32>()) || is(TypeId::of::<f64>()) {
        m.insert("type".into(), json!("number"));
    } else if is(TypeId::of::<bool>()) {
        m.insert("type".into(), json!("boolean"));
    } else {
        m.insert("type".into(), json!("string"));
        if is(TypeId::of::<std::path::PathBuf>()) {
            m.insert("format".into(), json!("path"));
        }
    }
    m
}

fn typed_default(raw: &str, item: &Map<String, Value>) -> Value {
    match item.get("type").and_then(|t| t.as_str()) {
        Some("integer") => raw.parse::<i64>().map(Value::from).unwrap_or_else(|_| json!(raw)),
        Some("number") => raw
            .parse::<f64>()
            .ok()
            .and_then(|f| serde_json::Number::from_f64(f).map(Value::Number))
            .unwrap_or_else(|| json!(raw)),
        Some("boolean") => raw.parse::<bool>().map(Value::from).unwrap_or_else(|_| json!(raw)),
        _ => json!(raw),
    }
}

fn action_name(a: &ArgAction) -> &'static str {
    match a {
        ArgAction::Set => "set",
        ArgAction::Append => "append",
        ArgAction::SetTrue => "flag",
        ArgAction::SetFalse => "flag-false",
        ArgAction::Count => "count",
        _ => "other",
    }
}

fn text(s: Option<&clap::builder::StyledStr>) -> Option<String> {
    s.map(|s| s.to_string().trim().to_string()).filter(|s| !s.is_empty())
}

/// Lint a tool's contract against its clap tree. Returns human-readable
/// problems; tools assert this is empty in a unit test so help text and
/// annotations can't silently rot.
///
/// Checks: every visible leaf has a [`CommandSpec`](crate::CommandSpec) and
/// vice versa, every `io` entry names a real arg, every leaf has an `about`,
/// every visible arg has help text.
pub fn check_contract(contract: &Contract, cmd: &Command) -> Vec<String> {
    let schema = schema_for(contract, cmd);
    let mut problems = Vec::new();
    let mut seen = Vec::new();
    for c in schema["commands"].as_array().into_iter().flatten() {
        let path = c["argv_prefix"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        seen.push(path.clone());
        let label = if path.is_empty() { contract.tool.to_string() } else { path.clone() };
        if c["x-effects"] == "undeclared" {
            problems.push(format!("`{}`: no CommandSpec declared", label));
        }
        if c["about"].is_null() {
            problems.push(format!("`{}`: missing about/doc comment", label));
        }
        let props = c["input_schema"]["properties"].as_object().cloned().unwrap_or_default();
        for (id, p) in &props {
            if p.get("description").is_none() {
                problems.push(format!("`{}`: arg `{}` has no help text", label, id));
            }
        }
        if let Some(spec) = contract.spec(&path) {
            for (id, _) in spec.io {
                if !props.contains_key(*id) {
                    problems.push(format!("`{}`: io names unknown arg `{}`", label, id));
                }
            }
        }
    }
    for spec in contract.commands {
        if !spec.feature_gated && !seen.iter().any(|s| s == spec.path) {
            problems.push(format!("CommandSpec `{}` matches no visible leaf command", spec.path));
        }
    }
    problems
}
