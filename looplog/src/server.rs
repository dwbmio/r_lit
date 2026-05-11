use crate::db::{NewRun, SqliteStorage};
use crate::error::{Error, Result};
use crate::protocol::{
    AppendLine, ErrorResponse, FinishRunRequest, HealthResponse, StartRunRequest, StartRunResponse,
};
use serde::Serialize;
use std::path::PathBuf;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub fn serve(addr: &str, db_path: PathBuf, keep_hours: u64) -> Result<()> {
    let server = Server::http(addr)?;
    eprintln!("looplog serve listening on http://{}", addr);
    for request in server.incoming_requests() {
        let result = handle_request(request, &db_path, keep_hours);
        if let Err(e) = result {
            eprintln!("[looplog serve] request failed: {}", e);
        }
    }
    Ok(())
}

fn handle_request(mut request: Request, db_path: &PathBuf, keep_hours: u64) -> Result<()> {
    let method = request.method().clone();
    let path = request.url().split('?').next().unwrap_or("/").to_string();
    let mut storage = SqliteStorage::open(db_path)?;
    storage.clean(keep_hours, false)?;

    match (method.clone(), path.as_str()) {
        (Method::Get, "/healthz") => respond_json(
            request,
            StatusCode(200),
            &HealthResponse {
                status: "ok",
                service: "looplog",
            },
        ),
        (Method::Post, "/v1/runs") => {
            let req: StartRunRequest = read_json(&mut request)?;
            let run_id = storage.start_run(NewRun {
                tag: req.tag,
                source: req.source,
                cwd: req.cwd,
                argv: req.argv,
                client_id: req.client_id,
                kind: req.kind,
                meta: req.meta,
            })?;
            respond_json(request, StatusCode(200), &StartRunResponse { run_id })
        }
        _ => {
            if method == Method::Post && path.starts_with("/v1/runs/") && path.ends_with("/lines") {
                let run_id = path
                    .trim_start_matches("/v1/runs/")
                    .trim_end_matches("/lines")
                    .trim_end_matches('/');
                let body = read_body(&mut request)?;
                let lines = parse_ndjson_lines(&body)?;
                let count = storage.append_lines(run_id, &lines)?;
                respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok", "lines": count}),
                )
            } else if method == Method::Patch && path.starts_with("/v1/runs/") {
                let run_id = path.trim_start_matches("/v1/runs/").trim_end_matches('/');
                let req: FinishRunRequest = read_json(&mut request)?;
                storage.finish_run(run_id, &req)?;
                respond_json(
                    request,
                    StatusCode(200),
                    &serde_json::json!({"status": "ok"}),
                )
            } else {
                respond_json(
                    request,
                    StatusCode(404),
                    &ErrorResponse {
                        status: "error",
                        error: "not found".to_string(),
                    },
                )
            }
        }
    }
}

fn parse_ndjson_lines(body: &str) -> Result<Vec<AppendLine>> {
    let mut lines = Vec::new();
    for raw in body.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        lines.push(serde_json::from_str(raw)?);
    }
    Ok(lines)
}

fn read_json<T: serde::de::DeserializeOwned>(request: &mut Request) -> Result<T> {
    let body = read_body(request)?;
    Ok(serde_json::from_str(&body)?)
}

fn read_body(request: &mut Request) -> Result<String> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

fn respond_json<T: Serialize>(request: Request, status: StatusCode, value: &T) -> Result<()> {
    let body = serde_json::to_string_pretty(value)?;
    let mut response = Response::from_string(body).with_status_code(status);
    if let Ok(header) = Header::from_bytes("Content-Type", "application/json; charset=utf-8") {
        response.add_header(header);
    }
    request
        .respond(response)
        .map_err(|e| Error::Http(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ndjson_batch() {
        let lines = parse_ndjson_lines(
            r#"{"level":"error","stream":"console","text":"TypeError"}
{"level":"info","text":"ok"}"#,
        )
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "TypeError");
    }
}
