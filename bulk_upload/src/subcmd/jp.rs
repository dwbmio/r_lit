use std::collections::HashMap;
use std::path::Path;

use aws_config;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, primitives::ByteStream, Client as S3Client};
use futures::future::join_all;
use serde::Deserialize;

use crate::error::AppError;

/// .s3 配置文件解析结果
struct S3Config {
    bucket: String,
    access_key: String,
    secret_key: String,
    endpoint: String,
    region: String,
}

/// JSON 文件中的书籍条目
#[derive(Deserialize)]
struct BookEntry {
    #[serde(default)]
    image: String,
}

/// JSON 文件根结构
#[derive(Deserialize)]
struct BookList {
    books: Vec<BookEntry>,
}

/// 从 .s3 dotenv 文件中加载配置
fn load_s3_config(path: &Path) -> Result<S3Config, AppError> {
    let content = std::fs::read_to_string(path)?;
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let get = |key: &str| -> Result<String, AppError> {
        map.get(key)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| AppError::S3ConfigError(format!(".s3 文件缺少必需字段: {}", key)))
    };

    let region = map
        .get("S3_REGION")
        .cloned()
        .unwrap_or_else(|| "us-east-1".to_string());

    Ok(S3Config {
        bucket: get("S3_BUCKET")?,
        access_key: get("S3_ACCESS_KEY")?,
        secret_key: get("S3_SECRET_KEY")?,
        endpoint: get("S3_ENDPOINT")?,
        region,
    })
}

/// 从 JSON 文件解析 URL 列表，分批并发下载后上传到 S3
pub async fn exec(
    json_path: &Path,
    s3_config_path: &Path,
    prefix: &str,
    concurrency: usize,
) -> Result<(), AppError> {
    // 1. 加载 .s3 配置
    let cfg = load_s3_config(s3_config_path)?;
    log::info!(
        "S3 配置: bucket={}, endpoint={}, region={}",
        cfg.bucket,
        cfg.endpoint,
        cfg.region
    );

    // 2. 读取并解析 JSON 文件（支持 { "books": [{ "image": "url" }, ...] } 结构）
    let content = tokio::fs::read_to_string(json_path).await?;
    let book_list: BookList = serde_json::from_str(&content)?;
    let urls: Vec<String> = book_list
        .books
        .into_iter()
        .map(|b| b.image)
        .filter(|url| !url.is_empty())
        .collect();

    log::info!("解析到 {} 个 URL，并发数: {}", urls.len(), concurrency);

    if urls.is_empty() {
        log::warn!("JSON 文件中无 URL，跳过");
        return Ok(());
    }

    // 3. 构建 S3 客户端
    let s3_client =
        build_s3_client(&cfg.endpoint, &cfg.region, &cfg.access_key, &cfg.secret_key).await;

    // 4. 分批处理
    let http_client = reqwest::Client::new();
    let total_batches = (urls.len() + concurrency - 1) / concurrency;

    for (batch_idx, chunk) in urls.chunks(concurrency).enumerate() {
        log::info!(
            "处理批次 {}/{} ({} 个文件)",
            batch_idx + 1,
            total_batches,
            chunk.len()
        );

        // 4a. 并发下载本批次所有文件
        let download_futures: Vec<_> = chunk
            .iter()
            .map(|url| download_file(&http_client, url))
            .collect();

        let download_results = join_all(download_futures).await;

        // 4b. 收集成功下载的文件，并发上传到 S3
        let mut upload_futures = Vec::new();
        for (i, result) in download_results.into_iter().enumerate() {
            match result {
                Ok(bytes) => {
                    let url = &chunk[i];
                    let s3_key = build_s3_key(prefix, url);
                    let bucket_owned = cfg.bucket.clone();
                    let url_owned = url.clone();
                    log::debug!("下载成功: {} -> s3://{}/{}", url, cfg.bucket, s3_key);
                    upload_futures.push(upload_to_s3(
                        &s3_client,
                        bucket_owned,
                        s3_key,
                        bytes,
                        url_owned,
                    ));
                }
                Err(e) => {
                    log::error!("下载失败: {}", e);
                }
            }
        }

        let upload_results = join_all(upload_futures).await;
        let success_count = upload_results.iter().filter(|r| r.is_ok()).count();
        let fail_count = upload_results.len() - success_count;

        for result in &upload_results {
            if let Err(e) = result {
                log::error!("上传失败: {}", e);
            }
        }

        log::info!(
            "批次 {}/{} 完成: {} 成功, {} 失败",
            batch_idx + 1,
            total_batches,
            success_count,
            fail_count
        );
    }

    log::info!("全部处理完成");
    Ok(())
}

/// 构建 S3 客户端（兼容 MinIO 等 S3 协议存储）
async fn build_s3_client(
    endpoint: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> S3Client {
    let creds = Credentials::new(access_key, secret_key, None, None, "bulk_upload");

    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .credentials_provider(creds)
        .region(Region::new(region.to_string()))
        .endpoint_url(endpoint)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();

    S3Client::from_conf(s3_config)
}

/// 下载单个文件，返回字节内容
async fn download_file(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, AppError> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::DownloadFailed(url.to_string(), e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::DownloadFailed(
            url.to_string(),
            format!("HTTP {}", resp.status()),
        ));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::DownloadFailed(url.to_string(), e.to_string()))?;

    Ok(bytes.to_vec())
}

/// 上传字节内容到 S3
async fn upload_to_s3(
    client: &S3Client,
    bucket: String,
    key: String,
    data: Vec<u8>,
    source_url: String,
) -> Result<(), AppError> {
    let body = ByteStream::from(data);
    client
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .body(body)
        .send()
        .await
        .map_err(|e| AppError::S3PutError(format!("上传 {} 失败: {}", source_url, e)))?;

    log::info!("上传成功: s3://{}/{}", bucket, key);
    Ok(())
}

/// 从 URL 提取文件名，拼接 S3 key
fn build_s3_key(prefix: &str, url: &str) -> String {
    let filename = url
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown");

    if prefix.is_empty() {
        filename.to_string()
    } else {
        let trimmed = prefix.trim_end_matches('/');
        format!("{}/{}", trimmed, filename)
    }
}
