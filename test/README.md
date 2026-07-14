# Manual benchmark script (OpenAI SDK)

This flow is manual:

- Build/start container with Docker Compose
- Configure Nimbox via CLI inside container
- Set env vars for direct-call tests
- Run `test/benchmark.py` manually

The script uses the **OpenAI Python SDK** for both direct provider requests and Nimbox-routed requests.

---

## 0) Start container

From repo root:

```bash
docker compose build
```

Start Nimbox and its Ollama dependency, then enter the Nimbox shell:

```bash
docker compose up -d --build
docker compose exec nimbox bash
```

> Docker image installs Python + pip deps (`test/requirements.txt`) during build.

For a quick Ollama integration check instead of the cloud benchmark flow below, run:

```bash
./test/ollama-smoke.sh
```

It downloads small chat and embedding models on the first run, then tests model discovery, chat, embeddings, and the Gemini-compatible route through Nimbox.

---

## 1) Default models

If `--model` is not passed, script defaults are:

- NVIDIA NIM: `meta/llama-4-maverick-17b-128e-instruct`
- OpenRouter: `openrouter/owl-alpha`

You can override with env vars:

- `NIM_MODEL`
- `OPENROUTER_MODEL`

---

## 2) Expected env keys

### Latency test

- For NIM direct call: `NIM_KEY_1`
- For OpenRouter direct call: `OPENROUTER_KEY_1`

### Rotation test (OpenRouter)

- `OPENROUTER_KEY_1`
- `OPENROUTER_KEY_2`
- `OPENROUTER_KEY_3`

Nimbox should also have those keys added via CLI for routed rotation behavior.

Optional Nimbox URL override:

- `NIMBOX_BASE_URL` (default: `http://localhost:11500`)

Important with OpenAI SDK:

- SDK base URL must include `/v1`.
- This script auto-normalizes it, so both work:
    - `http://localhost:11500`
    - `http://localhost:11500/v1`

---

## 3) Configure Nimbox manually (inside container)

### OpenRouter setup

```bash
nimbox provider openrouter
nimbox model openrouter/owl-alpha
nimbox remove --all
nimbox add -n key1 "$OPENROUTER_KEY_1"
nimbox add -n key2 "$OPENROUTER_KEY_2"
nimbox add -n key3 "$OPENROUTER_KEY_3"
NIMBOX_DAEMON=1 nimbox start --port 11500
```

### NVIDIA NIM setup

```bash
nimbox provider nvidia-nim
nimbox model meta/llama-4-maverick-17b-128e-instruct
nimbox remove --all
nimbox add -n key1 "$NIM_KEY_1"
NIMBOX_DAEMON=1 nimbox start --port 11500
```

---

## 4) Run tests

### Test 1: latency (direct vs nimbox)

Use more stable settings (warmup + more samples):

OpenRouter:

```bash
python3 test/benchmark.py latency \
  --provider openrouter \
  --samples 20 \
  --warmup 3 \
  --alternate-order \
  --out test/latency_openrouter.json
```

NVIDIA NIM:

```bash
python3 test/benchmark.py latency \
  --provider nvidia-nim \
  --samples 20 \
  --warmup 3 \
  --alternate-order \
  --out test/latency_nim.json
```

### Test 2: OpenRouter key rotation

Start with a higher-pressure run:

```bash
python3 test/benchmark.py rotation \
  --requests 500 \
  --concurrency 100 \
  --warmup 3 \
  --prompt-repeat 10 \
  --max-tokens 120 \
  --out test/rotation_openrouter.json
```

Rotation compares:

- `direct-single-key` (only `OPENROUTER_KEY_1`)
- `nimbox-multi-key` (through Nimbox with multiple configured keys)

---

## 5) Debug key rotation (which key was used)

Enable key debug logs before starting Nimbox:

```bash
export NIMBOX_DEBUG_KEYS=1
NIMBOX_DAEMON=1 nimbox start --port 11500
```

You will see logs like:

- `chat attempt=1 key=key1 selected`
- `chat attempt=1 key=key1 rate_limited ...`
- `chat attempt=2 key=key2 selected`
- `chat attempt=3 key=key3 selected`

This confirms whether requests are rotating across keys.

---

## 6) If rate limit is not triggered

Increase pressure with all four knobs:

- `--requests` (e.g. 300+)
- `--concurrency` (e.g. 80+)
- `--prompt-repeat` (e.g. 8-20) to increase prompt tokens
- `--max-tokens` (e.g. 120-300) to increase completion tokens

The script now also prints `p50_ms`, `p95_ms`, and `status` distribution for easier analysis.
