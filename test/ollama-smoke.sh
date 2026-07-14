#!/usr/bin/env bash
set -euo pipefail

OLLAMA_URL="${NIMBOX_OLLAMA_URL:-http://ollama:11434}"
NIMBOX_URL="${NIMBOX_BASE_URL:-http://127.0.0.1:11500}"
CHAT_MODEL="${NIMBOX_TEST_CHAT_MODEL:-qwen2.5:0.5b}"
EMBED_MODEL="${NIMBOX_TEST_EMBED_MODEL:-all-minilm}"

wait_for_url() {
    local name="$1"
    local url="$2"
    for _ in $(seq 1 60); do
        if curl --fail --silent --output /dev/null "$url"; then
            return 0
        fi
        sleep 1
    done
    echo "Timed out waiting for ${name} at ${url}" >&2
    return 1
}

pull_model() {
    local model="$1"
    echo "Pulling ${model} (cached in the ollama_data volume after the first run)..."
    curl --fail --show-error --silent \
        "${OLLAMA_URL}/api/pull" \
        --header 'Content-Type: application/json' \
        --data "{\"model\":\"${model}\",\"stream\":false}" \
        --output /dev/null
}

echo "Waiting for Ollama..."
wait_for_url "Ollama" "${OLLAMA_URL}/api/version"
pull_model "$CHAT_MODEL"
pull_model "$EMBED_MODEL"

nimbox stop >/dev/null 2>&1 || true
nimbox provider ollama
nimbox model "$CHAT_MODEL"
nimbox embed "$EMBED_MODEL"
nimbox start --port 11500

echo "Waiting for Nimbox..."
wait_for_url "Nimbox" "${NIMBOX_URL}/health"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Testing provider-backed model discovery..."
curl --fail --show-error --silent \
    "${NIMBOX_URL}/v1/models" \
    --output "${workdir}/models.json"

echo "Testing OpenAI-compatible chat..."
curl --fail --show-error --silent \
    "${NIMBOX_URL}/v1/chat/completions" \
    --header 'Content-Type: application/json' \
    --data "{\"model\":\"${CHAT_MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: nimbox works\"}],\"max_tokens\":20}" \
    --output "${workdir}/chat.json"

echo "Testing OpenAI-compatible embeddings..."
curl --fail --show-error --silent \
    "${NIMBOX_URL}/v1/embeddings" \
    --header 'Content-Type: application/json' \
    --data "{\"model\":\"${EMBED_MODEL}\",\"input\":\"nimbox embedding smoke test\"}" \
    --output "${workdir}/embedding.json"

echo "Testing Gemini-compatible requests through the Ollama provider..."
curl --fail --show-error --silent \
    "${NIMBOX_URL}/v1beta/models/${CHAT_MODEL}:generateContent" \
    --header 'Content-Type: application/json' \
    --data '{"contents":[{"role":"user","parts":[{"text":"Reply with exactly: gemini route works"}]}],"generationConfig":{"maxOutputTokens":20}}' \
    --output "${workdir}/gemini.json"

python3 - "$workdir" "$CHAT_MODEL" "$EMBED_MODEL" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
chat_model = sys.argv[2]
embed_model = sys.argv[3]

models = json.loads((root / "models.json").read_text())
model_ids = {item["id"] for item in models.get("data", [])}
assert chat_model in model_ids, f"chat model missing from /v1/models: {model_ids}"
assert embed_model in model_ids, f"embedding model missing from /v1/models: {model_ids}"

chat = json.loads((root / "chat.json").read_text())
assert chat["choices"][0]["message"]["content"], f"empty chat response: {chat}"

embedding = json.loads((root / "embedding.json").read_text())
vector = embedding["data"][0]["embedding"]
assert len(vector) > 0, f"empty embedding response: {embedding}"

gemini = json.loads((root / "gemini.json").read_text())
parts = gemini["candidates"][0]["content"]["parts"]
assert any(part.get("text") for part in parts), f"empty Gemini response: {gemini}"

print(f"PASS: discovered {len(model_ids)} models")
print(f"PASS: chat returned {len(chat['choices'][0]['message']['content'])} characters")
print(f"PASS: embedding has {len(vector)} dimensions")
print("PASS: Gemini-compatible request returned text")
PY

echo "Ollama + Nimbox smoke test passed."
