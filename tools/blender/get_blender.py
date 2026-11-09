#!/usr/bin/env python3
"""Fetch a pinned portable Blender into a cache folder, once, and say where it is.

    python3 tools/blender/get_blender.py --cache DIR [--libs]

prints `BLENDER=<path to the blender binary>` and, with --libs,
`LD_LIBRARY_PATH=<folders>`: the X11 and GL libraries a portable Blender
links to, taken from the machine's own apt sources with `apt-get download`
and unpacked into DIR/libs, so a runner with no desktop and no root can
start it. Both stay in DIR, so only the first run downloads (about 360 MB).

The archive comes from download.blender.org (or its mirrors) and is checked
against the SHA-256 pinned here before it is unpacked. The same approach,
version and pin run Project After's AssetLab Blender contracts on the same
CI machine.
"""
from __future__ import annotations

import argparse
import hashlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

VERSION = "4.5.14"
# Blender LTS linux-x64 official archives, checked by hand against
# download.blender.org's own .sha256 files.
PINNED_SHA256 = {
    "4.5.14": "9ba871ff2ecd36526b77432745980b7e6664ecd0c7ca11c48849073dcfe06da3",
}
USER_AGENT = "Mozilla/5.0 PlantGen-CI"

# What a portable Blender links to beyond a server's base system.
LIB_PACKAGES = (
    "libx11-6", "libxext6", "libxrender1", "libxi6", "libxfixes3",
    "libxxf86vm1", "libxinerama1", "libxcursor1", "libxrandr2",
    "libxkbcommon0", "libsm6", "libice6", "libxcb1", "libxau6", "libxdmcp6",
    "libgl1", "libegl1", "libglvnd0", "libglx0", "libwayland-client0",
    "libdbus-1-3", "libbsd0", "libmd0",
)


def download(urls: list[str], to: Path) -> str:
    errors = []
    for url in urls:
        part = to.with_suffix(to.suffix + ".part")
        part.unlink(missing_ok=True)
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=120) as r, part.open("wb") as f:
                shutil.copyfileobj(r, f, 8 << 20)
            if part.stat().st_size == 0:
                raise RuntimeError("empty download")
            part.replace(to)
            return url
        except Exception as exc:  # try the next mirror
            errors.append(f"{url}: {exc}")
    raise RuntimeError("every Blender download failed:\n" + "\n".join(errors))


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(8 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def blender(cache: Path, version: str = VERSION) -> Path:
    """The pinned Blender's binary, fetched and unpacked into cache first if missing."""
    dest = cache / f"blender-{version}"
    binary = dest / "blender"
    if binary.is_file():
        return binary
    pin = PINNED_SHA256.get(version)
    if pin is None:
        raise RuntimeError(f"Blender {version} has no SHA-256 pin in get_blender.py")
    series = ".".join(version.split(".")[:2])
    name = f"blender-{version}-linux-x64.tar.xz"
    roots = [
        f"https://download.blender.org/release/Blender{series}/",
        f"https://mirror.blender.org/release/Blender{series}/",
    ]
    cache.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="blender-", dir=cache) as tmp:
        tmp = Path(tmp)
        archive = tmp / name
        source = download([r + name for r in roots], archive)
        got = sha256(archive)
        if got != pin:
            raise RuntimeError(f"{source}: SHA-256 {got}, pinned {pin}; not unpacked")
        with tarfile.open(archive, "r:xz") as tar:
            tar.extractall(tmp / "x", filter="data")
        (top,) = [p for p in (tmp / "x").iterdir() if p.is_dir()]
        if not (top / "blender").is_file():
            raise RuntimeError(f"{name}: no blender binary at its top")
        shutil.rmtree(dest, ignore_errors=True)
        top.rename(dest)
    return binary


def libs(cache: Path) -> list[Path]:
    """Folders holding the libraries in LIB_PACKAGES, unpacked without root."""
    root = cache / "libs"
    stamp = root / ".ready"
    try:
        arch = subprocess.check_output(["dpkg-architecture", "-qDEB_HOST_MULTIARCH"], text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        arch = "x86_64-linux-gnu"
    dirs = [root / "usr/lib" / arch, root / "lib" / arch, root / "usr/lib", root / "lib"]
    if stamp.is_file() and stamp.read_text() == "\n".join(LIB_PACKAGES):
        return [d for d in dirs if d.is_dir()]
    for tool in ("apt-get", "dpkg-deb"):
        if shutil.which(tool) is None:
            raise RuntimeError(f"--libs needs {tool}")
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    with tempfile.TemporaryDirectory(prefix="blender-libs-") as tmp:
        for pkg in LIB_PACKAGES:
            r = subprocess.run(["apt-get", "download", pkg], cwd=tmp, capture_output=True, text=True)
            if r.returncode != 0:
                raise RuntimeError(f"apt-get download {pkg}: {r.stderr.strip()}")
        for deb in sorted(Path(tmp).glob("*.deb")):
            subprocess.run(["dpkg-deb", "-x", str(deb), str(root)], check=True)
    stamp.write_text("\n".join(LIB_PACKAGES))
    return [d for d in dirs if d.is_dir()]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cache", type=Path, required=True, help="folder kept between runs")
    ap.add_argument("--version", default=VERSION)
    ap.add_argument("--libs", action="store_true", help="also unpack X11/GL libraries without root")
    a = ap.parse_args()
    cache = a.cache.expanduser().resolve()
    print(f"BLENDER={blender(cache, a.version)}")
    if a.libs:
        print("LD_LIBRARY_PATH=" + ":".join(str(d) for d in libs(cache)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
