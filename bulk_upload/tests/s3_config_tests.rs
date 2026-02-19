// From AI: 添加 S3 配置解析的单元测试

#[cfg(test)]
mod s3_config_tests {
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[derive(Debug)]
    struct S3Config {
        bucket: String,
        access_key: String,
        secret_key: String,
        endpoint: String,
        region: String,
    }

    #[derive(Debug)]
    enum ConfigError {
        MissingField(String),
    }

    fn parse_s3_config(content: &str) -> Result<S3Config, ConfigError> {
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

        let get = |key: &str| -> Result<String, ConfigError> {
            map.get(key)
                .filter(|v| !v.is_empty())
                .cloned()
                .ok_or_else(|| ConfigError::MissingField(key.to_string()))
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

    #[test]
    fn test_parse_valid_config() {
        let content = r#"
S3_BUCKET=my-bucket
S3_ACCESS_KEY=access123
S3_SECRET_KEY=secret456
S3_ENDPOINT=https://s3.example.com
S3_REGION=us-west-2
"#;
        let config = parse_s3_config(content).expect("配置解析失败");
        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.access_key, "access123");
        assert_eq!(config.secret_key, "secret456");
        assert_eq!(config.endpoint, "https://s3.example.com");
        assert_eq!(config.region, "us-west-2");
    }

    #[test]
    fn test_parse_missing_region_uses_default() {
        let content = r#"
S3_BUCKET=my-bucket
S3_ACCESS_KEY=access123
S3_SECRET_KEY=secret456
S3_ENDPOINT=https://s3.example.com
"#;
        let config = parse_s3_config(content).expect("配置解析失败");
        assert_eq!(config.region, "us-east-1");
    }

    #[test]
    fn test_parse_missing_required_field() {
        let content = r#"
S3_BUCKET=my-bucket
S3_ACCESS_KEY=access123
S3_ENDPOINT=https://s3.example.com
"#;
        let result = parse_s3_config(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_comments() {
        let content = r#"
# S3 配置文件
S3_BUCKET=my-bucket
# 访问密钥
S3_ACCESS_KEY=access123
S3_SECRET_KEY=secret456
S3_ENDPOINT=https://s3.example.com
"#;
        let config = parse_s3_config(content).expect("配置解析失败");
        assert_eq!(config.bucket, "my-bucket");
    }

    #[test]
    fn test_parse_with_whitespace() {
        let content = r#"
  S3_BUCKET  =  my-bucket
S3_ACCESS_KEY=access123
S3_SECRET_KEY=secret456
S3_ENDPOINT=https://s3.example.com
"#;
        let config = parse_s3_config(content).expect("配置解析失败");
        assert_eq!(config.bucket, "my-bucket");
    }

    #[test]
    fn test_parse_empty_value() {
        let content = r#"
S3_BUCKET=
S3_ACCESS_KEY=access123
S3_SECRET_KEY=secret456
S3_ENDPOINT=https://s3.example.com
"#;
        let result = parse_s3_config(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_file() {
        let content = "";
        let result = parse_s3_config(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_only_comments() {
        let content = r#"
# 这是注释
# 另一行注释
"#;
        let result = parse_s3_config(content);
        assert!(result.is_err());
    }
}
