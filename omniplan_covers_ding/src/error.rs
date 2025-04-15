use thiserror::Error;
use rust_xlsxwriter::XlsxError;

#[derive(Debug, Error)]
pub enum ConvertError{
    #[error("csv read error:{0}")]
    CsvReadError(#[from] csv::Error),

    #[error("xlsx write error:{0}")]
    XlsxError(#[from] XlsxError),
    
    #[error("{0}")]
    LogicError(String)
}