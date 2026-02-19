// From AI: 添加 URL 提取逻辑的单元测试

#[cfg(test)]
mod url_extraction_tests {
    use serde_json::json;

    /// 递归遍历任意 JSON 结构，提取所有以 http:// 或 https:// 开头的字符串值
    fn extract_urls(value: &serde_json::Value, urls: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    urls.push(trimmed.to_string());
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    extract_urls(item, urls);
                }
            }
            serde_json::Value::Object(obj) => {
                for (_key, val) in obj {
                    extract_urls(val, urls);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn test_extract_simple_url() {
        let json = json!({
            "url": "https://example.com/image.jpg"
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/image.jpg");
    }

    #[test]
    fn test_extract_nested_urls() {
        let json = json!({
            "data": {
                "images": [
                    {"url": "https://example.com/1.jpg"},
                    {"url": "https://example.com/2.jpg"}
                ]
            }
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/1.jpg".to_string()));
        assert!(urls.contains(&"https://example.com/2.jpg".to_string()));
    }

    #[test]
    fn test_extract_mixed_content() {
        let json = json!({
            "title": "Test",
            "count": 42,
            "active": true,
            "image": "https://example.com/image.jpg",
            "description": "Some text without URL"
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/image.jpg");
    }

    #[test]
    fn test_extract_empty_json() {
        let json = json!({});
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 0);
    }

    #[test]
    fn test_extract_no_urls() {
        let json = json!({
            "title": "Test",
            "count": 42,
            "items": ["a", "b", "c"]
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 0);
    }

    #[test]
    fn test_extract_http_and_https() {
        let json = json!({
            "secure": "https://example.com/secure.jpg",
            "insecure": "http://example.com/insecure.jpg"
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_extract_with_whitespace() {
        let json = json!({
            "url": "  https://example.com/image.jpg  "
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/image.jpg");
    }

    #[test]
    fn test_extract_deeply_nested() {
        let json = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": {
                            "url": "https://example.com/deep.jpg"
                        }
                    }
                }
            }
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/deep.jpg");
    }

    #[test]
    fn test_extract_array_of_urls() {
        let json = json!([
            "https://example.com/1.jpg",
            "https://example.com/2.jpg",
            "https://example.com/3.jpg"
        ]);
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 3);
    }

    #[test]
    fn test_extract_invalid_url_format() {
        let json = json!({
            "url1": "ftp://example.com/file.txt",
            "url2": "example.com/image.jpg",
            "url3": "https://example.com/valid.jpg"
        });
        let mut urls = Vec::new();
        extract_urls(&json, &mut urls);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/valid.jpg");
    }
}
