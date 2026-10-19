"""`imagekit plan` and `imagekit run`: one photo-rules task over the images that lack it.

    imagekit plan triage --backend workstation
    imagekit run triage --backend workstation
    imagekit run observe --backend openrouter --model qwen/qwen3-vl-235b-a22b-instruct --max-usd 3
    imagekit run describe --backend claude --claude-batch --yes      # then: imagekit run --collect RUN --backend claude
    imagekit run describe --backend chat --model "my chat app" --chat-dir ~/chat-describe

Before anything is sent, the run drops every image that already has an
answer under the same analyzer (task, model and revision, rules version,
prompt, settings): no image is analysed, or paid for, twice.
"""
from __future__ import annotations

import gzip
import io
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from urllib.parse import urlparse

from PIL import Image

from . import analyzer, backends
from .prompts import TASKS, Rules, first_json, problems
from .store import Store, append_jsonl, check_open, jsonl, now

IMAGE_PX = {"triage": 512, "observe": 1536, "describe": 1024}
TOKENS_OUT = {"triage": 400, "observe": 1500, "describe": 500}  # answer sizes measured on the replay set, plus margin
ASK_OVER_USD = 1.0


def register(sub) -> None:
    for name, helptext in (("plan", "how many images a run would send, and roughly what it would cost"),
                           ("run", "run one photo-rules task on the images that lack it")):
        p = sub.add_parser(name, help=helptext)
        p.add_argument("task", nargs="?", choices=sorted(TASKS))
        p.add_argument("--backend", required=True, help="a name from backends.toml, or openrouter, vllm, ollama, claude, chat")
        p.add_argument("--backends", help="backends.toml to read (default ~/.config/imagekit/backends.toml)")
        p.add_argument("--model")
        p.add_argument("--base-url")
        p.add_argument("--parallel", type=int)
        p.add_argument("--rules", help="the photo-rules folder (default $IMAGEKIT_RULES, or the plantgen checkout you are in)")
        p.add_argument("--batch", nargs="*", help="only images from these fetch batches")
        p.add_argument("--taxon", help="only images of this taxon (name as fetched, or accepted name)")
        p.add_argument("--limit", type=int)
        p.add_argument("--image-px", type=int, help="long side sent to the model (default: triage 512, observe 1536, describe 1024)")
        p.add_argument("--triage", help="observe: the triage analyzer whose views to use (default: the one with most answers)")
        p.add_argument("--retry-invalid", action="store_true", help="send again the images whose answer failed the schema")
        p.add_argument("--claude-batch", action="store_true", help="send through the Message Batches API (half price)")
        p.set_defaults(func=cmd_plan if name == "plan" else cmd_run)
        if name == "run":
            p.add_argument("--run", help="run name (default <task>-<backend>-<date>)")
            p.add_argument("--max-usd", type=float, default=5.0, help="stop once this much is spent (default 5)")
            p.add_argument("--yes", action="store_true", help=f"needed when the estimate is over ${ASK_OVER_USD:.0f}")
            p.add_argument("--collect", metavar="RUN", help="collect the finished Claude batches of a run")
            p.add_argument("--chat-dir", help="chat backend: where prompts are written and answers read back")


# selection

def image_rows(store: Store, batches=None) -> dict:
    """sha256 -> its first images.jsonl row (provenance and taxon)."""
    out = {}
    for b in batches or store.batches():
        for r in jsonl(store.batch_dir(b) / "images.jsonl"):
            out.setdefault(r["sha256"], r)
    return out


def outputs(store: Store, aid: str) -> dict:
    """sha256 -> the ok answer under an analyzer, read from the run files."""
    runs = [r for (r,) in store.db.execute("select distinct run from analyses where analyzer=?", (aid,))]
    out = {}
    for run in runs:
        for row in jsonl(store.run_dir(run) / "analyses.jsonl"):
            if row["analyzer"] == aid and row.get("status") == "ok" and row.get("region", 0) == 0:
                out[row["sha256"]] = row["out"]
    return out


def default_triage(store: Store, shas=None) -> str | None:
    """The triage analyzer with most answers: among shas when given (the images in scope), else overall."""
    rows = store.db.execute("select analyzer, sha256 from analyses where task='triage' and status='ok' and region=0")
    keep = set(shas) if shas is not None else None
    n: dict[str, int] = {}
    for aid, sha in rows:
        if keep is None or sha in keep:
            n[aid] = n.get(aid, 0) + 1
    return max(sorted(n), key=n.get) if n else None


class Job:
    """What a plan or run needs: the backend, the analyzer, and the images to send."""

    def __init__(self, args, store: Store):
        self.args, self.store = args, store
        self.task = args.task
        self.backend = backends.load(args.backend, {"model": args.model, "base_url": args.base_url,
                                                    "parallel": args.parallel}, args.backends)
        self.rules = Rules(args.rules)
        self.px = args.image_px or IMAGE_PX[self.task]
        self.schema = self.rules.schema(self.task)
        rows = image_rows(store, args.batch)
        if args.taxon:
            rows = {s: r for s, r in rows.items()
                    if args.taxon in (r["taxon"].get("name_as_given"), r["taxon"].get("accepted_name"))}
        self.rows = rows
        self.views = {}
        params = {"image_px": self.px}
        if self.task == "observe":
            tri = args.triage or default_triage(store, rows)
            if not tri:
                raise SystemExit("observe needs a triage first: imagekit run triage --backend ...")
            params["triage"] = tri
            for sha, out in outputs(store, tri).items():
                if out.get("usable") and out.get("views"):
                    self.views[sha] = out["views"]
            rows = {s: r for s, r in rows.items() if s in self.views}
        if self.backend is None:  # chat
            model, revision, served = f"chat:{args.model or 'unnamed'}", args.model or "unnamed", "chat"
        else:
            model, revision = self.backend.model, self.backend.revision
            served = "anthropic" if self.backend.kind == "anthropic" else urlparse(self.backend.base_url).hostname
            if self.backend.kind == "anthropic":
                params["effort"] = self.backend.effort
            else:
                params["temperature"] = self.backend.cfg.get("temperature", 0)
        params["served_by"] = served
        self.definition = {"task": self.task, "model": model, "model_revision": revision,
                           "rules_version": self.rules.version, "prompt_hash": self.rules.template_hash(self.task),
                           "params": params}
        self.aid = analyzer.analyzer_id(self.definition)
        redo = ("invalid", "refused") if args.retry_invalid else ()
        self.todo = store.missing(self.aid, sorted(rows), 0, redo)
        if args.limit:
            self.todo = self.todo[: args.limit]
        self.held = len(rows) - len(store.missing(self.aid, sorted(rows), 0))

    def photo_id(self, sha: str) -> str:
        return "img-" + sha[:10]

    def image(self, sha: str) -> bytes:
        im = Image.open(self.store.blob(sha))
        im.thumbnail((self.px, self.px), Image.Resampling.LANCZOS)
        out = io.BytesIO()
        im.convert("RGB").save(out, "JPEG", quality=90)
        return out.getvalue()

    def parts(self, sha: str) -> list[str]:
        return self.rules.parts(self.task, self.photo_id(sha), self.views.get(sha))

    def user(self, sha: str) -> str:
        return "\n\n".join(self.parts(sha))

    def per_image_usd(self) -> tuple[float | None, str]:
        hist = [r for run in self.store.runs() for r in jsonl(self.store.run_dir(run) / "analyses.jsonl")
                if r["analyzer"] == self.aid and r.get("usd") is not None]
        if hist:
            return sum(r["usd"] for r in hist) / len(hist), f"mean of {len(hist)} earlier answers"
        b = self.backend
        if b is None or (b.kind == "openai" and b.is_local()):
            return 0.0, "local or chat: no API charge"
        tokens_in = (len(self.rules.system()) + len(self.user(self.todo[0]) if self.todo else 0)) / 3.5 \
            + self.px * self.px * 0.75 / 750
        tokens_out = TOKENS_OUT[self.task]
        if b.kind == "anthropic":
            price = backends.CLAUDE_PRICES.get(b.model)
            if price:
                half = 0.5 if getattr(self.args, "claude_batch", False) else 1.0
                return (tokens_in * price[0] + tokens_out * price[1]) / 1e6 * half, \
                    f"~{tokens_in:.0f} tokens in, ~{tokens_out} out at list price" + (" (Batch: half)" if half < 1 else "")
        if b.price[0] is not None:
            return (tokens_in * b.price[0] + tokens_out * (b.price[1] or 0)) / 1e6, "from the prices in backends.toml"
        return None, "unknown: set usd_per_mtok_in/out for this backend, or run a few with --limit"

    def summary(self) -> dict:
        usd, how = self.per_image_usd()
        return {"task": self.task, "analyzer": self.aid, "model": self.definition["model"],
                "images in scope": len(self.rows), "already answered": self.held, "to send": len(self.todo),
                "usd per image": None if usd is None else round(usd, 6),
                "estimate usd": None if usd is None else round(usd * len(self.todo), 2), "estimate from": how}


def cmd_plan(args, store: Store) -> None:
    if not args.task:
        raise SystemExit("plan needs a task")
    print(json.dumps(Job(args, store).summary(), indent=1))


# recording

def record(store: Store, job: Job, run: str, sha: str, status: str, out, reply=None, seconds=None, note=None) -> dict:
    row = {"sha256": sha, "region": 0, "analyzer": job.aid, "task": job.task, "status": status, "out": out,
           "run": run, "at": now()}
    if reply is not None:
        for k in ("tokens_in", "tokens_out", "usd"):
            if getattr(reply, k) is not None:
                row[k] = getattr(reply, k)
        if reply.model:
            row["served_model"] = reply.model
    if seconds is not None:
        row["seconds"] = round(seconds, 2)
    if note:
        row["note"] = note
    append_jsonl(store.run_dir(run) / "analyses.jsonl", [row])
    store.index_analysis(row, run)
    store.commit()
    return row


def keep_raw(store: Store, run: str, sha: str, replies) -> None:
    d = store.run_dir(run) / "raw"
    d.mkdir(parents=True, exist_ok=True)
    with gzip.open(d / "answers.jsonl.gz", "at", encoding="utf-8") as fh:
        for r in replies:
            fh.write(json.dumps({"sha256": sha, "text": r.text, "raw": r.raw}, ensure_ascii=False) + "\n")


def write_run_json(store: Store, job: Job, run: str, extra: dict) -> None:
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "created": now()}
    info.update({"task": job.task, "analyzer": job.aid, "definition": job.definition,
                 "rules_git": job.rules.git, "backend": job.args.backend, "updated": now()})
    for k, v in extra.items():
        info[k] = info.get(k, 0) + v if isinstance(v, (int, float)) and k in info else v
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")


def judge(job: Job, text: str):
    """(status, answer, problems) for one model answer."""
    try:
        obj = first_json(text)
    except ValueError as err:
        return "invalid", None, [str(err)]
    errs = problems(obj, job.schema)
    return ("invalid" if errs else "ok"), obj, errs


def ask_one(job: Job, sha: str):
    b = job.backend
    system, user, image = job.rules.system(), job.parts(sha), job.image(sha)
    t = time.monotonic()
    first = b.ask(system, user, image, job.schema)
    status, obj, errs = judge(job, first.text)
    replies = [first]
    if status == "invalid":  # once more, with what was wrong (photo-rules/README.md, "API")
        again = b.ask(system, [*user, "Your previous answer was not valid: " + "; ".join(errs)
                               + "\nAnswer again with one valid JSON object."], image, job.schema)
        replies.append(again)
        status, obj, errs = judge(job, again.text)
    total = backends.Reply(text=replies[-1].text, model=replies[-1].model,
                           tokens_in=sum(r.tokens_in or 0 for r in replies),
                           tokens_out=sum(r.tokens_out or 0 for r in replies),
                           usd=None if any(r.usd is None for r in replies) else sum(r.usd for r in replies))
    return status, obj, errs, total, replies, time.monotonic() - t


def cmd_run(args, store: Store) -> None:
    if args.collect:
        return collect(args, store)
    if not args.task:
        raise SystemExit("run needs a task")
    job = Job(args, store)
    s = job.summary()
    print(json.dumps(s, indent=1), file=sys.stderr)
    if not job.todo:
        print("nothing to send: every image in scope has an answer under this analyzer", file=sys.stderr)
        return
    if job.backend is not None and job.backend.kind != "manual":
        est = s["estimate usd"]
        if est is None and not args.yes:
            raise SystemExit("the cost is unknown; rerun with --limit 5 to measure it, or --yes")
        if est is not None and est > ASK_OVER_USD and not args.yes:
            raise SystemExit(f"estimated ${est:.2f}: rerun with --yes to go ahead (and --max-usd to cap it)")
    run = args.run or store.unsealed("run", f"{job.task}-{args.backend}-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    analyzer.register(store, job.definition)
    if job.backend is None:
        return chat(args, store, job, run)
    if args.claude_batch:
        if s["estimate usd"] is not None and s["estimate usd"] > args.max_usd:  # a batch cannot be stopped halfway
            raise SystemExit(f"estimated ${s['estimate usd']:.2f} is over --max-usd ${args.max_usd:.2f}; "
                             "raise it, or send part with --limit")
        return submit_batch(args, store, job, run)
    spent, counts, t0 = 0.0, {"ok": 0, "invalid": 0, "errors": 0}, time.monotonic()
    with ThreadPoolExecutor(max_workers=job.backend.parallel) as pool:
        queue = list(job.todo)
        while queue and spent < args.max_usd:
            wave, queue = queue[: job.backend.parallel * 2], queue[job.backend.parallel * 2:]
            futures = {pool.submit(ask_one, job, sha): sha for sha in wave}
            for f in as_completed(futures):
                sha = futures[f]
                try:
                    status, obj, errs, total, replies, secs = f.result()
                except backends.BackendError as err:  # not recorded: a rerun tries again
                    counts["errors"] += 1
                    print(f"  {job.photo_id(sha)}: {err}", file=sys.stderr)
                    continue
                keep_raw(store, run, sha, replies)
                record(store, job, run, sha, status, obj, total, secs, "; ".join(errs) if errs else None)
                spent += total.usd or 0
                counts[status] += 1
            print(f"  {sum(counts.values())}/{len(job.todo)} done, ${spent:.4f} spent", file=sys.stderr)
    if queue:
        print(f"stopped at --max-usd ${args.max_usd:.2f}; {len(queue)} left for the next run", file=sys.stderr)
    write_run_json(store, job, run, {"answers_ok": counts["ok"], "answers_invalid": counts["invalid"],
                                     "usd": round(spent, 6), "seconds": round(time.monotonic() - t0, 1)})
    print(json.dumps({"run": run, **counts, "usd": round(spent, 4)}))


# Claude Message Batches

def submit_batch(args, store: Store, job: Job, run: str) -> None:
    b = job.backend
    if b.kind != "anthropic":
        raise SystemExit("--claude-batch needs a Claude backend")
    reqs = [(sha, b.params(job.rules.system(), job.parts(sha), job.image(sha), job.schema)) for sha in job.todo]
    bid = b.submit(reqs)
    d = store.run_dir(run)
    d.mkdir(parents=True, exist_ok=True)
    (d / f"pending-{bid}.json").write_text(json.dumps({"batch": bid, "analyzer": job.aid, "task": job.task,
                                                       "shas": job.todo, "submitted": now()}, indent=1))
    write_run_json(store, job, run, {"batches": [bid]})
    print(f"submitted {len(reqs)} requests as {bid}; collect with: imagekit run --collect {run} --backend {args.backend}")


def collect(args, store: Store) -> None:
    d = store.run_dir(args.collect)
    info = json.loads((d / "run.json").read_text())
    pending = sorted(d.glob("pending-*.json"))
    if not pending:
        print("no batches waiting in this run")
        return
    args.task = info["task"]
    args.model = info["definition"]["model"]
    job = Job(args, store)
    if job.aid != info["analyzer"]:
        raise SystemExit("the rules or settings changed since this batch was sent; collect with the same ones")
    for p in pending:
        meta = json.loads(p.read_text())
        res = job.backend.collect(meta["batch"])
        if res is None:
            print(f"{meta['batch']}: still running")
            continue
        counts, spent = {"ok": 0, "invalid": 0, "errors": 0}, 0.0
        for sha, r in res:
            if isinstance(r, str):
                counts["errors"] += 1
                continue
            status, obj, errs = judge(job, r.text)
            keep_raw(store, args.collect, sha, [r])
            record(store, job, args.collect, sha, status, obj, r, None, "; ".join(errs) if errs else None)
            counts[status] += 1
            spent += r.usd or 0
        p.rename(p.with_name(p.name.replace("pending-", "collected-")))
        write_run_json(store, job, args.collect, {"answers_ok": counts["ok"], "answers_invalid": counts["invalid"],
                                                  "usd": round(spent, 6)})
        print(json.dumps({"batch": meta["batch"], **counts, "usd": round(spent, 4)}))


# chat window

def chat(args, store: Store, job: Job, run: str) -> None:
    """First call: write prompts and images to --chat-dir. Later calls: read back <photo id>.json answers."""
    if not args.chat_dir:
        raise SystemExit("the chat backend needs --chat-dir")
    d = Path(args.chat_dir).expanduser()
    d.mkdir(parents=True, exist_ok=True)
    (d / "00-system.md").write_text(job.rules.system())
    got = 0
    for sha in job.todo:
        pid = job.photo_id(sha)
        answer = d / f"{pid}.json"
        if answer.exists():
            status, obj, errs = judge(job, answer.read_text())
            record(store, job, run, sha, status, obj, None, None, "; ".join(errs) if errs else None)
            got += 1
            continue
        if not (d / f"{pid}.jpg").exists():
            (d / f"{pid}.jpg").write_bytes(job.image(sha))
            (d / f"{pid}.prompt.md").write_text(job.user(sha))
    left = len(job.todo) - got
    (d / "README.txt").write_text(
        "Paste 00-system.md into a new chat, then for each image paste its .prompt.md and attach its .jpg.\n"
        "Save each answer as <photo id>.json here, then run the same imagekit command again.\n")
    if got:
        write_run_json(store, job, run, {"answers": got})
    print(f"{got} answers read back; {left} prompts waiting in {d}")
