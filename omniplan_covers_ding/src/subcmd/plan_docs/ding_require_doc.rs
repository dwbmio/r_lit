// 标题	描述	状态	负责人	优先级	研发人员	测试人员	迭代	标签	计划开始时间	计划完成时间	预计工时	研发量（价值点）	价值点（测试）	价值点（产品）	价值点（研发） 父项ID 父项是否存在
use csv::StringRecord;
use std::{collections::HashMap, fmt::Display};
use strum_macros::EnumIter;

use super::DocRecord;

#[derive(Debug, EnumIter)]
pub enum DingRequireDocRow {
    Title,
    Description,
    Status,
    Owner,
    Priority,
    DevPersonnel,
    TestPersonnel,
    Iteration,
    Tags,
    PlannedStartTime,
    PlannedEndTime,
    EstimatedWorkHours,
    DevValuePoints,
    TestValuePoints,
    ProductValuePoints,
    ValuePoints,
    ParentTaskId,
    IsParentTask,
}

impl Into<u8> for DingRequireDocRow {
    fn into(self) -> u8 {
        match self {
            DingRequireDocRow::Title => 0,
            DingRequireDocRow::Description => 1,
            DingRequireDocRow::Status => 2,
            DingRequireDocRow::Owner => 3,
            DingRequireDocRow::Priority => 4,
            DingRequireDocRow::DevPersonnel => 5,
            DingRequireDocRow::TestPersonnel => 6,
            DingRequireDocRow::Iteration => 7,
            DingRequireDocRow::Tags => 8,
            DingRequireDocRow::PlannedStartTime => 9,
            DingRequireDocRow::PlannedEndTime => 10,
            DingRequireDocRow::EstimatedWorkHours => 11,
            DingRequireDocRow::DevValuePoints => 12,
            DingRequireDocRow::TestValuePoints => 13,
            DingRequireDocRow::ProductValuePoints => 14,
            DingRequireDocRow::ValuePoints => 15,
            DingRequireDocRow::ParentTaskId => 16,
            DingRequireDocRow::IsParentTask => 17,
        }
    }
}

impl Display for DingRequireDocRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DingRequireDocRow::Title => write!(f, "标题"),
            DingRequireDocRow::Description => write!(f, "描述"),
            DingRequireDocRow::Status => write!(f, "状态"),
            DingRequireDocRow::Owner => write!(f, "负责人"),
            DingRequireDocRow::Priority => write!(f, "优先级"),
            DingRequireDocRow::DevPersonnel => write!(f, "研发人员"),
            DingRequireDocRow::TestPersonnel => write!(f, "测试人员"),
            DingRequireDocRow::Iteration => write!(f, "迭代"),
            DingRequireDocRow::Tags => write!(f, "标签"),
            DingRequireDocRow::PlannedStartTime => write!(f, "计划开始时间"),
            DingRequireDocRow::PlannedEndTime => write!(f, "计划完成时间"),
            DingRequireDocRow::EstimatedWorkHours => write!(f, "预计工时"),
            DingRequireDocRow::DevValuePoints => write!(f, "研发量（价值点）"),
            DingRequireDocRow::TestValuePoints => write!(f, "价值点（测试）"),
            DingRequireDocRow::ProductValuePoints => write!(f, "价值点（产品）"),
            DingRequireDocRow::ValuePoints => write!(f, "价值点"),
            DingRequireDocRow::ParentTaskId => write!(f, "父项ID"),
            DingRequireDocRow::IsParentTask => write!(f, "父项是否存在"),
        }
    }
}

/// 需求数据结构
#[derive(Debug)]
pub struct RequireOnceRecord {
    require_str: String,
    planned_start_date: String,
    planned_end_data: String,
    hours_cost: f32,
    dev_avatar: String,
    task_parent: String,
    liter_belong: String,
}

impl DocRecord for RequireOnceRecord {
    fn deadline(&self) -> Option<String> {
        Some(
            self.planned_end_data
                .split_whitespace()
                .next()
                .unwrap_or("")
                .replace("/", "-"),
        )
    }
    fn requirement_convert_to_hash(&self) -> HashMap<u32, String> {
        let mut val_map: HashMap<u32, String> = HashMap::new();
        // variable
        val_map.insert(DingRequireDocRow::Title as u32, self.require_str.clone());
        val_map.insert(
            DingRequireDocRow::Description as u32,
            self.require_str.clone(),
        );
        val_map.insert(
            DingRequireDocRow::PlannedStartTime as u32,
            self.planned_start_date
                .split_whitespace()
                .next()
                .unwrap_or("")
                .replace("/", "-"),
        );
        val_map.insert(
            DingRequireDocRow::Iteration as u32,
            self.liter_belong.clone(),
        );
        val_map.insert(
            DingRequireDocRow::PlannedEndTime as u32,
            self.planned_end_data
                .split_whitespace()
                .next()
                .unwrap_or("")
                .replace("/", "-"),
        );
        val_map.insert(
            DingRequireDocRow::EstimatedWorkHours as u32,
            format!("{:.2}", self.hours_cost as f32),
        );
        val_map.insert(DingRequireDocRow::Owner as u32, self.dev_avatar.clone());
        val_map.insert(
            DingRequireDocRow::ParentTaskId as u32,
            self.task_parent.clone(),
        );
        val_map.insert(
            DingRequireDocRow::DevPersonnel as u32,
            self.dev_avatar.clone(),
        );
        // const
        val_map.insert(DingRequireDocRow::Priority as u32, "紧急".to_owned());
        val_map.insert(DingRequireDocRow::Tags as u32, "星链".to_owned());
        val_map.insert(DingRequireDocRow::IsParentTask as u32, "Y".to_owned());
        val_map
    }

    fn is_empty(&self) -> bool {
        return self.dev_avatar.len() == 0;
    }

    fn new(record: &StringRecord, parent: &str, liter_belong: &str) -> Self {
        let require_str = record.get(1).expect("require_str get empty!").to_owned();
        let planned_start_date = record
            .get(2)
            .expect("planned_start_date get empty!")
            .to_owned();
        let planned_end_data = record
            .get(3)
            .expect("planned_end_date get empty!")
            .to_owned();
        let dev_avatar = record.get(10).expect("dev_avatar get empty!").to_owned();
        let hours_cost = record
            .get(4)
            .and_then(|f| f.parse::<f32>().ok())
            .expect("hours cost get failed!");
        return Self {
            require_str,
            planned_start_date,
            planned_end_data: planned_end_data,
            hours_cost,
            dev_avatar,
            task_parent: parent.to_owned(),
            liter_belong: liter_belong.to_string(),
        };
    }
}
