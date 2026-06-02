use std::{error::Error, fmt};

#[derive(Debug)]
pub enum DomainError {
    InvalidState(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState(message) => write!(f, "invalid domain state: {message}"),
        }
    }
}

impl Error for DomainError {}
