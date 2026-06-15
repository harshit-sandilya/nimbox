# Nimbox

Nimbox is a local compatibility proxy that allows you to use **OpenAI SDKs** and **Anthropic SDKs** interchangeably while routing requests through **NVIDIA NIM**.

It acts as a translation layer between SDKs and providers, enabling:

- OpenAI-compatible API server
- Anthropic-compatible API behavior
- Streaming (SSE) support
- Key rotation & rate-limit handling
- Unified local dev server

---

## 🚀 Features

- OpenAI Chat Completions compatible endpoint
- OpenAI Responses API endpoint (`/v1/responses`)
- OpenAI Models endpoint (`/v1/models`) for single configured model
- Anthropic-style request support
- Server-Sent Events (SSE) streaming
- Reasoning/thinking controls pass-through (`reasoning_effort`, `thinking.budget_tokens`)
- NVIDIA NIM / OpenRouter / OpenAI backend integration
- API key management with rotation
- Local dev-friendly CLI

---

## 📦 Installation

Install Nimbox using the official install script:

```bash
curl -fsSL https://raw.githubusercontent.com/harshit-sandilya/nimbox/main/install.sh | sh
```

This will:

- Download the latest release
- Install the `nimbox` binary

---

## 🔑 NVIDIA API Key

Nimbox supports multiple backends/providers including NVIDIA NIM, OpenRouter, and OpenAI.

Get your API key from:

[https://build.nvidia.com/](https://build.nvidia.com/)
or
[https://openrouter.ai/](https://openrouter.ai/)
or
[https://platform.openai.com/](https://platform.openai.com/)

Then add it:

```bash
nimbox add -n default YOUR_API_KEY
```

If OpenRouter, use:

```bash
nimbox provider openrouter
```

If OpenAI, use:

```bash
nimbox provider openai
```

---

## ⚠️ Important

- Streaming models are required for full functionality
- Ensure the selected model supports streaming (`stream: true`)
- Without streaming support, SSE-based clients (Claude Code, etc.) may fail

---

## 🧠 Quick Start

### Provider defaults

Nimbox provider-specific chat model defaults:

- `nvidia-nim` → `meta/llama-4-maverick-17b-128e-instruct`
- `openrouter` → `openrouter/owl2`
- `openai` → `gpt-5.4`

Embedding default:

- `nvidia-nim` → `nvidia/nv-embed-v1`
- `openai` → `text-embedding-3-small`
- `openrouter` → embedding not supported

### 1. Start server

```bash
nimbox start --port 11434
```

Server runs on:

```
http://localhost:11434
```

---

### 2. Set model

```bash
# nvidia-nim default
nimbox model meta/llama-4-maverick-17b-128e-instruct

# openrouter default
nimbox model openrouter/owl2

# openai default
nimbox model gpt-5.4
```

---

### 3. Set embedding model

```bash
# nvidia-nim default
nimbox embed nvidia/nv-embed-v1

# openai default
nimbox embed text-embedding-3-small
```

---

### 4. Add API key

```bash
nimbox add -n default YOUR_API_KEY
```

---

### 5. List keys

```bash
nimbox list
```

---

### 6. Remove key

```bash
nimbox remove -n default
```

---

## 🔌 API Compatibility

Implemented endpoints:

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/models` (returns only the currently configured single model)
- `POST /v1/messages` (Anthropic-style)

### OpenAI SDK

Point your client to:

```
http://localhost:11434/
```

Example:

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:11434/",
    api_key="nimbox"
)
```

---

### Anthropic

Set:

```
ANTHROPIC_BASE_URL=http://localhost:11434
```

Nimbox translates requests into internal format and routes them to the configured provider (NVIDIA NIM / OpenRouter / OpenAI).

Reasoning/thinking support:

- OpenAI-style: `reasoning_effort` (or `reasoning.effort`)
- Anthropic-style: `thinking.budget_tokens`

These fields are normalized into Nimbox internal request DTOs and passed to providers where applicable.

---

## 🌊 Streaming (SSE)

Streaming responses are supported via:

- OpenAI `stream: true`
- Anthropic SSE mode

Example:

```bash
curl http://localhost:11434/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "moonshotai/kimi-k2.6",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }'
```

---

## 🧩 Commands

### Start server

```bash
nimbox start --port 11434
```

### Stop server

```bash
nimbox stop
```

### Add API key

```bash
nimbox add -n <name> <key>
```

### Remove API key

```bash
nimbox remove -n <name>
```

### List API keys

```bash
nimbox list
```

### Set model

```bash
nimbox model <model-name>
```

### Set embedding model

```bash
nimbox embed <model-name>
```

---

## 🏗 Architecture

```
OpenAI SDK / Anthropic SDK
            ↓
     Nimbox Proxy Server
            ↓
   Internal Request Format
            ↓
      NVIDIA NIM API
```

---

## 📡 Backend

Nimbox uses:

- NVIDIA NIM API: [https://integrate.api.nvidia.com/v1](https://integrate.api.nvidia.com/v1)
- Chat Completions endpoint
- Embeddings endpoint

---

## 📜 License

MIT License

---

## 🤝 Contributing

Pull requests are welcome. For major changes, open an issue first.

---

## ⚠️ Disclaimer

Nimbox is not affiliated with OpenAI, Anthropic, or NVIDIA.
