use csv::StringRecord;

pub mod ding_task_doc;
pub mod ding_require_doc;

pub trait DocRecord {
    fn new(rec: &StringRecord, parent: &str, liter: &str) -> Self;
    fn requirement_convert_to_hash(&self) -> std::collections::HashMap<u32, String>;
    fn is_empty(&self) -> bool;
}
