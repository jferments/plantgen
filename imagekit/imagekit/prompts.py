"""Requests built from photo-rules/, the same for every model.

As photo-rules/README.md says for an API: the system text is
00-ground-rules.md; the user text is the task file (plus, for observing,
palette.yaml and the features of the image's views) and the answer's JSON
Schema, then one image and its opaque id. Everything before the image is
the same for every image of a task, so servers that cache prompts charge
it once. The species is never named.
"""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path

import yaml



def find_rules() -> Path:
    """$IMAGEKIT_RULES, else the photo-rules/ beside this checkout, else one above the current folder."""
    if os.environ.get("IMAGEKIT_RULES"):
        return Path(os.environ["IMAGEKIT_RULES"])
    here = Path(__file__).resolve().parents[2] / "photo-rules"  # running from the plantgen checkout
    if here.exists():
        return here
    for d in (Path.cwd(), *Path.cwd().parents):  # installed as a tool: run it inside the checkout
        if (d / "photo-rules" / "00-ground-rules.md").exists():
            return d / "photo-rules"
    return here

TASKS = {
    "triage": ("01-triage.md", "triage"),
    "observe": ("02-observe.md", "observation"),
    "describe": ("08-describe.md", "description"),
}


class Rules:
    def __init__(self, path: str | Path | None = None):
        self.path = Path(path or find_rules()).expanduser()
        if not (self.path / "00-ground-rules.md").exists():
            raise SystemExit(f"no photo rules at {self.path}: pass --rules PATH or set IMAGEKIT_RULES "
                             "(the photo-rules folder of a plantgen checkout)")
        self.features = yaml.safe_load((self.path / "features.yaml").read_text())["features"]
        files = sorted(p for p in self.path.rglob("*") if p.is_file() and p.suffix in (".md", ".yaml", ".json"))
        h = hashlib.sha256()
        for p in files:
            h.update(p.relative_to(self.path).as_posix().encode() + b"\0" + p.read_bytes())
        self.version = h.hexdigest()[:16]  # changes whenever any rule file does
        try:
            self.git = subprocess.run(["git", "-C", str(self.path), "log", "-1", "--format=%H", "--", "."],
                                      capture_output=True, text=True, timeout=10).stdout.strip() or None
        except (OSError, subprocess.SubprocessError):
            self.git = None

    def read(self, name: str) -> str:
        return (self.path / name).read_text()

    def schema(self, task: str) -> dict:
        return json.loads(self.read(f"schemas/{TASKS[task][1]}.schema.json"))

    def system(self) -> str:
        return self.read("00-ground-rules.md")

    def features_for(self, views: list[str]) -> list[dict]:
        return [f for f in self.features if set(f.get("views") or []) & set(views)]

    def shared(self, task: str, views: list[str] | None = None) -> str:
        """The user text every image of a task (and view set) shares; it comes first so servers can cache it."""
        parts = [self.read(TASKS[task][0])]
        if task == "observe":
            parts.append("## palette.yaml\n\n" + self.read("palette.yaml"))
            feats = self.features_for(views or [])
            parts.append(f"## The features to answer (views {', '.join(views or [])})\n\n"
                         + yaml.safe_dump({"features": feats}, sort_keys=False, allow_unicode=True))
        if task == "triage":
            parts.append('You are not told the species: set "species_match" to "unsure". Leave "kind" out.')
        parts.append("Answer with exactly one JSON object for the image, valid against this JSON Schema, "
                     "and nothing else:\n\n" + json.dumps(self.schema(task), ensure_ascii=False))
        return "\n\n".join(parts)

    def parts(self, task: str, photo_id: str, views: list[str] | None = None) -> list[str]:
        """[shared text, per-image text]: the request's user text, sent around the image."""
        return [self.shared(task, views), f"Photo id: {photo_id}"]

    def user(self, task: str, photo_id: str, views: list[str] | None = None) -> str:
        return "\n\n".join(self.parts(task, photo_id, views))

    def template_hash(self, task: str) -> str:
        """The prompt without the image, its id or the per-image feature list: one per task and rules version."""
        text = self.system() + "\0" + self.user(task, "<photo id>", ["<views>"] if task == "observe" else None)
        return hashlib.sha256(text.encode()).hexdigest()[:16]


def first_json(text: str):
    """The first JSON object in a model's answer (code fences and chatter around it allowed)."""
    dec = json.JSONDecoder()
    i = text.find("{")
    while i != -1:
        try:
            obj, _ = dec.raw_decode(text, i)
            if isinstance(obj, dict):
                return obj
        except json.JSONDecodeError:
            pass
        i = text.find("{", i + 1)
    raise ValueError("no JSON object in the answer")


def problems(obj: dict, schema: dict) -> list[str]:
    """Schema errors, with jsonschema if installed, else missing required keys only."""
    try:
        import jsonschema
    except ImportError:
        return [f"missing {k}" for k in schema.get("required", []) if k not in obj]
    v = jsonschema.Draft202012Validator(schema)
    return [f"{'/'.join(map(str, e.absolute_path)) or '(top)'}: {e.message}" for e in v.iter_errors(obj)][:10]
