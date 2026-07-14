# Nimbox

Nimbox is a local cross-provider API testing and compatibility proxy. Point OpenAI, Anthropic, or Gemini clients at one local server, then switch the backend without rewriting the application you are testing.

Nimbox normalizes chat, streaming, tool calls, reasoning controls, model discovery, and embeddings across local and cloud providers. It also rotates provider-scoped API keys and cools down keys that hit rate limits.

## What it supports

### Client-facing APIs

- OpenAI Chat Completions, Responses, Embeddings, and Models APIs
- Anthropic Messages API, including SSE streaming and tool use
- Gemini `generateContent`, `streamGenerateContent`, embeddings, and Models APIs
- Text conversations, system instructions, tool/function calls, token usage, streaming, and reasoning/thinking controls

### Providers

| Provider         | CLI name     | API key | Chat | Embeddings |
| ---------------- | ------------ | ------: | ---: | ---------: |
| Ollama           | `ollama`     |      No |  Yes |        Yes |
| Groq Cloud       | `groq`       |     Yes |  Yes |         No |
| Google AI Studio | `ai-studio`  |     Yes |  Yes |        Yes |
| NVIDIA NIM       | `nvidia-nim` |     Yes |  Yes |        Yes |
| OpenRouter       | `openrouter` |     Yes |  Yes |         No |
| OpenAI           | `openai`     |     Yes |  Yes |        Yes |

Groq currently has no general text-embeddings endpoint, so Nimbox returns a clear error for embedding requests when Groq is active.

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/harshit-sandilya/nimbox/main/install.sh | sh
```

Or build it from source:

```bash
cargo build --release
```

## Quick start with Ollama

Ollama listens on port `11434`. Nimbox deliberately defaults to `11500`, so both services can run together.

```bash
ollama pull gemma3
ollama pull embeddinggemma

nimbox provider ollama
nimbox model gemma3
nimbox embed embeddinggemma
nimbox start
```

Nimbox is now available at `http://localhost:11500`. Ollama does not need a placeholder API key.

To use a non-default Ollama server:

```bash
NIMBOX_OLLAMA_URL=http://other-host:11434 nimbox start
```

## Cloud providers

Select a provider before adding its keys. Keys, chat models, and embedding models are stored separately for every provider, so switching back restores that provider's settings.

### Groq Cloud

```bash
nimbox provider groq
nimbox add -n primary "$GROQ_API_KEY"
nimbox model llama-3.3-70b-versatile
nimbox start
```

### Google AI Studio

```bash
nimbox provider ai-studio
nimbox add -n primary "$GEMINI_API_KEY"
nimbox model gemini-2.5-flash
nimbox embed gemini-embedding-001
nimbox start
```

The `gemini` and `google-ai-studio` provider names are accepted as aliases for `ai-studio`.

### Other providers

```bash
nimbox provider nvidia-nim   # or openrouter / openai
nimbox add -n primary YOUR_API_KEY
nimbox model PROVIDER_MODEL_ID
nimbox embed PROVIDER_EMBEDDING_MODEL_ID  # when supported
nimbox start
```

Run `nimbox provider --list` to see all canonical provider names.

## Model handling

Nimbox keeps two model roles for each provider:

- `nimbox model <id>` sets that provider's fallback chat model.
- `nimbox embed <id>` sets that provider's fallback embedding model.

If an incoming request contains a model, Nimbox uses it for that request. Otherwise, it uses the saved fallback for the active provider. This makes it possible to test several Ollama models through one running proxy without repeatedly changing configuration, while keeping a dedicated embedding model such as `embeddinggemma`.

`GET /v1/models` and `GET /v1beta/models` query the active provider. Ollama models come from `/api/tags`; AI Studio models include their advertised generation and embedding capabilities.

Existing Nimbox config files are migrated automatically: old global models and keys are assigned to the provider that owned them when you first switch providers.

## API endpoints

### OpenAI-compatible

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/models`

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:11500/v1",
    api_key="nimbox",  # Nimbox manages the upstream credential
)

response = client.chat.completions.create(
    model="gemma3",
    messages=[{"role": "user", "content": "Hello"}],
)
print(response.choices[0].message.content)
```

### Anthropic-compatible

- `POST /v1/messages`

```bash
export ANTHROPIC_BASE_URL=http://localhost:11500
export ANTHROPIC_API_KEY=nimbox
```

The request's `model` is passed to the active provider. If it is empty or omitted, Nimbox uses the configured chat model.

### Gemini-compatible

- `POST /v1beta/models/{model}:generateContent`
- `POST /v1beta/models/{model}:streamGenerateContent`
- `POST /v1beta/models/{model}:embedContent`
- `POST /v1beta/models/{model}:batchEmbedContents`
- `GET /v1beta/models`
- `GET /v1beta/models/{model}`

```bash
curl http://localhost:11500/v1beta/models/gemma3:generateContent \
  -H 'Content-Type: application/json' \
  -d '{
    "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
  }'
```

The Gemini-facing server works with any active Nimbox provider; it is not limited to Google AI Studio.

## Streaming and tools

Nimbox translates provider streams into the protocol used by the calling client:

- OpenAI data-only SSE with `[DONE]`
- Anthropic named SSE events and content blocks
- Gemini `GenerateContentResponse` SSE chunks

OpenAI tools, Anthropic tool use/results, and Gemini function declarations/calls are normalized into one internal representation. Provider support still depends on the selected model.

Reasoning controls are normalized where the provider exposes an equivalent:

- OpenAI-style `reasoning_effort` or `reasoning.effort`
- Anthropic-style `thinking.budget_tokens`
- Gemini `generationConfig.thinkingConfig`

## Key management

Commands operate on the active provider:

```bash
nimbox add -n primary YOUR_API_KEY
nimbox add -n backup ANOTHER_API_KEY
nimbox list
nimbox remove -n backup
nimbox remove --all
```

Nimbox rotates these keys and temporarily cools down a key after a rate-limit response. Set `NIMBOX_DEBUG_KEYS=1` to include key-pool diagnostics in logs; key values are never printed.

## CLI

```text
nimbox start [--port 11500]
nimbox stop
nimbox provider <name>
nimbox provider --list
nimbox model <model-id>
nimbox embed <model-id>
nimbox add -n <name> <api-key>
nimbox list
nimbox remove -n <name>
nimbox remove --all
nimbox info
nimbox update
```

`nimbox start` runs as a background process. Use `nimbox stop` before changing the active provider for a running server, then start it again.

## Architecture

```text
OpenAI / Anthropic / Gemini client
                 |
       protocol-specific server
                 |
      normalized request/events
                 |
 active provider adapter + key pool
                 |
 Ollama / Groq / AI Studio / NIM / OpenRouter / OpenAI
```

Provider adapters live in `src/providers/`; client-facing protocol servers live in `src/server/`. This keeps upstream provider details separate from the API shape used by a test client.

## Development

### Docker smoke test with Ollama

The Compose stack includes the official Ollama container, persistent model storage, and an internal `NIMBOX_OLLAMA_URL=http://ollama:11434` connection. On the host, Nimbox is exposed at `11500` and the containerized Ollama API is exposed at `11435`, leaving the usual local Ollama port `11434` free.

Start both containers and open the Nimbox development shell:

```bash
docker compose up -d --build
docker compose exec nimbox bash
```

Then run the end-to-end smoke test inside that shell:

```bash
./test/ollama-smoke.sh
```

The first run downloads `qwen2.5:0.5b` and `all-minilm` into the persistent `ollama_data` volume. Later runs reuse them. The test starts Nimbox and verifies:

- live model discovery from Ollama
- an OpenAI-compatible chat request
- an OpenAI-compatible embedding request
- a Gemini-compatible request routed to Ollama

You can override the test models:

```bash
NIMBOX_TEST_CHAT_MODEL=gemma3 \
NIMBOX_TEST_EMBED_MODEL=embeddinggemma \
./test/ollama-smoke.sh
```

From the host, the two services are available at:

```text
Nimbox: http://localhost:11500
Ollama: http://localhost:11435
```

Stop the stack without deleting downloaded models:

```bash
docker compose down
```

Use `docker compose down --volumes` only when you also want to remove the downloaded Ollama models and Nimbox configuration.

### Rust development

```bash
cargo test
cargo run -- provider ollama
cargo run -- model gemma3
cargo run -- start
```

Nimbox configuration persists in `nimbox_data`; Ollama models persist in `ollama_data`.

## License

MIT. Nimbox is not affiliated with Anthropic, Google, Groq, NVIDIA, Ollama, OpenAI, or OpenRouter.
