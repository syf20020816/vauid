use thiserror::Error;


#[derive(Debug, Error, PartialEq)]
pub enum Error {

    
    #[error("{0}")]
    Other(String),

}