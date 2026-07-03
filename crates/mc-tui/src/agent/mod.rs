pub mod claude;

use std::fmt;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AgentMode {
    Normal,
    Learning,
}

impl AgentMode {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "learning" | "learn" => Some(Self::Learning),
            _ => None,
        }
    }
}

impl fmt::Display for AgentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Learning => write!(f, "learning"),
        }
    }
}
