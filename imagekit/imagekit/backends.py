"""Where a request goes: any OpenAI-style server, the Claude API, or a chat window.

`openai` covers local servers (vLLM, SGLang, llama.cpp's server, Ollama, LM
Studio) and OpenRouter, which all take the same chat-completions request.
`anthropic` uses the official SDK (pip install 'plantgen-imagekit[claude]').
Backends are named in ~/.config/imagekit/backends.toml (or --backends FILE):

    [workstation]
    kind = "openai"
    base_url = "http://localhost:8000/v1"
    model = "Qwen/Qwen3-VL-8B-Instruct"
    revision = "<hf commit>"
    parallel = 8

    [openrouter]
    kind = "openai"
    base_url = "https://openrouter.ai/api/v1"
    key_env = "OPENROUTER_API_KEY"      # leave out if a proxy adds the key
    model = "qwen/qwen3-vl-235b-a22b-instruct"

    [claude]
    kind = "anthropic"
    key_env = "ANTHROPIC_API_KEY"
    model = "claude-haiku-5-5"
    effort = "low"

Keys are only ever read from the environment. `--backend NAME` picks one;
`--model`, `--base-url` and `--parallel` override its fields.
"""
from __future__ import annotations

import base64
import json
import os
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

CONFIG = Path(os.environ.get("IMAGEKIT_BACKENDS", Path.home() / ".config" / "imagekit" / "backends.toml"))

PRESETS = {
    "openrouter": {"kind": "openai", "base_url": "https://openrouter.ai/api/v1", "key_env": "OPENROUTER_API_KEY"},
    "ollama": {"kind": "openai", "base_url": "http://localhost:11434/v1"},
    "vllm": {"kind": "openai", "base_url": "http://localhost:8000/v1"},
    "claude": {"kind": "anthropic", "key_env": "ANTHROPIC_API_KEY", "model": "claude-haiku-5-5", "effort": "low"},
    "chat": {"kind": "manual", "model": "chat"},
}

# Claude API list prices, USD per million tokens (input, output); the Batch API halves them.
CLAUDE_PRICES = {"claude-haiku-5-5": (0.10, 0.50), "claude-sonnet-5-5": (2.0, 10.0), "claude-opus-5-5": (4.0, 20.0)}


@dataclass
class Reply:
    text: str
    tokens_in: int | None = None
    tokens_out: int | None = None
    usd: float | None = None
    model: str | None = None  # the exact model the server says answered
    raw: dict = field(default_factory=dict)


class BackendError(Exception):
    pass


def jpeg_b64(data: bytes) -> str:
    return base64.standard_b64encode(data).decode()


class OpenAICompat:
    kind = "openai"

    def __init__(self, cfg: dict):
        self.cfg = cfg
        self.model = cfg["model"]
        self.revision = cfg.get("revision") or cfg["model"]
        self.base_url = cfg["base_url"].rstrip("/")
        self.parallel = int(cfg.get("parallel", 4))
        self.json_mode = cfg.get("json_mode", "schema")  # schema | object | none; steps down on a 400
        self.price = (cfg.get("usd_per_mtok_in"), cfg.get("usd_per_mtok_out"))

    def headers(self) -> dict:
        h = {"Content-Type": "application/json", **self.cfg.get("headers", {})}
        key = os.environ.get(self.cfg["key_env"]) if self.cfg.get("key_env") else None
        if key:
            h["Authorization"] = f"Bearer {key}"
        return h

    def body(self, system: str, user: list[str], image: bytes, schema: dict, mode: str) -> dict:
        shared = {"type": "text", "text": user[0]}
        if self.cache_marks():
            shared["cache_control"] = {"type": "ephemeral"}
        b = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": [
                    shared,
                    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64," + jpeg_b64(image)}},
                    *({"type": "text", "text": t} for t in user[1:]),
                ]},
            ],
            "temperature": self.cfg.get("temperature", 0),
            "max_tokens": int(self.cfg.get("max_tokens", 4000)),
        }
        if mode == "schema":
            b["response_format"] = {"type": "json_schema", "json_schema": {"name": "answer", "schema": schema}}
        elif mode == "object":
            b["response_format"] = {"type": "json_object"}
        if "openrouter.ai" in self.base_url:
            b["usage"] = {"include": True}  # OpenRouter then reports the cost
        return b

    def cache_marks(self) -> bool:
        """OpenRouter caches Claude and Gemini prompts only where the request marks them; others cache on their own."""
        return "openrouter.ai" in self.base_url and self.model.startswith(("anthropic/", "google/"))

    def ask(self, system: str, user: list[str], image: bytes, schema: dict) -> Reply:
        modes = ["schema", "object", "none"]
        for mode in modes[modes.index(self.json_mode):]:
            req = urllib.request.Request(self.base_url + "/chat/completions", method="POST", headers=self.headers(),
                                         data=json.dumps(self.body(system, user, image, schema, mode)).encode())
            try:
                with urllib.request.urlopen(req, timeout=float(self.cfg.get("timeout", 300))) as resp:
                    data = json.loads(resp.read())
            except urllib.error.HTTPError as err:
                detail = err.read().decode(errors="replace")[:500]
                if err.code == 400 and mode != "none" and ("response_format" in detail or "json" in detail.lower()):
                    self.json_mode = modes[modes.index(mode) + 1]  # this server cannot constrain; remember it
                    continue
                raise BackendError(f"HTTP {err.code}: {detail}") from err
            except (urllib.error.URLError, TimeoutError) as err:
                raise BackendError(str(err)) from err
            choice = (data.get("choices") or [{}])[0]
            usage = data.get("usage") or {}
            tin, tout = usage.get("prompt_tokens"), usage.get("completion_tokens")
            usd = usage.get("cost")
            if usd is None and self.price[0] is not None and tin is not None:
                usd = (tin * self.price[0] + (tout or 0) * (self.price[1] or 0)) / 1e6
            return Reply(text=(choice.get("message") or {}).get("content") or "", tokens_in=tin, tokens_out=tout,
                         usd=usd if usd is not None else (0.0 if self.is_local() else None),
                         model=data.get("model"), raw=data)
        raise BackendError("no answer")

    def is_local(self) -> bool:
        return any(h in self.base_url for h in ("localhost", "127.0.0.1", "0.0.0.0")) or bool(self.cfg.get("local"))


class AnthropicAPI:
    """The Claude API through the official SDK; structured outputs keep answers to the schema."""
    kind = "anthropic"

    def __init__(self, cfg: dict):
        try:
            import anthropic
        except ImportError as err:
            raise SystemExit("the Claude backend needs the SDK: pip install 'plantgen-imagekit[claude]'") from err
        self.cfg = cfg
        self.model = cfg["model"]
        self.revision = cfg.get("revision") or cfg["model"]
        self.parallel = int(cfg.get("parallel", 4))
        self.effort = cfg.get("effort", "low")
        key = os.environ.get(cfg.get("key_env", "ANTHROPIC_API_KEY"))
        self.client = anthropic.Anthropic(api_key=key) if key else anthropic.Anthropic()
        self.anthropic = anthropic
        self.use_format = True  # off after the API refuses a schema it cannot constrain to

    def params(self, system: str, user: list[str], image: bytes, schema: dict) -> dict:
        p = {
            "model": self.model,
            "max_tokens": int(self.cfg.get("max_tokens", 16000)),
            "system": [{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": user[0], "cache_control": {"type": "ephemeral"}},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": jpeg_b64(image)}},
                *({"type": "text", "text": t} for t in user[1:]),
            ]}],
            "output_config": {"effort": self.effort},
        }
        if self.use_format:
            p["output_config"]["format"] = {"type": "json_schema", "schema": schema}
        return p

    def usd(self, usage, batch: bool = False) -> float | None:
        price = CLAUDE_PRICES.get(self.model)
        if not price or usage is None:
            return None
        cached = getattr(usage, "cache_read_input_tokens", 0) or 0
        written = getattr(usage, "cache_creation_input_tokens", 0) or 0
        tin = usage.input_tokens + written * 1.25 + cached * 0.1
        return (tin * price[0] + usage.output_tokens * price[1]) / 1e6 * (0.5 if batch else 1.0)

    def reply(self, msg, batch: bool = False) -> Reply:
        if msg.stop_reason == "refusal":
            raise BackendError(f"refused ({getattr(msg.stop_details, 'category', None)})")
        text = next((b.text for b in msg.content if b.type == "text"), "")
        u = msg.usage
        return Reply(text=text, tokens_in=u.input_tokens + (u.cache_read_input_tokens or 0)
                     + (u.cache_creation_input_tokens or 0), tokens_out=u.output_tokens,
                     usd=self.usd(u, batch), model=msg.model, raw=json.loads(msg.model_dump_json()))

    def ask(self, system: str, user: list[str], image: bytes, schema: dict) -> Reply:
        try:
            msg = self.client.messages.create(**self.params(system, user, image, schema))
        except self.anthropic.BadRequestError as err:
            if self.use_format and "schema" in str(err).lower():
                self.use_format = False
                return self.ask(system, user, image, schema)
            raise BackendError(str(err)) from err
        except self.anthropic.APIError as err:
            raise BackendError(str(err)) from err
        return self.reply(msg)

    # Batch API: half price, answers within a day
    def submit(self, requests: list[tuple[str, dict]]) -> str:
        from anthropic.types.message_create_params import MessageCreateParamsNonStreaming
        from anthropic.types.messages.batch_create_params import Request
        batch = self.client.messages.batches.create(requests=[
            Request(custom_id=cid, params=MessageCreateParamsNonStreaming(**params)) for cid, params in requests])
        return batch.id

    def collect(self, batch_id: str):
        """None while the batch runs; else (custom_id, Reply or error text) pairs."""
        b = self.client.messages.batches.retrieve(batch_id)
        if b.processing_status != "ended":
            return None
        out = []
        for r in self.client.messages.batches.results(batch_id):
            if r.result.type == "succeeded":
                try:
                    out.append((r.custom_id, self.reply(r.result.message, batch=True)))
                except BackendError as err:
                    out.append((r.custom_id, str(err)))
            else:
                out.append((r.custom_id, r.result.type))
        return out


def load(name: str, overrides: dict | None = None, config: Path | None = None):
    cfg = dict(PRESETS.get(name, {}))
    path = Path(config or CONFIG)
    if path.exists():
        cfg.update(tomllib.loads(path.read_text()).get(name, {}))
    if name not in PRESETS and not cfg:
        raise SystemExit(f"no backend {name!r}: add it to {path} or use one of {', '.join(PRESETS)}")
    cfg.update({k: v for k, v in (overrides or {}).items() if v is not None})
    cfg["name"] = name
    kind = cfg.get("kind", "openai")
    if kind == "manual":
        return None
    if not cfg.get("model"):
        raise SystemExit(f"backend {name!r} needs a model (--model)")
    if kind == "openai" and not cfg.get("base_url"):
        raise SystemExit(f"backend {name!r} needs base_url (--base-url)")
    return {"openai": OpenAICompat, "anthropic": AnthropicAPI}[kind](cfg)

