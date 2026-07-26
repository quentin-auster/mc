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

## Invocation contract

`mc-application` defines provider-neutral asynchronous request, response, usage, and failure types.
The first infrastructure adapter calls OpenAI's Responses API. It sends the selected model and the
JSON value stored in the request artifact, with provider-side response storage disabled.

Every invocation is inserted as `running` before network activity. A terminal record retains the
provider and model, request and response artifact IDs, latency, provider request ID, completion
time, and either token usage or structured error fields. Provider response and error bodies are
stored as content-addressed artifacts. API keys are confined to the provider adapter, sent only in
the authorization header, redacted from debug output, and never persisted with invocation data.

You can create this config from the TUI with:

```text
/gas
```

The setup flow asks for `openai`, `anthropic`, `other`, or `cancel`. For OpenAI and Anthropic it then asks for the API key, masks typed input, and saves the key to `.mc/config.json`.
