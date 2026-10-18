"""Check the photo rules against the library's vocabulary and their own schemas.

Run from the repository root:  python3 photo-rules/tools/check.py
Needs PyYAML and jsonschema (pip install pyyaml jsonschema).

- every feature id is unique, its type is one the rules know, and every
  `trait` or `related_trait` it names is a character in
  crates/plantgen/traits.json;
- a `state`, `states` or `ordinal` feature mapped one for one to an enum
  trait uses only that trait's states, or lists the extra ones under
  `extra_states`;
- a `profile` feature lists its `bands`, rising heights between 0 and 1;
- every example JSON file validates against its schema, and every answer
  in an example observation names a feature and fits its type.
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
TYPES = {"state", "states", "ordinal", "ratio", "share", "angle", "count", "colour", "profile"}
NOT_ANSWERED = {"cannot tell", "not applicable"}
BINS = {"16-30", "31-60", "61-120", ">120"}


def names(value):
    if value is None:
        return []
    return value if isinstance(value, list) else [value]


def check_feature(feature, traits):
    fid, kind = feature["id"], feature.get("type")
    problems = []
    if kind not in TYPES:
        problems.append(f"{fid}: unknown type `{kind}`")
    mapping = feature.get("maps_to", {})
    for key in ("trait", "related_trait"):
        for name in names(mapping.get(key)):
            if name not in traits:
                problems.append(f"{fid}: {key} `{name}` is not in traits.json")
    trait = mapping.get("trait")
    if kind in ("state", "states", "ordinal") and isinstance(trait, str) and trait in traits:
        allowed = set(traits[trait].get("values") or [])
        extra = set(mapping.get("extra_states", []))
        if allowed:
            for state in feature.get("states", {}):
                if state not in allowed and state not in extra:
                    problems.append(f"{fid}: state `{state}` is not a state of `{trait}`")
    if not isinstance(feature.get("face_on", False), bool):
        problems.append(f"{fid}: face_on is true or left out")
    if kind == "profile":
        bands = feature.get("bands") or []
        if not bands or any(not 0 < b < 1 for b in bands) or bands != sorted(set(bands)):
            problems.append(f"{fid}: a profile needs `bands`, rising heights between 0 and 1")
    elif "bands" in feature:
        problems.append(f"{fid}: only a profile has `bands`")
    return problems


def check_answer(where, feature, answer):
    """Does one example answer fit its feature's type?"""
    value, kind = answer["value"], feature["type"]
    if isinstance(value, str) and value in NOT_ANSWERED:
        return []
    ok = {
        "state": lambda v: v in feature.get("states", {}),
        "ordinal": lambda v: v in feature.get("states", {}),
        "states": lambda v: isinstance(v, list) and all(s in feature.get("states", {}) for s in v),
        "ratio": lambda v: isinstance(v, (int, float)) and v >= 0,
        "share": lambda v: isinstance(v, (int, float)) and 0 <= v <= 1,
        "angle": lambda v: isinstance(v, (int, float)) and 0 <= v <= 180,
        "count": lambda v: (isinstance(v, int) and v >= 0) or (isinstance(v, str) and v in BINS)
        or (isinstance(v, dict) and "count" in v),
        "colour": lambda v: isinstance(v, dict) and "hex" in v,
        "profile": lambda v: isinstance(v, list) and len(v) == len(feature["bands"])
        and all(isinstance(x, (int, float)) and 0 <= x <= 2 for x in v),
    }[kind](value)
    problems = [] if ok else [f"{where}: `{feature['id']}` answer {value!r} does not fit type {kind}"]
    for band in answer.get("estimated", []):
        if kind != "profile" or band >= len(feature["bands"]):
            problems.append(f"{where}: `{feature['id']}` estimated band {band} is not one of its bands")
    return problems


def main():
    traits = json.loads((ROOT / "crates/plantgen/traits.json").read_text())["traits"]
    features = yaml.safe_load((RULES / "features.yaml").read_text())["features"]
    problems = []
    by_id = {}
    for feature in features:
        if feature["id"] in by_id:
            problems.append(f"{feature['id']}: id used twice")
        by_id[feature["id"]] = feature
        problems += check_feature(feature, traits)
    for path in sorted((RULES / "examples").rglob("*.json")):
        where = path.relative_to(ROOT)
        kind = next((k for k in EXAMPLE_SCHEMAS if path.stem.startswith(k)), None)
        if kind is None:
            problems.append(f"{where}: no schema matches its name")
            continue
        schema = json.loads((RULES / "schemas" / f"{EXAMPLE_SCHEMAS[kind]}.schema.json").read_text())
        example = json.loads(path.read_text())
        try:
            jsonschema.validate(example, schema)
        except jsonschema.ValidationError as error:
            problems.append(f"{where}: {error.message}")
            continue
        if kind == "observation":
            for fid, answer in example["answers"].items():
                if fid not in by_id:
                    problems.append(f"{where}: `{fid}` is not a feature")
                else:
                    problems += check_answer(where, by_id[fid], answer)
    for problem in problems:
        print(problem)
    print(f"{len(features)} features, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
