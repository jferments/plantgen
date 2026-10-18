"""Licence codes: one spelling for the many each source uses."""
from __future__ import annotations

# Fetched unless --licences says otherwise. Non-commercial licences
# (cc-by-nc, cc-by-nc-sa) are opt-in: keep only numbers measured from them.
DEFAULT = ("cc0", "cc-by", "cc-by-sa", "pd")
KNOWN = ("cc0", "pd", "cc-by", "cc-by-sa", "cc-by-nc", "cc-by-nc-sa", "cc-by-nd", "cc-by-nc-nd")

LABEL = {
    "cc0": "CC0", "pd": "public domain", "cc-by": "CC BY", "cc-by-sa": "CC BY-SA",
    "cc-by-nc": "CC BY-NC", "cc-by-nc-sa": "CC BY-NC-SA", "cc-by-nd": "CC BY-ND",
    "cc-by-nc-nd": "CC BY-NC-ND",
}


def norm(text: str | None) -> str:
    """'CC-BY-NC', 'cc_by_nc', 'http://creativecommons.org/licenses/by-nc/4.0/' -> 'cc-by-nc'; unknown -> ''."""
    if not text:
        return ""
    t = text.lower().strip()
    if "publicdomain/zero" in t or t in ("cc0", "cc-0", "cc0 1.0", "cc0-1.0"):
        return "cc0"
    if "publicdomain/mark" in t or t in ("pd", "pdm", "public domain") or t.startswith("pd-"):
        return "pd"
    if "creativecommons.org/licenses/" in t:
        t = "cc-" + t.split("/licenses/")[1].split("/")[0]
    t = t.replace(" ", "-").replace("_", "-")
    if t.startswith("cc-by") or t.startswith("by"):
        t = t if t.startswith("cc-") else "cc-" + t
        for tag in ("cc-by-nc-sa", "cc-by-nc-nd", "cc-by-nc", "cc-by-nd", "cc-by-sa", "cc-by"):
            if t.startswith(tag):
                return tag
    return ""


def parse(arg: str) -> set[str]:
    out = {norm(x) for x in arg.split(",") if x.strip()}
    bad = [x for x in arg.split(",") if x.strip() and not norm(x)]
    if bad:
        raise SystemExit(f"unknown licence: {', '.join(bad)} (known: {', '.join(KNOWN)})")
    return out


def attribution(creator: str | None, licence: str) -> str:
    who = creator or "unknown creator"
    return f"{who}, {LABEL.get(licence, licence)}"
