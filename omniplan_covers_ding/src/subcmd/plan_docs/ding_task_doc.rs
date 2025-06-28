#![allow(dead_code)]
#![allow(unused)]
// 标题	描述	状态	负责人	优先级	迭代	参与者	抄送	标签	计划开始时间	计划完成时间
use std::{collections::HashMap, fmt::Display};
use strum_macros::EnumIter;

use super::DocRecord;

#[derive(Debug, EnumIter)]
pub enum DingTaskDocRow {
    Title,
    Description,
    Status,
    Assignee,
    Priority,
    Iteration,
    Participants,
    Cc,
    Tags,
    PlannedStartTime,
    PlannedEndTime,
}

impl Into<u8> for DingTaskDocRow {
    fn into(self) -> u8 {
        match self {
            DingTaskDocRow::Title => 0,
            DingTaskDocRow::Description => 1,
            DingTaskDocRow::Status => 2,
            DingTaskDocRow::Assignee => 3,
            DingTaskDocRow::Priority => 4,
            DingTaskDocRow::Iteration => 5,
            DingTaskDocRow::Participants => 6,
            DingTaskDocRow::Cc => 7,
            DingTaskDocRow::Tags => 8,
            DingTaskDocRow::PlannedStartTime => 9,
            DingTaskDocRow::PlannedEndTime => 10,
        }
    }
}

impl Display for DingTaskDocRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DingTaskDocRow::Title => write!(f, "标题"),
            DingTaskDocRow::Description => write!(f, "描述"),
            DingTaskDocRow::Status => write!(f, "状态"),
            DingTaskDocRow::Assignee => write!(f, "负责人"),
            DingTaskDocRow::Priority => write!(f, "优先级"),
            DingTaskDocRow::Iteration => write!(f, "迭代"),
            DingTaskDocRow::Participants => write!(f, "参与者"),
            DingTaskDocRow::Cc => write!(f, "抄送"),
            DingTaskDocRow::Tags => write!(f, "标签"),
            DingTaskDocRow::PlannedStartTime => write!(f, "计划开始时间"),
            DingTaskDocRow::PlannedEndTime => write!(f, "计划完成时间"),
        }
    }
}

/// 任务数据结构
#[derive(Debug)]
pub struct TaskOnceRecord {
    task_str: String,
    planned_start_date: String,
    planned_end_data: String,
    hours_cost: u32,
    dev_avatar: String,
    iter_parent: String,
    liter_belong: String,
}

impl DocRecord for TaskOnceRecord {
    fn deadline(&self) -> Option<String> {
        return None;
    }

    fn requirement_convert_to_hash(&self) -> std::collections::HashMap<u32, String> {
        let mut val_map: HashMap<u32, String> = HashMap::new();

        return val_map;
    }

    fn is_empty(&self) -> bool {
        return self.dev_avatar.len() == 0;
    }

    fn new(rec: &csv::StringRecord, parent: &str, liter_belong: &str) -> Self {
        let require_str = rec.get(1).expect("require_str get empty!").to_owned();
        let planned_start_date = rec
            .get(2)
            .expect("planned_start_date get empty!")
            .to_owned();
        let planned_end_data = rec.get(3).expect("planned_end_date get empty!").to_owned();
        let dev_avatar = rec.get(10).expect("dev_avatar get empty!").to_owned();
        let hours_cost = rec
            .get(4)
            .and_then(|f| f.parse::<u32>().ok())
            .expect("hours cost get failed!");
        Self {
            task_str: todo!(),
            planned_start_date: todo!(),
            planned_end_data: todo!(),
            hours_cost: todo!(),
            dev_avatar: todo!(),
            iter_parent: todo!(),
            liter_belong: todo!(),
        }
    }
}
