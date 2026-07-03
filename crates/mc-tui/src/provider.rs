use std::env;
use std::fmt;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

impl Provider {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "open-ai" => Some(Self::OpenAi),
            _ => None,
        }
    }

    pub fn env_var(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
        }
    }

    pub fn has_credentials(self) -> bool {
        env::var_os(self.env_var()).is_some()
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAi => write!(f, "openai"),
        }
    }
}
