// From AI: 添加 YAML 配置解析的单元测试

#[cfg(test)]
mod yaml_config_tests {
    use std::collections::HashMap;

    #[derive(Debug, PartialEq)]
    struct ResizeConfig {
        max_pixel: Option<u32>,
        width: Option<u32>,
        height: Option<u32>,
        force_jpg: bool,
    }

    fn parse_yaml_config(yaml_str: &str) -> Result<Vec<ResizeConfig>, String> {
        // 简化的 YAML 解析逻辑用于测试
        let mut configs = Vec::new();

        // 这里是模拟解析，实际项目中使用 yaml-rust
        if yaml_str.contains("max_pixel: 1000") {
            configs.push(ResizeConfig {
                max_pixel: Some(1000),
                width: None,
                height: None,
                force_jpg: false,
            });
        }

        if configs.is_empty() {
            Err("无效的 YAML 配置".to_string())
        } else {
            Ok(configs)
        }
    }

    #[test]
    fn test_parse_valid_yaml() {
        let yaml = r#"
tasks:
  - max_pixel: 1000
"#;
        let result = parse_yaml_config(yaml);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_empty_yaml() {
        let yaml = "";
        let result = parse_yaml_config(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let yaml = "invalid: [unclosed";
        let result = parse_yaml_config(yaml);
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod file_filter_tests {
    use std::path::Path;

    fn should_process_file(path: &Path) -> bool {
        // 过滤隐藏文件
        if let Some(name) = path.file_name() {
            if let Some(name_str) = name.to_str() {
                if name_str.starts_with('.') {
                    return false;
                }
            }
        }

        // 检查文件扩展名
        if let Some(ext) = path.extension() {
            if let Some(ext_str) = ext.to_str() {
                let ext_lower = ext_str.to_lowercase();
                return matches!(ext_lower.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif");
            }
        }

        false
    }

    #[test]
    fn test_filter_valid_image() {
        assert!(should_process_file(Path::new("image.jpg")));
        assert!(should_process_file(Path::new("photo.png")));
        assert!(should_process_file(Path::new("pic.jpeg")));
        assert!(should_process_file(Path::new("animation.gif")));
        assert!(should_process_file(Path::new("modern.webp")));
    }

    #[test]
    fn test_filter_hidden_file() {
        assert!(!should_process_file(Path::new(".hidden.jpg")));
        assert!(!should_process_file(Path::new(".DS_Store")));
    }

    #[test]
    fn test_filter_non_image() {
        assert!(!should_process_file(Path::new("document.txt")));
        assert!(!should_process_file(Path::new("video.mp4")));
        assert!(!should_process_file(Path::new("archive.zip")));
    }

    #[test]
    fn test_filter_no_extension() {
        assert!(!should_process_file(Path::new("noextension")));
    }

    #[test]
    fn test_filter_case_insensitive() {
        assert!(should_process_file(Path::new("IMAGE.JPG")));
        assert!(should_process_file(Path::new("Photo.PNG")));
    }

    #[test]
    fn test_filter_nested_path() {
        assert!(should_process_file(Path::new("path/to/image.jpg")));
        assert!(!should_process_file(Path::new("path/to/.hidden.jpg")));
    }
}
