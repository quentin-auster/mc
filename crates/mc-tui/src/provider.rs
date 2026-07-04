use std::env;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::Deserialize;

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
        self.credential_source().is_some()
    }

    pub fn credential_source(self) -> Option<String> {
        if env::var_os(self.env_var()).is_some() {
            return Some(format!("env {}", self.env_var()));
        }
        ProviderConfig::load()
            .ok()
            .and_then(|config| config.has_key(self).then(|| ".mc/config.json".to_string()))
    }
}

#[derive(Default, Deserialize)]
pub struct ProviderConfig {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
}

impl ProviderConfig {
    pub fn load() -> Result<Self, String> {
        let path = Path::new(".mc").join("config.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }

    pub fn default_provider(&self) -> Option<Provider> {
        self.default_provider.as_deref().and_then(Provider::parse)
    }

    pub fn has_key(&self, provider: Provider) -> bool {
        match provider {
            Provider::Anthropic => has_secret(&self.anthropic_api_key),
            Provider::OpenAi => has_secret(&self.openai_api_key),
        }
    }
}

fn has_secret(value: &Option<String>) -> bool {
    value
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

impl Provider {
    pub fn default_model_from_config() -> Option<String> {
        ProviderConfig::load()
            .ok()
            .and_then(|config| config.default_model)
    }

    pub fn default_from_config() -> Option<Self> {
        ProviderConfig::load()
            .ok()
            .and_then(|config| config.default_provider())
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
