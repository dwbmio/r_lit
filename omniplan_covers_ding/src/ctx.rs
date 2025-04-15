
#[derive(Default)]
pub struct AppContext {}

#[derive(Debug, Clone)]
pub enum DocTemplate {
    DingRequireDoc, 
    DingTaskDoc, 
}

impl std::str::FromStr for DocTemplate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "require" => Ok(DocTemplate::DingRequireDoc),
            "task" => Ok(DocTemplate::DingTaskDoc),
            _ => Err(format!("Invalid value for DocTemplate: {}", s)),
        }
    }
}