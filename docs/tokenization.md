# Tokenization And Budgets

The first context-management implementation uses a shared conservative token
estimate instead of provider-specific tokenizers.

## Decision

- Estimate text as `ceil(byte_count / 4)`.
- Keep the estimate centralized in `token.rs`.
- Treat the estimate as approximate, with an expected error tolerance of about
  25%.
- Cache future token counts by content hash when persistence exists.
- Prefer provider-specific tokenizers only if the approximation causes real
  prompt failures or visibly bad budget decisions.

This keeps the harness lightweight: no tokenizer model downloads, no native
tokenizer dependencies, and no provider-specific tokenization path in the first
prompt planner.

## Provider And Model Differences

Provider/model differences are handled at the budget level rather than the
tokenizer level.

The first-pass context-window defaults are:

- Anthropic/Claude: 200k tokens.
- OpenAI/GPT/O-series: 128k tokens.
- Unknown models: provider default.

These are budget defaults, not exact provider guarantees. They should be easy
to override later from config or provider metadata.

## Budget Modes

Prompt planning should use one of these modes:

- `cheap`: up to 20% of the model context window.
- `balanced`: up to 40% of the model context window.
- `deep`: up to 70% of the model context window.
- `custom`: user-selected token cap, never above the context window.

The UI should nudge users toward `cheap` or `balanced` first and make larger
contexts visible before sending.

## Capture And Cache Policy

- Estimate eagerly when context is captured so listings can show rough token
  cost.
- Re-estimate lazily during prompt planning after final text is assembled.
- Cache estimates by content hash once local persistence exists.
- Do not fail prompt planning because token estimation fails; surface the item
  as unknown cost and bias toward excluding or asking the user.

## Follow-On Contract

Prompt planning can rely on:

- a shared text estimate;
- budget modes with provider/model-aware context windows;
- inclusion records that carry estimated tokens;
- visible uncertainty rather than pretending estimates are exact.
