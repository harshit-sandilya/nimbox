#!/usr/bin/env python3
"""
Manual benchmark helper for Nimbox using OpenAI Python SDK.

Test 1 (latency):
- Compare direct provider call vs call routed through Nimbox.

Test 2 (rotation):
- Stress OpenRouter and compare:
  - direct-single-key (OPENROUTER_KEY_1 only)
  - nimbox-multi-key (expects multiple keys configured in Nimbox)
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import random
import string
import time
from dataclasses import asdict, dataclass
from typing import Dict, List, Optional, Tuple

from openai import OpenAI


def _rand(n: int = 8) -> str:
    return "".join(random.choices(string.ascii_lowercase + string.digits, k=n))


@dataclass
class Hit:
    scenario: str
    index: int
    ok: bool
    status: int
    latency_ms: float
    error: Optional[str]


def _extract_error(e: Exception) -> Tuple[int, str]:
    status = int(getattr(e, "status_code", 0) or 0)
    message = str(e)

    # OpenAI SDK exceptions often include response body on .response
    resp = getattr(e, "response", None)
    if resp is not None:
        try:
            text = resp.text
            if text:
                message = text
        except Exception:
            pass

    return status, message[:500]


def get_provider_and_model(
    provider_arg: str, model_arg: Optional[str]
) -> Tuple[str, str]:
    provider = provider_arg
    if provider not in ("openrouter", "nvidia-nim"):
        raise SystemExit("--provider must be openrouter or nvidia-nim")

    if model_arg:
        return provider, model_arg

    if provider == "openrouter":
        return provider, os.getenv("OPENROUTER_MODEL", "openrouter/owl-alpha")

    return provider, os.getenv("NIM_MODEL", "meta/llama-4-maverick-17b-128e-instruct")


def _build_prompt(base_prompt: str, repeat: int) -> str:
    return "\n".join([base_prompt] * max(1, repeat))


def _sdk_chat_call(
    *,
    client: OpenAI,
    model: str,
    prompt: str,
    timeout_s: float,
    extra_headers: Optional[Dict[str, str]] = None,
    max_tokens: int = 40,
) -> Tuple[bool, int, float, str]:
    started = time.perf_counter()
    try:
        _ = client.chat.completions.create(
            model=model,
            messages=[
                {
                    "role": "user",
                    "content": f"{prompt}\nnonce={_rand()}\nRespond in one short line.",
                }
            ],
            temperature=0,
            max_tokens=max_tokens,
            timeout=timeout_s,
            extra_headers=extra_headers,
        )
        elapsed = (time.perf_counter() - started) * 1000
        return True, 200, elapsed, ""
    except Exception as e:  # noqa: BLE001
        elapsed = (time.perf_counter() - started) * 1000
        status, msg = _extract_error(e)
        return False, status, elapsed, msg


def _nimbox_sdk_base_url() -> str:
    raw = os.getenv("NIMBOX_BASE_URL", "http://localhost:11500").rstrip("/")
    # OpenAI SDK expects base_url that already includes /v1
    return raw if raw.endswith("/v1") else f"{raw}/v1"


def _build_clients(provider: str) -> Tuple[OpenAI, OpenAI, Optional[Dict[str, str]]]:
    # returns: direct_client, nimbox_client, direct_extra_headers
    if provider == "openrouter":
        direct_key = os.getenv("OPENROUTER_KEY_1", "").strip()
        if not direct_key:
            raise SystemExit("OPENROUTER_KEY_1 is required for direct openrouter calls")

        direct_client = OpenAI(
            api_key=direct_key, base_url="https://openrouter.ai/api/v1"
        )
        direct_headers = {
            "HTTP-Referer": "https://github.com/harshit-sandilya/nimbox",
            "X-Title": "nimbox-manual-benchmark",
        }
    else:
        direct_key = os.getenv("NIM_KEY_1", "").strip()
        if not direct_key:
            raise SystemExit("NIM_KEY_1 is required for direct nvidia-nim calls")

        direct_client = OpenAI(
            api_key=direct_key, base_url="https://integrate.api.nvidia.com/v1"
        )
        direct_headers = None

    nimbox_client = OpenAI(api_key="nimbox", base_url=_nimbox_sdk_base_url())
    return direct_client, nimbox_client, direct_headers


def _percentile(values: List[float], pct: float) -> Optional[float]:
    if not values:
        return None
    if len(values) == 1:
        return round(values[0], 2)

    sorted_vals = sorted(values)
    pos = (pct / 100.0) * (len(sorted_vals) - 1)
    lo = math.floor(pos)
    hi = math.ceil(pos)
    if lo == hi:
        return round(sorted_vals[lo], 2)
    frac = pos - lo
    interpolated = sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * frac
    return round(interpolated, 2)


def summarize(hits: List[Hit]) -> Dict:
    total = len(hits)
    success = sum(1 for h in hits if h.ok)
    fails = total - success
    only_429 = sum(1 for h in hits if h.status == 429)
    lats = [h.latency_ms for h in hits]

    status_counts: Dict[str, int] = {}
    for h in hits:
        key = str(h.status)
        status_counts[key] = status_counts.get(key, 0) + 1

    if not lats:
        return {
            "total": 0,
            "success": 0,
            "failures": 0,
            "rate_429": 0,
            "avg_ms": None,
            "p50_ms": None,
            "p95_ms": None,
            "min_ms": None,
            "max_ms": None,
            "status_counts": status_counts,
        }

    avg = sum(lats) / len(lats)
    return {
        "total": total,
        "success": success,
        "failures": fails,
        "rate_429": only_429,
        "avg_ms": round(avg, 2),
        "p50_ms": _percentile(lats, 50),
        "p95_ms": _percentile(lats, 95),
        "min_ms": round(min(lats), 2),
        "max_ms": round(max(lats), 2),
        "status_counts": status_counts,
    }


def print_summary(name: str, stats: Dict) -> None:
    print(
        f"{name}: total={stats['total']} success={stats['success']} failures={stats['failures']} "
        f"429={stats['rate_429']} avg_ms={stats['avg_ms']} p50_ms={stats['p50_ms']} "
        f"p95_ms={stats['p95_ms']} min_ms={stats['min_ms']} max_ms={stats['max_ms']} "
        f"statuses={stats['status_counts']}"
    )


def latency_test(args: argparse.Namespace) -> int:
    provider, model = get_provider_and_model(args.provider, args.model)
    prompt = _build_prompt(args.prompt, args.prompt_repeat)

    print(
        f"[latency] provider={provider} model={model} samples={args.samples} warmup={args.warmup}"
    )

    direct_client, nimbox_client, direct_headers = _build_clients(provider)

    def direct_once() -> Tuple[bool, int, float, str]:
        return _sdk_chat_call(
            client=direct_client,
            model=model,
            prompt=prompt,
            timeout_s=args.timeout_s,
            extra_headers=direct_headers,
            max_tokens=args.max_tokens,
        )

    def nimbox_once() -> Tuple[bool, int, float, str]:
        return _sdk_chat_call(
            client=nimbox_client,
            model=model,
            prompt=prompt,
            timeout_s=args.timeout_s,
            max_tokens=args.max_tokens,
        )

    # Warmup (not measured)
    for _ in range(args.warmup):
        _ = direct_once()
        _ = nimbox_once()

    direct_hits: List[Hit] = []
    nimbox_hits: List[Hit] = []

    for i in range(args.samples):
        if args.alternate_order and i % 2 == 1:
            ok, status, ms, err = nimbox_once()
            nimbox_hits.append(Hit("nimbox", i, ok, status, ms, err or None))

            ok, status, ms, err = direct_once()
            direct_hits.append(Hit("direct", i, ok, status, ms, err or None))
        else:
            ok, status, ms, err = direct_once()
            direct_hits.append(Hit("direct", i, ok, status, ms, err or None))

            ok, status, ms, err = nimbox_once()
            nimbox_hits.append(Hit("nimbox", i, ok, status, ms, err or None))

    direct_stats = summarize(direct_hits)
    nimbox_stats = summarize(nimbox_hits)

    print_summary("direct", direct_stats)
    print_summary("nimbox", nimbox_stats)

    if direct_stats["avg_ms"] is not None and nimbox_stats["avg_ms"] is not None:
        delta = round(nimbox_stats["avg_ms"] - direct_stats["avg_ms"], 2)
        print(f"delta_ms (nimbox - direct): {delta}")

    if args.out:
        report = {
            "test": "latency",
            "provider": provider,
            "model": model,
            "samples": args.samples,
            "warmup": args.warmup,
            "alternate_order": args.alternate_order,
            "prompt_repeat": args.prompt_repeat,
            "max_tokens": args.max_tokens,
            "direct": {
                "summary": direct_stats,
                "hits": [asdict(h) for h in direct_hits],
            },
            "nimbox": {
                "summary": nimbox_stats,
                "hits": [asdict(h) for h in nimbox_hits],
            },
        }
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2)
        print(f"saved report: {args.out}")

    return 0


def _openrouter_keys() -> List[str]:
    keys = []
    for i in range(1, 10):
        v = os.getenv(f"OPENROUTER_KEY_{i}", "").strip()
        if v:
            keys.append(v)
    return keys


def run_parallel(
    *,
    scenario: str,
    requests_count: int,
    concurrency: int,
    fn,
) -> List[Hit]:
    hits: List[Hit] = []

    def one(i: int) -> Hit:
        ok, status, ms, err = fn()
        return Hit(scenario, i, ok, status, ms, err or None)

    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as ex:
        futs = [ex.submit(one, i) for i in range(requests_count)]
        for fut in concurrent.futures.as_completed(futs):
            hits.append(fut.result())

    hits.sort(key=lambda x: x.index)
    return hits


def rotation_test(args: argparse.Namespace) -> int:
    keys = _openrouter_keys()
    if len(keys) < 3:
        raise SystemExit(
            "rotation test needs at least OPENROUTER_KEY_1..OPENROUTER_KEY_3"
        )

    model = args.model or os.getenv("OPENROUTER_MODEL", "openrouter/owl-alpha")
    prompt = _build_prompt(args.prompt, args.prompt_repeat)

    print(
        f"[rotation] openrouter model={model} requests={args.requests} concurrency={args.concurrency} warmup={args.warmup}"
    )
    print(
        "This compares direct-single-key vs nimbox-multi-key (keys already configured in nimbox CLI)."
    )

    direct_key = os.getenv("OPENROUTER_KEY_1", "").strip()
    if not direct_key:
        raise SystemExit("OPENROUTER_KEY_1 is required")

    direct_client = OpenAI(api_key=direct_key, base_url="https://openrouter.ai/api/v1")
    nimbox_client = OpenAI(api_key="nimbox", base_url=_nimbox_sdk_base_url())

    direct_headers = {
        "HTTP-Referer": "https://github.com/harshit-sandilya/nimbox",
        "X-Title": "nimbox-manual-benchmark",
    }

    def direct_single() -> Tuple[bool, int, float, str]:
        return _sdk_chat_call(
            client=direct_client,
            model=model,
            prompt=prompt,
            timeout_s=args.timeout_s,
            extra_headers=direct_headers,
            max_tokens=args.max_tokens,
        )

    def via_nimbox() -> Tuple[bool, int, float, str]:
        return _sdk_chat_call(
            client=nimbox_client,
            model=model,
            prompt=prompt,
            timeout_s=args.timeout_s,
            max_tokens=args.max_tokens,
        )

    # Warmup per scenario
    for _ in range(args.warmup):
        _ = direct_single()
    for _ in range(args.warmup):
        _ = via_nimbox()

    direct_hits = run_parallel(
        scenario="direct-single-key",
        requests_count=args.requests,
        concurrency=args.concurrency,
        fn=direct_single,
    )

    nimbox_hits = run_parallel(
        scenario="nimbox-multi-key",
        requests_count=args.requests,
        concurrency=args.concurrency,
        fn=via_nimbox,
    )

    direct_stats = summarize(direct_hits)
    nimbox_stats = summarize(nimbox_hits)

    print_summary("direct-single-key", direct_stats)
    print_summary("nimbox-multi-key", nimbox_stats)

    if direct_stats["rate_429"] > nimbox_stats["rate_429"]:
        print(
            "result: nimbox likely rotated away from limited keys (fewer 429 than direct single key)."
        )
    elif direct_stats["rate_429"] == nimbox_stats["rate_429"]:
        print(
            "result: similar 429 count. Increase --requests/--concurrency/--prompt-repeat/--max-tokens to force limits."
        )
    else:
        print("result: nimbox had more 429s in this run; check key setup and rerun.")

    if args.out:
        report = {
            "test": "rotation",
            "model": model,
            "requests": args.requests,
            "concurrency": args.concurrency,
            "warmup": args.warmup,
            "prompt_repeat": args.prompt_repeat,
            "max_tokens": args.max_tokens,
            "direct_single_key": {
                "summary": direct_stats,
                "hits": [asdict(h) for h in direct_hits],
            },
            "nimbox_multi_key": {
                "summary": nimbox_stats,
                "hits": [asdict(h) for h in nimbox_hits],
            },
        }
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(report, f, indent=2)
        print(f"saved report: {args.out}")

    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="Manual Nimbox benchmark")
    sub = p.add_subparsers(dest="cmd", required=True)

    p_latency = sub.add_parser("latency", help="Test 1: direct vs nimbox latency")
    p_latency.add_argument(
        "--provider", default="openrouter", choices=["openrouter", "nvidia-nim"]
    )
    p_latency.add_argument("--model", default=None)
    p_latency.add_argument("--samples", type=int, default=10)
    p_latency.add_argument("--warmup", type=int, default=2)
    p_latency.add_argument("--timeout-s", type=float, default=60.0)
    p_latency.add_argument("--prompt", default="Say hello")
    p_latency.add_argument("--prompt-repeat", type=int, default=1)
    p_latency.add_argument("--max-tokens", type=int, default=40)
    p_latency.add_argument(
        "--alternate-order",
        action=argparse.BooleanOptionalAction,
        default=True,
    )
    p_latency.add_argument("--out", default=None)

    p_rotation = sub.add_parser(
        "rotation", help="Test 2: openrouter multi-key rotation via nimbox"
    )
    p_rotation.add_argument("--model", default=None)
    p_rotation.add_argument("--requests", type=int, default=60)
    p_rotation.add_argument("--concurrency", type=int, default=20)
    p_rotation.add_argument("--warmup", type=int, default=2)
    p_rotation.add_argument("--timeout-s", type=float, default=60.0)
    p_rotation.add_argument("--prompt", default="Reply with ok")
    p_rotation.add_argument("--prompt-repeat", type=int, default=1)
    p_rotation.add_argument("--max-tokens", type=int, default=60)
    p_rotation.add_argument("--out", default=None)

    return p


def main() -> int:
    args = build_parser().parse_args()

    if args.cmd == "latency":
        return latency_test(args)

    if args.cmd == "rotation":
        return rotation_test(args)

    raise SystemExit(2)


if __name__ == "__main__":
    raise SystemExit(main())
