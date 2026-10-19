"""imagekit plan/run tests: a fake OpenAI-style server on localhost and a fake Claude client; no network."""
from __future__ import annotations

import gzip
import io
import json
import re
import sys
import threading
import unittest
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from test_imagekit import Base, picture  # noqa: E402

from imagekit.store import Store, jsonl  # noqa: E402

TRIAGE = {"description": "a shoot with leaves", "usable": True, "views": ["V3"], "stage": "vegetative",
          "setting": "forest", "age_class": "mature", "lighting": "shade", "species_match": "unsure"}
DESCRIBE = {"overall": "A leafy shoot seen from above.", "tags": ["leaf.division=compound"], "not_shown": ["flowers"],
            "organs": [{"organ": "leaf", "text": "Twice divided into narrow segments.", "look": ["lacy", "ferny"]}]}


def answer_for(user: str) -> dict:
    pid = re.search(r"Photo id: (\S+)", user).group(1)
    if '"triage.schema.json"' in user:
        return {"photo_id": pid, **TRIAGE}
    if '"observation.schema.json"' in user:
        return {"photo_id": pid, "description": "leaves", "answers": {
            "leaf.arrangement": {"value": "opposite", "confidence": "high", "evidence": "pairs at each node"}}}
    return {"photo_id": pid, **DESCRIBE}


class FakeServer(BaseHTTPRequestHandler):
    requests: list = []
    cost = None          # usage.cost to report, as OpenRouter does
    no_schema = False    # answer 400 to response_format json_schema, as some servers do
    bad_first = False    # the first answer per image misses a required key
    upstream_grammar = False  # answer a json_schema request with an error inside HTTP 200

    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        FakeServer.requests.append(body)
        fmt = (body.get("response_format") or {}).get("type")
        if self.no_schema and fmt == "json_schema":
            return self.send(400, {"error": {"message": "response_format json_schema is not supported"}})
        if self.upstream_grammar and fmt == "json_schema":  # OpenRouter: an upstream failure inside a 200
            return self.send(200, {"error": {"message": "Grammar error: Unimplemented keys", "code": 400}})
        user = "\n\n".join(c["text"] for c in body["messages"][1]["content"] if c["type"] == "text")
        ans = answer_for(user)
        if self.bad_first and "previous answer was not valid" not in user:
            ans.pop("overall", None)
            ans.pop("usable", None)
        usage = {"prompt_tokens": 1000, "completion_tokens": 200}
        if self.cost is not None:
            usage["cost"] = self.cost
        self.send(200, {"model": body["model"] + "-served", "usage": usage,
                        "choices": [{"message": {"content": "Here it is:\n```json\n" + json.dumps(ans) + "\n```"}}]})

    def send(self, code, obj):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class RunBase(Base):
    def setUp(self):
        super().setUp()
        FakeServer.requests, FakeServer.cost, FakeServer.no_schema, FakeServer.bad_first = [], None, False, False
        FakeServer.upstream_grammar = False
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeServer)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/v1"
        photos = self.tmp / "photos"
        photos.mkdir()
        for i in range(4):
            (photos / f"p{i}.jpg").write_bytes(picture(100 + i))
        self.quiet("fetch", "folder", str(photos), "--licence", "cc0", "--creator", "Jay",
                   "--taxon", "Acer circinatum", "--batch", "own")
        self.backends = self.tmp / "backends.toml"
        self.backends.write_text(f'[box]\nkind = "openai"\nbase_url = "{self.url}"\nmodel = "vlm-8b"\nparallel = 2\n')

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        super().tearDown()

    def quiet(self, *argv) -> str:
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            self.run_cli(*argv)
        return out.getvalue()

    def go(self, *argv) -> str:
        return self.quiet(*argv, "--backends", str(self.backends))

    def analyses(self, run):
        return list(jsonl(Path(self.store_dir) / "sources" / "image-analyses" / run / "analyses.jsonl"))


class TestOpenAICompat(RunBase):
    def test_triage_then_rerun_sends_nothing(self):
        plan = json.loads(self.go("plan", "triage", "--backend", "box"))
        self.assertEqual((plan["to send"], plan["usd per image"]), (4, 0.0))
        self.go("run", "triage", "--backend", "box", "--run", "t1")
        rows = self.analyses("t1")
        self.assertEqual(len(rows), 4)
        self.assertTrue(all(r["status"] == "ok" and r["out"]["usable"] for r in rows))
        self.assertEqual(rows[0]["served_model"], "vlm-8b-served")
        sent = FakeServer.requests[0]
        self.assertEqual(sent["response_format"]["type"], "json_schema")
        self.assertNotIn("Acer", json.dumps(sent))  # the species is never named
        shared, image, tail = sent["messages"][1]["content"]
        self.assertTrue(image["image_url"]["url"].startswith("data:image/jpeg;base64,"))
        self.assertTrue(tail["text"].startswith("Photo id: img-"))
        self.assertNotIn("Photo id", shared["text"])  # everything before the image is the same for every image
        raw = Path(self.store_dir) / "sources" / "image-analyses" / "t1" / "raw" / "answers.jsonl.gz"
        self.assertEqual(len(gzip.open(raw, "rt").read().splitlines()), 4)
        info = json.loads((raw.parents[1] / "run.json").read_text())
        self.assertEqual((info["task"], info["answers_ok"]), ("triage", 4))

        FakeServer.requests.clear()
        self.go("run", "triage", "--backend", "box", "--run", "t2")
        self.assertEqual(FakeServer.requests, [])  # already answered under this analyzer
        self.go("run", "triage", "--backend", "box", "--model", "other-vlm", "--run", "t3", "--limit", "1")
        self.assertEqual(len(FakeServer.requests), 1)  # another model is another analyzer

    def test_observe_uses_triage_views(self):
        with self.assertRaises(SystemExit):
            self.go("run", "observe", "--backend", "box")
        self.go("run", "triage", "--backend", "box", "--run", "t1")
        FakeServer.requests.clear()
        self.go("run", "observe", "--backend", "box", "--run", "o1")
        self.assertEqual(len(FakeServer.requests), 4)
        user = FakeServer.requests[0]["messages"][1]["content"][0]["text"]  # the shared part, before the image
        self.assertIn("views V3", user)
        self.assertIn("palette.yaml", user)
        self.assertTrue(all(r["status"] == "ok" for r in self.analyses("o1")))

    def test_invalid_answer_is_asked_again_once(self):
        FakeServer.bad_first = True
        self.go("run", "describe", "--backend", "box", "--run", "d1")
        self.assertEqual(len(FakeServer.requests), 8)
        rows = self.analyses("d1")
        self.assertTrue(all(r["status"] == "ok" and r["tokens_in"] == 2000 for r in rows))
        self.assertEqual(rows[0]["out"]["organs"][0]["look"], ["lacy", "ferny"])

    def test_server_without_schema_mode(self):
        FakeServer.no_schema = True
        self.backends.write_text(self.backends.read_text().replace("parallel = 2", "parallel = 1"))
        self.go("run", "triage", "--backend", "box", "--run", "t1", "--limit", "2")
        modes = [(r.get("response_format") or {}).get("type") for r in FakeServer.requests]
        self.assertEqual(modes[:2], ["json_schema", "json_object"])
        self.assertNotIn("json_schema", modes[2:])  # remembered for the rest of the run
        self.assertEqual(len(self.analyses("t1")), 2)

    def test_upstream_error_inside_a_200(self):
        FakeServer.upstream_grammar = True
        self.backends.write_text(self.backends.read_text().replace("parallel = 2", "parallel = 1"))
        self.go("run", "triage", "--backend", "box", "--run", "t1", "--limit", "2")
        self.assertTrue(all(r["status"] == "ok" for r in self.analyses("t1")))
        self.assertEqual(len(FakeServer.requests), 3)  # one refused, then plain JSON for both

    def test_spend_cap_and_confirmation(self):
        FakeServer.cost = 0.5
        self.backends.write_text(self.backends.read_text().replace("parallel = 2", "parallel = 1"))
        self.go("run", "triage", "--backend", "box", "--run", "t1", "--limit", "1", "--yes")  # measures the price
        plan = json.loads(self.go("plan", "triage", "--backend", "box"))
        self.assertEqual((plan["usd per image"], plan["to send"]), (0.5, 3))
        with self.assertRaises(SystemExit):  # $1.50 estimated: needs --yes
            self.go("run", "triage", "--backend", "box", "--run", "t1")
        self.go("run", "triage", "--backend", "box", "--run", "t1", "--yes", "--max-usd", "0.9")
        self.assertEqual(len(self.analyses("t1")), 3)  # one wave of two, then the cap stopped it
        self.assertEqual(json.loads(self.go("plan", "triage", "--backend", "box"))["to send"], 1)


class TestChat(RunBase):
    def test_export_then_read_back(self):
        d = self.tmp / "chat"
        self.quiet("run", "describe", "--backend", "chat", "--model", "my chat", "--chat-dir", str(d), "--run", "c1")
        prompts = sorted(d.glob("*.prompt.md"))
        self.assertEqual((len(prompts), len(list(d.glob("*.jpg")))), (4, 4))
        self.assertTrue((d / "00-system.md").exists())
        pid = prompts[0].name.split(".")[0]
        (d / f"{pid}.json").write_text(json.dumps({"photo_id": pid, **DESCRIBE}))
        self.quiet("run", "describe", "--backend", "chat", "--model", "my chat", "--chat-dir", str(d), "--run", "c1")
        rows = self.analyses("c1")
        self.assertEqual([(r["status"], r["out"]["photo_id"]) for r in rows], [("ok", pid)])
        out = self.quiet("plan", "describe", "--backend", "chat", "--model", "my chat")
        self.assertEqual(json.loads(out)["to send"], 3)


class FakeClaude:
    """Stands in for anthropic.Anthropic: messages.create and the batches calls."""
    calls: list = []

    def __init__(self, **_):
        self.messages = SimpleNamespace(create=self.create, batches=SimpleNamespace(
            create=self.batch_create, retrieve=lambda bid: SimpleNamespace(processing_status="ended"),
            results=self.batch_results))
        self.batch = []

    @staticmethod
    def message(params):
        user = "\n\n".join(c["text"] for c in params["messages"][0]["content"] if c["type"] == "text")
        from anthropic.types import Message
        return Message.model_validate({
            "id": "msg_1", "type": "message", "role": "assistant", "model": params["model"], "stop_reason": "end_turn",
            "content": [{"type": "text", "text": json.dumps(answer_for(user))}],
            "usage": {"input_tokens": 900, "output_tokens": 300, "cache_read_input_tokens": 100,
                      "cache_creation_input_tokens": 0}})

    def create(self, **params):
        FakeClaude.calls.append(params)
        return self.message(params)

    def batch_create(self, requests):
        FakeClaude.calls.extend(requests)
        self.batch = requests
        FakeClaude.last = self
        return SimpleNamespace(id="msgbatch_1")

    def batch_results(self, bid):
        for r in FakeClaude.last.batch:
            yield SimpleNamespace(custom_id=r["custom_id"], result=SimpleNamespace(
                type="succeeded", message=self.message(r["params"])))


@unittest.skipUnless(__import__("importlib").util.find_spec("anthropic"), "needs the anthropic SDK")
class TestClaude(RunBase):
    def setUp(self):
        super().setUp()
        import anthropic
        self._real = anthropic.Anthropic
        anthropic.Anthropic = FakeClaude
        FakeClaude.calls = []

    def tearDown(self):
        import anthropic
        anthropic.Anthropic = self._real
        super().tearDown()

    def test_messages(self):
        self.go("run", "describe", "--backend", "claude", "--run", "c1")
        p = FakeClaude.calls[0]
        self.assertEqual(p["model"], "claude-haiku-5-5")
        self.assertEqual(p["output_config"]["effort"], "low")
        self.assertEqual(p["output_config"]["format"]["type"], "json_schema")
        self.assertEqual(p["system"][0]["cache_control"], {"type": "ephemeral"})
        self.assertEqual(p["messages"][0]["content"][0]["cache_control"], {"type": "ephemeral"})
        self.assertEqual(p["messages"][0]["content"][1]["type"], "image")
        rows = self.analyses("c1")
        self.assertEqual(len(rows), 4)
        self.assertAlmostEqual(rows[0]["usd"], (900 + 100 * 0.1) * 0.1e-6 + 300 * 0.5e-6)

    def test_batch_submit_and_collect(self):
        out = self.go("run", "describe", "--backend", "claude", "--claude-batch", "--run", "b1")
        self.assertIn("msgbatch_1", out)
        run_dir = Path(self.store_dir) / "sources" / "image-analyses" / "b1"
        self.assertTrue((run_dir / "pending-msgbatch_1.json").exists())
        self.assertTrue(all(len(r["custom_id"]) == 64 for r in FakeClaude.calls))
        sent = FakeClaude.calls[0]["params"]["output_config"]["format"]["schema"]
        self.assertTrue(all(o.get("additionalProperties") is False for o in objects(sent)))  # the API refuses open objects
        self.go("run", "--collect", "b1", "--backend", "claude")
        rows = self.analyses("b1")
        self.assertEqual(len(rows), 4)
        self.assertAlmostEqual(rows[0]["usd"], ((900 + 10) * 0.1e-6 + 300 * 0.5e-6) / 2)
        self.assertTrue((run_dir / "collected-msgbatch_1.json").exists())
        with Store(self.store_dir) as s:
            self.assertEqual(json.loads(self.go("plan", "describe", "--backend", "claude"))["to send"], 0)
            self.assertEqual(len(s.all_images()), 4)


def objects(schema):
    if isinstance(schema, dict):
        if "properties" in schema:
            yield schema
        for v in schema.values():
            yield from objects(v)
    elif isinstance(schema, list):
        for v in schema:
            yield from objects(v)


class TestClaudeSchema(unittest.TestCase):
    def test_closes_objects_and_drops_constraints(self):
        from imagekit.backends import claude_schema
        out = claude_schema({"type": "object", "required": ["n"], "properties": {
            "n": {"type": "integer", "minimum": 0, "maximum": 9},
            "tags": {"type": "array", "maxItems": 3, "items": {"type": "object", "properties": {"a": {"type": "string"}}}},
            "v": {"oneOf": [{"type": "string", "maxLength": 4}, {"type": "number"}]}}})
        self.assertEqual(out["properties"]["n"], {"type": "integer"})
        self.assertIs(out["additionalProperties"], False)
        self.assertIs(out["properties"]["tags"]["items"]["additionalProperties"], False)
        self.assertNotIn("maxItems", out["properties"]["tags"])
        self.assertEqual(out["properties"]["v"], {"anyOf": [{"type": "string"}, {"type": "number"}]})

    def test_open_keys_send_no_format(self):
        from imagekit.backends import claude_schema
        self.assertIsNone(claude_schema({"type": "object", "properties": {
            "answers": {"type": "object", "additionalProperties": {"type": "string"}}}}))
        self.assertIsNone(claude_schema({"type": "object", "propertyNames": {"pattern": "^[a-z]+$"}}))


if __name__ == "__main__":
    unittest.main()
