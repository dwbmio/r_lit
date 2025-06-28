use super::plan_docs::{
    DocRecord, ding_require_doc::DingRequireDocRow, ding_task_doc::DingTaskDocRow,
};
use crate::{
    ctx::{AppContext, DocTemplate},
    error::ConvertError,
    subcmd::plan_docs::ding_require_doc::RequireOnceRecord,
};
use cli_common::chrono;
use cli_common::clap::ArgMatches;
use rust_xlsxwriter::workbook::Workbook;
use std::fmt::Debug;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use strum::IntoEnumIterator;

const EXCEL_CONTENT_START: u32 = 1;

/// 根据时间字符串数组计算最后（最大）时间
/// # 参数
/// - `times`: 时间字符串数组，格式如 "2024-06-01 12:00:00"
/// # 返回
/// - Option<String>: 最后（最大）的时间字符串
use chrono::NaiveDateTime;

pub fn get_last_time_from_array(times: &[&str]) -> Option<String> {
    times
        .iter()
        .filter_map(|s| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| (dt, *s))
        })
        .max_by_key(|(dt, _)| *dt)
        .map(|(_, s)| s.to_string())
}

/// 读取原始gante数据从csv file
fn read_gante_data<T: DocRecord + Debug>(
    csv_file: &PathBuf,
    task_parent: &str,
    liter_belong: &str,
) -> Result<Vec<T>, ConvertError> {
    let mut rdr = csv::Reader::from_path(csv_file)?;
    let mut vec_tasks: Vec<T> = vec![];
    for result in rdr.records() {
        let record = result?;
        let task_cell = T::new(&record, task_parent, liter_belong);
        println!("{:?}", task_cell);
        vec_tasks.push(task_cell);
    }
    Ok(vec_tasks)
}

fn template_xlsx_writer<T: DocRecord>(
    doc_template: DocTemplate,
    doc_record: Vec<T>,
    file_path: &PathBuf,
) -> Result<(), ConvertError> {
    let mut work_book = Workbook::new();
    let sheet = work_book
        .add_worksheet()
        .set_name(r#"{displayName} 导入模板"#)?;

    // title
    let row_titles = match doc_template {
        DocTemplate::DingRequireDoc => {
            let mut row_titles: Vec<String> = vec![];
            for row in DingRequireDocRow::iter() {
                row_titles.push(format!("{}", row));
            }
            row_titles
        }
        DocTemplate::DingTaskDoc => {
            let mut row_titles: Vec<String> = vec![];
            for row in DingTaskDocRow::iter() {
                row_titles.push(format!("{}", row));
            }
            row_titles
        }
    };
    // title
    for (col, header) in row_titles.iter().enumerate() {
        sheet.write_string(0, col as u16, header.to_owned())?;
    }
    // content
    match doc_template {
        DocTemplate::DingRequireDoc => {
            let mut write_line_idx: u32 = 0;
            for (_, rec) in doc_record.iter().enumerate() {
                if rec.is_empty() {
                    continue;
                }
                let u_string_map = rec.requirement_convert_to_hash();
                for (_, (col, str_val)) in u_string_map.iter().enumerate() {
                    sheet.write_string(
                        EXCEL_CONTENT_START + write_line_idx,
                        *col as u16,
                        str_val,
                    )?;
                }
                write_line_idx += 1;
            }
        }
        DocTemplate::DingTaskDoc => {}
    };
    work_book.save(file_path)?;

    Ok(())
}

pub async fn handle(matches: &ArgMatches, ctx: &AppContext) -> Result<(), ConvertError> {
    // read csv-file
    let f_opt = matches.get_one::<String>("csv-file");
    // parent
    let task_parent = matches
        .get_one::<String>("parent")
        .expect("required task-parent params!");
    // liter
    let liter_parent = matches
        .get_one::<String>("liter")
        .expect("required liter params!");

    // doc-type
    let opt_type = matches
        .get_one::<String>("doc-type")
        .expect("required doc-type params!");
    let opt_type_enum = DocTemplate::from_str(&opt_type).expect("parse doc-type failed!");

    let o_f_opt = {
        // 输出的运行路径的临时文件名内
        use cli_common::chrono;
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let temp_file_name = format!("{}_for_ding-import_{}.xlsx", opt_type, timestamp);
        let temp_file_name = std::env::current_dir().unwrap().join(temp_file_name);
        Some(temp_file_name)
    };

    if let (Some(f), Some(o_f)) = (f_opt, o_f_opt) {
        let vec_tasks = read_gante_data::<RequireOnceRecord>(
            &Path::new(f).to_path_buf(),
            task_parent,
            liter_parent,
        )?;
        println!("write to excel file :{:?}...", &o_f);
        template_xlsx_writer(opt_type_enum, vec_tasks, &o_f)?;
        Ok(())
    } else {
        Err(ConvertError::LogicError("params get empty".to_string()))
    }
}
