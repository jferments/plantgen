"""Check the photo rules against the library's vocabulary and their own schemas.

Run from the repository root:  python3 photo-rules/tools/check.py
Needs PyYAML and jsonschema (pip install pyyaml jsonschema).

- every feature id is unique and every `trait` or `related_trait` it names
  is a character in crates/plantgen/traits.json;
- a `state` feature mapped one for one to an enum trait uses only that
  trait's states, or lists the extra ones under `extra_states`;
- every example JSON file validates against its schema.
"""
import json
import pathlib
import sys

import jsonschema
import yaml

ROOT = pathlib.Path(__file__).resolve().parents[2]
RULES = ROOT / "photo-rules"
EXAMPLE_SCHEMAS = {
    "triage": "triage",
    "observation": "observation",
    "check": "check",
    "finding": "finding",
    "proposal": "proposal",
}


def names(value):
    if value is None:
        return []
    return value if isinstance(value, list) else [value]


def main():
    traits = json.loads((ROOT / "crates/plantgen/traits.json").read_text())["traits"]
    features = yaml.safe_load((RULES / "features.yaml").read_text())["features"]
    problems = []
    seen = set()
    for feature in features:
        fid = feature["id"]
        if fid in seen:
            problems.append(f"{fid}: id used twice")
        seen.add(fid)
        mapping = feature.get("maps_to", {})
        for key in ("trait", "related_trait"):
            for name in names(mapping.get(key)):
                if name not in traits:
                    problems.append(f"{fid}: {key} `{name}` is not in traits.json")
        trait = mapping.get("trait")
        if feature["type"] == "state" and isinstance(trait, str) and trait in traits:
            allowed = set(traits[trait].get("values") or [])
            extra = set(mapping.get("extra_states", []))
            if allowed:
                for state in feature.get("states", {}):
                    if state not in allowed and state not in extra:
                        problems.append(f"{fid}: state `{state}` is not a state of `{trait}`")
    for path in sorted((RULES / "examples").rglob("*.json")):
        kind = next((k for k in EXAMPLE_SCHEMAS if path.stem.startswith(k)), None)
        if kind is None:
            problems.append(f"{path.relative_to(ROOT)}: no schema matches its name")
            continue
        schema = json.loads((RULES / "schemas" / f"{EXAMPLE_SCHEMAS[kind]}.schema.json").read_text())
        try:
            jsonschema.validate(json.loads(path.read_text()), schema)
        except jsonschema.ValidationError as error:
            problems.append(f"{path.relative_to(ROOT)}: {error.message}")
    for problem in problems:
        print(problem)
    print(f"{len(features)} features, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
