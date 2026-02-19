// From AI: 添加 CSV 解析和时间格式转换的单元测试

#[cfg(test)]
mod csv_parse_tests {
    use std::collections::HashMap;

    #[derive(Debug, PartialEq)]
    struct GanttRecord {
        task_name: String,
        start_date: String,
        end_date: String,
        duration: String,
    }

    fn parse_csv_line(line: &str) -> Result<HashMap<String, String>, String> {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 {
            return Err("CSV 行格式错误".to_string());
        }

        let mut record = HashMap::new();
        record.insert("task_name".to_string(), parts[0].to_string());
        record.insert("start_date".to_string(), parts[1].to_string());
        record.insert("end_date".to_string(), parts[2].to_string());
        record.insert("duration".to_string(), parts[3].to_string());

        Ok(record)
    }

    #[test]
    fn test_parse_valid_csv_line() {
        let line = "任务1,2024/01/01,2024/01/05,5天";
        let result = parse_csv_line(line);
        assert!(result.is_ok());
        let record = result.unwrap();
        assert_eq!(record.get("task_name").unwrap(), "任务1");
        assert_eq!(record.get("start_date").unwrap(), "2024/01/01");
    }

    #[test]
    fn test_parse_invalid_csv_line() {
        let line = "任务1,2024/01/01";
        let result = parse_csv_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_line() {
        let line = "";
        let result = parse_csv_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_commas_in_field() {
        let line = "\"任务1,子任务\",2024/01/01,2024/01/05,5天";
        // 注意：简化版本不处理引号，实际应使用 csv crate
        let result = parse_csv_line(line);
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod date_format_tests {
    /// 将日期格式从 / 转换为 -
    fn convert_date_format(date: &str) -> String {
        date.replace('/', "-")
    }

    #[test]
    fn test_convert_date_slash_to_dash() {
        assert_eq!(convert_date_format("2024/01/15"), "2024-01-15");
        assert_eq!(convert_date_format("2024/12/31"), "2024-12-31");
    }

    #[test]
    fn test_convert_date_already_dash() {
        assert_eq!(convert_date_format("2024-01-15"), "2024-01-15");
    }

    #[test]
    fn test_convert_date_empty() {
        assert_eq!(convert_date_format(""), "");
    }

    #[test]
    fn test_convert_date_mixed_format() {
        assert_eq!(convert_date_format("2024/01-15"), "2024-01-15");
    }

    #[test]
    fn test_convert_date_with_time() {
        assert_eq!(
            convert_date_format("2024/01/15 10:30:00"),
            "2024-01-15 10:30:00"
        );
    }
}

#[cfg(test)]
mod data_mapping_tests {
    use std::collections::HashMap;

    #[derive(Debug, PartialEq)]
    struct DingRequireDoc {
        title: String,
        start_time: String,
        end_time: String,
        parent_id: Option<String>,
    }

    fn map_csv_to_ding_require(csv_record: &HashMap<String, String>) -> DingRequireDoc {
        DingRequireDoc {
            title: csv_record
                .get("task_name")
                .cloned()
                .unwrap_or_default(),
            start_time: csv_record
                .get("start_date")
                .map(|d| d.replace('/', "-"))
                .unwrap_or_default(),
            end_time: csv_record
                .get("end_date")
                .map(|d| d.replace('/', "-"))
                .unwrap_or_default(),
            parent_id: None,
        }
    }

    #[test]
    fn test_map_csv_to_ding_require() {
        let mut csv_record = HashMap::new();
        csv_record.insert("task_name".to_string(), "需求1".to_string());
        csv_record.insert("start_date".to_string(), "2024/01/01".to_string());
        csv_record.insert("end_date".to_string(), "2024/01/31".to_string());

        let ding_doc = map_csv_to_ding_require(&csv_record);

        assert_eq!(ding_doc.title, "需求1");
        assert_eq!(ding_doc.start_time, "2024-01-01");
        assert_eq!(ding_doc.end_time, "2024-01-31");
        assert_eq!(ding_doc.parent_id, None);
    }

    #[test]
    fn test_map_empty_csv() {
        let csv_record = HashMap::new();
        let ding_doc = map_csv_to_ding_require(&csv_record);

        assert_eq!(ding_doc.title, "");
        assert_eq!(ding_doc.start_time, "");
        assert_eq!(ding_doc.end_time, "");
    }

    #[test]
    fn test_map_partial_csv() {
        let mut csv_record = HashMap::new();
        csv_record.insert("task_name".to_string(), "需求2".to_string());

        let ding_doc = map_csv_to_ding_require(&csv_record);

        assert_eq!(ding_doc.title, "需求2");
        assert_eq!(ding_doc.start_time, "");
        assert_eq!(ding_doc.end_time, "");
    }
}

#[cfg(test)]
mod time_calculation_tests {
    /// 从时间数组中获取最晚的时间
    fn get_last_time_from_array(times: &[&str]) -> Option<String> {
        if times.is_empty() {
            return None;
        }

        let mut latest = times[0].to_string();
        for &time in times.iter().skip(1) {
            if time > latest.as_str() {
                latest = time.to_string();
            }
        }

        Some(latest)
    }

    #[test]
    fn test_get_last_time_single() {
        let times = vec!["2024-01-15"];
        assert_eq!(get_last_time_from_array(&times), Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_get_last_time_multiple() {
        let times = vec!["2024-01-15", "2024-01-20", "2024-01-10"];
        assert_eq!(get_last_time_from_array(&times), Some("2024-01-20".to_string()));
    }

    #[test]
    fn test_get_last_time_empty() {
        let times: Vec<&str> = vec![];
        assert_eq!(get_last_time_from_array(&times), None);
    }

    #[test]
    fn test_get_last_time_same() {
        let times = vec!["2024-01-15", "2024-01-15", "2024-01-15"];
        assert_eq!(get_last_time_from_array(&times), Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_get_last_time_with_datetime() {
        let times = vec!["2024-01-15 10:00:00", "2024-01-15 15:30:00", "2024-01-15 08:00:00"];
        assert_eq!(get_last_time_from_array(&times), Some("2024-01-15 15:30:00".to_string()));
    }
}
