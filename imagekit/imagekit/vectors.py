"""Image vectors: the embedding models, their text and image encoders, and the stored vectors.

A run's vectors live in its folder:

    sources/image-analyses/<run>/vectors/<model>/part-NNNN.npy        float16, L2-normalised, n x dim
    sources/image-analyses/<run>/vectors/<model>/part-NNNN.ids.jsonl  {"sha256", "region"} per row, same order
    sources/image-analyses/<run>/run.json                             models.<model>.parts: part -> analyzer id

`load(store, model)` gathers every part of one model across runs, memory-mapped.
The encoders here are shared by `imagekit embed` and `imagekit search`.

Nothing heavy is imported at module level (numpy, torch, transformers,
open_clip), so the command line stays usable with Pillow alone.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

from .store import Store, jsonl

# Presets. revision None means "resolve the current commit when embedding, and record it".
MODELS = {
    "siglip2": {"repo": "google/siglip2-so400m-patch14-384", "revision": "e8e487298228002f3d8a82e0cd5c8ea9c567f57f",
                "kind": "siglip", "text": True, "image_px": 384, "pooling": "siglip-head"},
    "siglip2-base": {"repo": "google/siglip2-base-patch16-224", "revision": None,
                     "kind": "siglip", "text": True, "image_px": 224, "pooling": "siglip-head"},
    "dinov2": {"repo": "facebook/dinov2-large", "revision": "47b73eefe95e8d44ec3623f8890bd894b6ea2d6c",
               "kind": "dino", "text": False, "image_px": 224, "pooling": "cls"},
    "dinov2-small": {"repo": "facebook/dinov2-small", "revision": None,
                     "kind": "dino", "text": False, "image_px": 224, "pooling": "cls"},
    "bioclip2": {"repo": "imageomics/bioclip-2", "revision": "2957b322090f9cb17ae72c71981c7218a28d81e0",
                 "kind": "openclip", "text": True, "image_px": 224, "pooling": "clip-proj"},
    # Test models: deterministic vectors from a few pixels, no torch. Not shown in --help.
    "fake": {"repo": "imagekit/fake-rgb", "revision": "fake-1", "kind": "fake-rgb", "text": True, "image_px": 4,
             "pooling": "mean", "test": True},
    "fake-gray": {"repo": "imagekit/fake-gray", "revision": "fake-1", "kind": "fake-gray", "text": False,
                  "image_px": 4, "pooling": "mean", "test": True},
}
TEXT_TEMPLATE = "a photo of {}."
# Siglip text towers were trained with this padding (64 tokens, padded to the maximum).
SIGLIP_TEXT_LEN = 64


def preset(name: str) -> dict:
    if name not in MODELS:
        raise SystemExit(f"unknown model {name!r}: one of {', '.join(public_models())}")
    return MODELS[name]


def public_models() -> list[str]:
    return [k for k, v in MODELS.items() if not v.get("test")]


def text_models() -> list[str]:
    return [k for k in public_models() if MODELS[k]["text"]]


def resolve_revision(name: str) -> str:
    """The pinned revision, or the current commit of the model's repository (needs the network once)."""
    m = preset(name)
    if m["revision"]:
        return m["revision"]
    from huggingface_hub import HfApi
    return HfApi().model_info(m["repo"]).sha


def definition(name: str, revision: str, image_px: int | None = None, regions: dict | None = None) -> dict:
    """The analyzer definition for one model's vectors: of whole images, or with `regions` (the detector, the
    organs kept, the triage that kept them, the margin and the lowest score) of organ crops."""
    m = preset(name)
    params = {"image_px": image_px or m["image_px"], "region_kind": "organ" if regions else "whole",
              "pooling": m["pooling"], "normalized": True}
    if regions:
        params["regions"] = regions
    return {"task": "embed", "model": m["repo"], "model_revision": revision, "params": params}


def pick_device(device: str = "auto") -> str:
    if device != "auto":
        return device
    try:
        import torch
    except ImportError:
        return "cpu"
    return "cuda" if torch.cuda.is_available() else "cpu"


def _normalise(v):
    import numpy as np
    v = np.asarray(v, dtype=np.float32)
    return v / np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-12)


# --- encoders ---------------------------------------------------------------------------------------------------

class Embedder:
    """images(list of RGB PIL images) and texts(list of str) -> float32 n x dim, rows L2-normalised."""

    text = False

    def __init__(self, name: str, revision: str, image_px: int, device: str):
        m = preset(name)
        self.name, self.repo, self.revision, self.image_px, self.device = name, m["repo"], revision, image_px, device
        self.pooling = m["pooling"]
        self.dim = None

    def images(self, ims):
        raise NotImplementedError

    def texts(self, texts):
        raise SystemExit(f"{self.name} has no text encoder; text search works with {', '.join(text_models())}. "
                         f"Use --image with {self.name}.")


class FakeRGB(Embedder):
    """Test model: the image shrunk to image_px x image_px RGB; text is the colour its colour words name."""

    text = True
    COLOURS = {"red": (230, 20, 20), "green": (20, 200, 40), "blue": (20, 40, 230), "white": (250, 250, 250),
               "yellow": (240, 220, 20), "black": (5, 5, 5)}

    def images(self, ims):
        import numpy as np
        rows = [np.asarray(im.convert("RGB").resize((self.image_px, self.image_px)), dtype=np.float32).ravel() / 255 - 0.5
                for im in ims]
        out = _normalise(np.stack(rows))
        self.dim = out.shape[1]
        return out

    def texts(self, texts):
        from PIL import Image
        ims = []
        for t in texts:
            cols = [c for w, c in self.COLOURS.items() if w in t.lower()] or [(128, 128, 128)]
            ims.append(Image.new("RGB", (self.image_px, self.image_px), tuple(sum(c[i] for c in cols) // len(cols)
                                                                              for i in range(3))))
        return self.images(ims)


class FakeGray(FakeRGB):
    """Test model: brightness only, image queries only."""

    text = False

    def images(self, ims):
        import numpy as np
        rows = [np.asarray(im.convert("L").resize((self.image_px, self.image_px)), dtype=np.float32).ravel() / 255 - 0.5
                for im in ims]
        out = _normalise(np.stack(rows))
        self.dim = out.shape[1]
        return out

    texts = Embedder.texts


class Siglip(Embedder):
    """SigLIP / SigLIP2 (fixed resolution): the attention-pooled image and text features, in one space."""

    text = True

    def __init__(self, *a):
        super().__init__(*a)
        import torch
        from transformers import AutoModel, AutoProcessor
        self.torch = torch
        self.model = AutoModel.from_pretrained(self.repo, revision=self.revision).to(self.device).eval()
        self.proc = AutoProcessor.from_pretrained(self.repo, revision=self.revision)
        native = self.model.config.vision_config.image_size
        self.interpolate = self.image_px != native
        if self.interpolate:
            self.proc.image_processor.size = {"height": self.image_px, "width": self.image_px}

    @staticmethod
    def _features(out):
        return out if hasattr(out, "dim") else out.pooler_output  # newer transformers return a model output

    def images(self, ims):
        with self.torch.inference_mode():
            x = self.proc(images=ims, return_tensors="pt").to(self.device)
            kw = {"interpolate_pos_encoding": True} if self.interpolate else {}
            v = self._features(self.model.get_image_features(pixel_values=x["pixel_values"], **kw))
        return _normalise(v.float().cpu().numpy())

    def texts(self, texts):
        with self.torch.inference_mode():
            x = self.proc(text=[t.lower() for t in texts], padding="max_length", max_length=SIGLIP_TEXT_LEN,
                          truncation=True, return_tensors="pt").to(self.device)
            v = self._features(self.model.get_text_features(input_ids=x["input_ids"]))
        return _normalise(v.float().cpu().numpy())


class Dino(Embedder):
    """DINOv2: the class token after the last layer norm. Form and texture only, no text."""

    def __init__(self, *a):
        super().__init__(*a)
        import torch
        from transformers import AutoImageProcessor, AutoModel
        self.torch = torch
        self.model = AutoModel.from_pretrained(self.repo, revision=self.revision).to(self.device).eval()
        self.proc = AutoImageProcessor.from_pretrained(self.repo, revision=self.revision)
        if self.image_px != 224:  # the processor resizes the short side to 256/224 of the crop, then crops
            self.proc.size = {"shortest_edge": round(self.image_px * 256 / 224)}
            self.proc.crop_size = {"height": self.image_px, "width": self.image_px}

    def images(self, ims):
        with self.torch.inference_mode():
            x = self.proc(images=ims, return_tensors="pt").to(self.device)
            v = self.model(pixel_values=x["pixel_values"]).pooler_output
        return _normalise(v.float().cpu().numpy())


class OpenClip(Embedder):
    """BioCLIP-2 through open_clip, from the pinned snapshot of its Hugging Face repository."""

    text = True

    def __init__(self, *a):
        super().__init__(*a)
        import open_clip
        import torch
        from huggingface_hub import snapshot_download
        self.torch = torch
        path = snapshot_download(self.repo, revision=self.revision)
        self.model, _, self.pre = open_clip.create_model_and_transforms(f"local-dir:{path}")
        self.model = self.model.to(self.device).eval()
        self.tok = open_clip.get_tokenizer(f"local-dir:{path}")
        native = self.model.visual.image_size
        native = native[0] if isinstance(native, (tuple, list)) else native
        if self.image_px != native:
            raise SystemExit(f"{self.name} runs at {native} px only")

    def images(self, ims):
        with self.torch.inference_mode():
            v = self.model.encode_image(self.torch.stack([self.pre(im) for im in ims]).to(self.device))
        return _normalise(v.float().cpu().numpy())

    def texts(self, texts):
        with self.torch.inference_mode():
            v = self.model.encode_text(self.tok(texts).to(self.device))
        return _normalise(v.float().cpu().numpy())


KINDS = {"siglip": Siglip, "dino": Dino, "openclip": OpenClip, "fake-rgb": FakeRGB, "fake-gray": FakeGray}


def load_embedder(name: str, revision: str, image_px: int | None = None, device: str = "auto") -> Embedder:
    m = preset(name)
    return KINDS[m["kind"]](name, revision, image_px or m["image_px"], pick_device(device))


def load_image(path, image_px: int | None = None, box=None):
    """An RGB image, decoded at a reduced size when that is plenty for image_px; box is fractions x0,y0,x1,y1."""
    from PIL import Image, ImageOps
    im = Image.open(path)
    if image_px and im.format == "JPEG" and box is None:
        im.draft("RGB", (image_px * 2, image_px * 2))
    im = ImageOps.exif_transpose(im).convert("RGB")
    if box is not None:
        x0, y0, x1, y1 = box
        w, h = im.size
        im = im.crop((round(x0 * w), round(y0 * h), round(x1 * w), round(y1 * h)))
    return im


# --- stored vectors -------------------------------------------------------------------------------------------

def vector_dir(store: Store, run: str, name: str) -> Path:
    return store.run_dir(run) / "vectors" / name


def read_run(store: Store, run: str) -> dict:
    p = store.run_dir(run) / "run.json"
    return json.loads(p.read_text()) if p.exists() else {}


def analyzers(store: Store) -> dict:
    return {r["id"]: r for r in jsonl(store.root / "analyzers.jsonl")}


class VectorSet:
    """One model's vectors across runs: ids[i] = (sha256, region) for row i; parts memory-mapped."""

    def __init__(self, name: str, analyzer: str, definition: dict, ids, parts, others: dict):
        self.name, self.analyzer, self.definition = name, analyzer, definition
        self.ids, self.parts, self.others = ids, parts, others  # parts: [(memmap, row index array or None)]
        self.pos = {k: i for i, k in enumerate(ids)}

    def __len__(self):
        return len(self.ids)

    @property
    def dim(self) -> int:
        return self.parts[0][0].shape[1] if self.parts else 0

    @property
    def matrix(self):
        """All rows as one float16 array (a copy)."""
        import numpy as np
        if not self.parts:
            return np.zeros((0, 0), dtype=np.float16)
        return np.concatenate([a if rows is None else a[rows] for a, rows in self.parts])

    def vector(self, sha: str, region: int = 0):
        """The stored vector of one image as float32, or None."""
        import numpy as np
        i = self.pos.get((sha, region))
        if i is None:
            return None
        for a, rows in self.parts:
            n = len(a) if rows is None else len(rows)
            if i < n:
                return np.asarray(a[i] if rows is None else a[rows[i]], dtype=np.float32)
            i -= n
        return None

    def scores(self, q, chunk: int = 65536):
        """Cosine of every row with q (an L2-normalised vector): exact search, a part at a time."""
        import numpy as np
        q = np.asarray(q, dtype=np.float32).ravel()
        out = []
        for a, rows in self.parts:
            src = a if rows is None else a[rows]
            for i in range(0, len(src), chunk):
                out.append(np.asarray(src[i:i + chunk], dtype=np.float32) @ q)
        return np.concatenate(out) if out else np.zeros(0, dtype=np.float32)


def load(store: Store, name: str, analyzer: str | None = None, region_kind: str = "whole") -> VectorSet:
    """Every stored vector of one model. With several analyzers for the model (another revision or
    image size), the one with the most vectors of this region kind (whole images, or organ crops) is
    used unless `analyzer` names one; vectors from different analyzers are never mixed. A
    (sha256, region) seen twice keeps its first row."""
    import numpy as np
    defs = analyzers(store)
    kind_of = lambda a: ((defs.get(a) or {}).get("params") or {}).get("region_kind", "whole")  # noqa: E731
    found = []  # (run, part path, analyzer)
    for run in store.runs():
        info = (read_run(store, run).get("models") or {}).get(name) or {}
        for part, aid in sorted((info.get("parts") or {}).items()):
            p = vector_dir(store, run, name) / part
            if p.exists() and (analyzer is not None or kind_of(aid) == region_kind):
                found.append((run, p, aid))
    counts: dict[str, int] = {}
    for _, p, aid in found:
        counts[aid] = counts.get(aid, 0) + sum(1 for _ in open(p.with_name(p.name[:-4] + ".ids.jsonl")))
    if analyzer is None and counts:
        analyzer = max(counts, key=lambda a: counts[a])
    ids, parts, seen = [], [], set()
    for _, p, aid in found:
        if aid != analyzer:
            continue
        arr = np.load(p, mmap_mode="r")
        part_ids = [(r["sha256"], r.get("region", 0)) for r in jsonl(p.with_name(p.name[:-4] + ".ids.jsonl"))]
        if len(part_ids) != len(arr):
            print(f"  skipped {p}: {len(arr)} vectors but {len(part_ids)} ids", file=sys.stderr)
            continue
        keep = [i for i, k in enumerate(part_ids) if k not in seen]
        seen.update(part_ids)
        if not keep:
            continue
        ids.extend(part_ids[i] for i in keep)
        parts.append((arr, None if len(keep) == len(part_ids) else np.array(keep)))
    defn = defs.get(analyzer, {}) if analyzer else {}
    return VectorSet(name, analyzer, defn, ids, parts, {a: n for a, n in counts.items() if a != analyzer})
