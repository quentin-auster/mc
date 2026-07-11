use crate::provider::Provider;

const ANTHROPIC_DEFAULT_CONTEXT_WINDOW: usize = 200_000;
const OPENAI_DEFAULT_CONTEXT_WINDOW: usize = 128_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TokenBudgetMode {
    Cheap,
    Balanced,
    Deep,
    Custom(usize),
}

impl TokenBudgetMode {
    pub fn prompt_budget(self, context_window: usize) -> usize {
        match self {
            Self::Cheap => context_window / 5,
            Self::Balanced => (context_window * 2) / 5,
            Self::Deep => (context_window * 7) / 10,
            Self::Custom(tokens) => tokens.min(context_window),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TokenBudget {
    pub context_window: usize,
    pub prompt_budget: usize,
    pub mode: TokenBudgetMode,
}

impl TokenBudget {
    pub fn for_model(provider: Provider, model: Option<&str>, mode: TokenBudgetMode) -> Self {
        let context_window = model_context_window(provider, model);
        Self {
            context_window,
            prompt_budget: mode.prompt_budget(context_window),
            mode,
        }
    }
}

pub fn estimate_text(text: &str) -> usize {
    estimate_bytes(text.len())
}

pub fn estimate_bytes(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

pub fn model_context_window(provider: Provider, model: Option<&str>) -> usize {
    let Some(model) = model.map(str::to_lowercase) else {
        return default_context_window(provider);
    };

    if model.contains("claude") {
        ANTHROPIC_DEFAULT_CONTEXT_WINDOW
    } else if model.contains("gpt") || model.contains("o1") || model.contains("o3") {
        OPENAI_DEFAULT_CONTEXT_WINDOW
    } else {
        default_context_window(provider)
    }
}

fn default_context_window(provider: Provider) -> usize {
    match provider {
        Provider::Anthropic => ANTHROPIC_DEFAULT_CONTEXT_WINDOW,
        Provider::OpenAi => OPENAI_DEFAULT_CONTEXT_WINDOW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_tokens_from_bytes_conservatively() {
        assert_eq!(estimate_bytes(0), 0);
        assert_eq!(estimate_bytes(1), 1);
        assert_eq!(estimate_bytes(8), 2);
        assert_eq!(estimate_text("abcd"), 1);
    }

    #[test]
    fn budget_modes_scale_with_context_window() {
        assert_eq!(TokenBudgetMode::Cheap.prompt_budget(100_000), 20_000);
        assert_eq!(TokenBudgetMode::Balanced.prompt_budget(100_000), 40_000);
        assert_eq!(TokenBudgetMode::Deep.prompt_budget(100_000), 70_000);
        assert_eq!(
            TokenBudgetMode::Custom(150_000).prompt_budget(100_000),
            100_000
        );
    }

    #[test]
    fn budget_uses_provider_and_model_context_windows() {
        let anthropic = TokenBudget::for_model(Provider::Anthropic, None, TokenBudgetMode::Cheap);
        assert_eq!(anthropic.context_window, 200_000);
        assert_eq!(anthropic.prompt_budget, 40_000);

        let openai = TokenBudget::for_model(
            Provider::OpenAi,
            Some("gpt-5-mini"),
            TokenBudgetMode::Balanced,
        );
        assert_eq!(openai.context_window, 128_000);
        assert_eq!(openai.prompt_budget, 51_200);
    }
}
