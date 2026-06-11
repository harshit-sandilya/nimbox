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
- Anthropic-style request support
- Server-Sent Events (SSE) streaming
- NVIDIA NIM backend integration
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

Nimbox uses NVIDIA NIM as its backend. And supports open-router too.

Get your API key from:

[https://build.nvidia.com/](https://build.nvidia.com/)
or
[https://openrouter.ai/](https://openrouter.ai/)

Then add it:

```bash
nimbox add -n default YOUR_API_KEY
```

If openrouter, use:

```bash
nimbox provider openrouter
```

---

## ⚠️ Important

- Streaming models are required for full functionality
- Ensure the selected model supports streaming (`stream: true`)
- Without streaming support, SSE-based clients (Claude Code, etc.) may fail

---

## 🧠 Quick Start

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
nimbox model meta/llama-3.3-70b-instruct
```

---

### 3. Set embedding model

```bash
nimbox embed nvidia/nv-embed-v1
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

Nimbox translates requests into internal format and routes them to NVIDIA NIM.

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
