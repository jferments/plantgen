"""Polite HTTP: one User-Agent, a minimum gap per host, backoff on 429 and 5xx."""
from __future__ import annotations

import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from . import __version__

UA = f"plantgen-imagekit/{__version__} (+https://github.com/jferments/plantgen)"

# Seconds between requests to one host. iNaturalist asks for at most ~60
# API requests a minute; Commons answers 429 quickly.
GAP = {
    "api.inaturalist.org": 1.0,
    "commons.wikimedia.org": 2.0,
    "upload.wikimedia.org": 1.0,
    "api.gbif.org": 0.5,
    "archive.org": 1.0,
}
DEFAULT_GAP = 0.2
_last: dict[str, float] = {}


def _wait(host: str) -> None:
    gap = GAP.get(host, DEFAULT_GAP)
    since = time.monotonic() - _last.get(host, 0.0)
    if since < gap:
        time.sleep(gap - since)
    _last[host] = time.monotonic()


def get_bytes(url: str, *, tries: int = 5, headers: dict | None = None) -> bytes:
    host = urllib.parse.urlparse(url).hostname or ""
    delay = 2.0
    for attempt in range(tries):
        _wait(host)
        req = urllib.request.Request(url, headers={"User-Agent": UA, **(headers or {})})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return resp.read()
        except urllib.error.HTTPError as err:
            if err.code in (429, 500, 502, 503, 504) and attempt + 1 < tries:
                wait = float(err.headers.get("Retry-After") or delay)
                print(f"  {host}: {err.code}, waiting {wait:.0f} s", file=sys.stderr)
                time.sleep(wait)
                delay *= 2
                continue
            raise
        except (urllib.error.URLError, TimeoutError):
            if attempt + 1 < tries:
                time.sleep(delay)
                delay *= 2
                continue
            raise
    raise RuntimeError("unreachable")


def get_json(url: str, params: dict | None = None) -> dict:
    if params:
        url += ("&" if "?" in url else "?") + urllib.parse.urlencode(params)
    return json.loads(get_bytes(url))
