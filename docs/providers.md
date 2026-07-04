# Provider Configuration

Provider credentials are read from environment variables first:

- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`

The harness also reads `.mc/config.json`, which is ignored by git. This file can set defaults and local API keys:

```json
{
  "default_provider": "anthropic",
  "default_model": "provider-model-name",
  "anthropic_api_key": "sk-ant-...",
  "openai_api_key": "sk-..."
}
```

The TUI reports only whether credentials are available and which source was used. It does not display secret values.

You can create this config from the TUI with:

```text
/gas
```

The setup flow asks for `openai`, `anthropic`, `other`, or `cancel`. For OpenAI and Anthropic it then asks for the API key, masks typed input, and saves the key to `.mc/config.json`.
