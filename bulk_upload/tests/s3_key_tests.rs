// From AI: 添加 S3 key 构建逻辑的单元测试

#[cfg(test)]
mod s3_key_tests {
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

    #[test]
    fn test_build_key_simple_url() {
        let key = build_s3_key("assets", "https://example.com/image.jpg");
        assert_eq!(key, "assets/image.jpg");
    }

    #[test]
    fn test_build_key_with_query_params() {
        let key = build_s3_key("assets", "https://example.com/image.jpg?size=large&v=2");
        assert_eq!(key, "assets/image.jpg");
    }

    #[test]
    fn test_build_key_empty_prefix() {
        let key = build_s3_key("", "https://example.com/image.jpg");
        assert_eq!(key, "image.jpg");
    }

    #[test]
    fn test_build_key_prefix_with_trailing_slash() {
        let key = build_s3_key("assets/images/", "https://example.com/photo.png");
        assert_eq!(key, "assets/images/photo.png");
    }

    #[test]
    fn test_build_key_nested_path() {
        let key = build_s3_key("assets", "https://example.com/path/to/image.jpg");
        assert_eq!(key, "assets/image.jpg");
    }

    #[test]
    fn test_build_key_url_ending_with_slash() {
        let key = build_s3_key("assets", "https://example.com/path/");
        assert_eq!(key, "assets/unknown");
    }

    #[test]
    fn test_build_key_complex_filename() {
        let key = build_s3_key("assets", "https://example.com/my-image_2024.jpg");
        assert_eq!(key, "assets/my-image_2024.jpg");
    }

    #[test]
    fn test_build_key_chinese_filename() {
        let key = build_s3_key("assets", "https://example.com/图片.jpg");
        assert_eq!(key, "assets/图片.jpg");
    }

    #[test]
    fn test_build_key_multiple_query_params() {
        let key = build_s3_key(
            "assets",
            "https://example.com/image.jpg?width=800&height=600&format=webp",
        );
        assert_eq!(key, "assets/image.jpg");
    }

    #[test]
    fn test_build_key_no_extension() {
        let key = build_s3_key("assets", "https://example.com/image");
        assert_eq!(key, "assets/image");
    }

    #[test]
    fn test_build_key_multiple_dots() {
        let key = build_s3_key("assets", "https://example.com/my.image.file.jpg");
        assert_eq!(key, "assets/my.image.file.jpg");
    }

    #[test]
    fn test_build_key_nested_prefix() {
        let key = build_s3_key("assets/images/photos", "https://example.com/photo.jpg");
        assert_eq!(key, "assets/images/photos/photo.jpg");
    }
}
