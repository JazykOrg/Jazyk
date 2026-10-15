"""Opt-in real model acceptance through the production ACP agent and MCP tools.

No model transport is mocked. Evidence contains prompts, ACP updates, run records,
and independent assertions. Credentials are inherited, never written to evidence.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import threading
import time


def environment(env_file, engine, model, codec, rounds):
    env = dict(os.environ)
    if env_file:
        for line in Path(env_file).read_text().splitlines():
            match = re.match(r"(?:export )?(JAZYK_[A-Z_]+)\s*=\s*(.*)", line)
            if match:
                env.setdefault(match[1], match[2].strip().strip("\"'"))
    for key in ("JAZYK_RUNTIME_URL", "JAZYK_RUNTIME_TOKEN_FILE"):
        env.pop(key, None)
    env.update(JAZYK_CONTAINER_ENGINE=engine, JAZYK_MODEL=model,
               JAZYK_CODEC=codec, JAZYK_ACP_AGENT="embedded",
               JAZYK_AGENT_MAX_ROUNDS=str(rounds), JAZYK_MAX_RETRIES="0",
               JAZYK_CALL_TIMEOUT="90", JAZYK_READ_TIMEOUT="90",
               JAZYK_MAX_COMPLETION_TOKENS="2048")
    return env


class ACP:
    def __init__(self, binary, root, env, evidence, proxy):
        self.seq = 0
        self.messages = queue.Queue()
        self.trace = (evidence / "acp.jsonl").open("w")
        self.stderr = (evidence / "stderr.log").open("w")
        self.proc = subprocess.Popen([binary, "acp" if proxy else "agent"], cwd=root,
                                     env=env, stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=self.stderr,
                                     text=True, bufsize=1)
        self.calls = []
        self.text = []
        self.updates = []
        def reader():
            for line in self.proc.stdout:
                self.messages.put(json.loads(line))
            self.messages.put(None)
        threading.Thread(target=reader, daemon=True).start()

    def send(self, method, params, notify=False):
        self.seq += 1
        message = dict(jsonrpc="2.0", method=method, params=params)
        if not notify:
            message["id"] = self.seq
        self.trace.write(json.dumps({"sent": message}) + "\n")
        self.trace.flush()
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()
        return self.seq

    def receive(self, request_id, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                message = self.messages.get(timeout=1)
            except queue.Empty:
                continue
            if message is None:
                raise RuntimeError("ACP exited; inspect stderr.log")
            self.trace.write(json.dumps({"received": message}) + "\n")
            self.trace.flush()
            update = message.get("params", {}).get("update", {})
            self.updates.append(update)
            kind = update.get("sessionUpdate")
            if kind == "tool_call":
                self.calls.append(update)
                print("TOOL", update.get("title"), json.dumps(update.get("rawInput")), flush=True)
            elif kind == "tool_call_update":
                print("RESULT", update.get("status"), str(update.get("rawOutput", ""))[:350], flush=True)
            elif kind == "agent_message_chunk":
                self.text.append(update.get("content", {}).get("text", ""))
            if message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]
        raise TimeoutError("ACP deadline; inspect acp.jsonl")

    def close(self, sid):
        try:
            if sid:
                self.send("session/cancel", {"sessionId": sid}, True)
            self.proc.stdin.close()
            self.proc.wait(timeout=8)
        except (BrokenPipeError, subprocess.TimeoutExpired):
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.trace.close()
        self.stderr.close()


def fixture(root, case):
    (root / ".jazyk").mkdir(parents=True)
    (root / "docs").mkdir()
    (root / "docs/main.md").write_text("# Receipt\n\nProduce a receipt from input.txt.\n")
    (root / "input.txt").write_text("violet\n")
    (root / "build.py").write_text('''import json, sys
from pathlib import Path
assert sys.version_info[:2] == (3, 12)
assert 'alpine' in Path('/etc/os-release').read_text()
assert Path('input.txt').read_text().strip() == 'violet'
assert Path('docs/main.md').exists()
assert not Path('scratch.txt').exists()
c = Path('.cache/count'); n = int(c.read_text()) + 1 if c.exists() else 1
c.parent.mkdir(exist_ok=True); c.write_text(str(n))
Path('products').mkdir(exist_ok=True)
Path('products/receipt.json').write_text(json.dumps({'input': 'violet', 'count': n}))
Path('scratch.txt').write_text('discard')
print('RECEIPT_READY', n)
''')
    config = 'version=1\n[[sets]]\nid="ds_main"\nname="Main"\ndocs=["docs/**/*.md"]\n[gen]\ndeliverable="."\n'
    if case != "create":
        command = "exit 7" if case == "repair" else "python build.py"
        if case in ("cancel", "timeout"):
            command = "echo CHILD_READY; sleep 40 & wait"
        config += f'''[[tasks]]
id="receipt"
name="Receipt"
description="Build the receipt from input.txt, retaining products and reusable cache."
command={json.dumps(command)}
image="docker.io/library/python:3.12-alpine"
timeout_seconds={2 if case == 'timeout' else 45}
products=["products/**"]
cache=[".cache/**"]
'''
    (root / ".jazyk/jazyk.toml").write_text(config)


PROMPTS = {
    "ping": "Reply with exactly READY. Do not call any tools.",
    "discover": "Inspect the configured tasks and tell me which one builds a receipt, its image, and whether local execution is available. Do not run or edit anything.",
    "run": "Build the receipt with the existing task. Wait for completion, inspect its retained receipt and log, and report the outcome with evidence. Do not change the task or files.",
    "create": "Save and run a task named Receipt with id receipt to execute python build.py in docker.io/library/python:3.12-alpine. Use a 45 second timeout. Retain products/** and cache .cache/**. Wait until it finishes, inspect the receipt and log, and report the outcome.",
    "repair": "Run the receipt task and inspect its outcome and log. If it fails, repair its command to python build.py, preserving its other settings, then run again and inspect the retained receipt. Report evidence of both outcomes.",
    "cancel": "Start the receipt task, wait until its log contains CHILD_READY, then cancel it while executing. Confirm termination and report its final state. Do not edit its settings or files.",
    "timeout": "Run the receipt task, let its configured timeout expire, then inspect its log and final status. Do not cancel it or change its timeout. Report whether it actually succeeded and whether it terminated.",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--env-file")
    parser.add_argument("--engine", choices=["podman", "docker"], default="podman")
    parser.add_argument("--model", default="qwen3.8:27b-mlx")
    parser.add_argument("--codec", choices=["native", "text"], default="native")
    parser.add_argument("--case", choices=list(PROMPTS), required=True)
    parser.add_argument("--entry", choices=["acp", "mcp"], default="acp")
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--rounds", type=int, default=10)
    parser.add_argument("--deadline", type=int, default=360)
    parser.add_argument("--repeat", action="store_true")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    evidence = Path(args.evidence).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    root = evidence / "project"
    fixture(root, args.case)
    env = environment(args.env_file, args.engine, args.model, args.codec, args.rounds)
    acp = ACP(binary, root, env, evidence, args.entry == "acp")
    sid = None
    started = time.monotonic()
    def cli(*words):
        result = subprocess.run([binary, *words], cwd=root, env=env,
                                text=True, capture_output=True, timeout=15)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return json.loads(result.stdout)
    try:
        acp.receive(acp.send("initialize", {"protocolVersion": 1, "clientCapabilities": {},
                    "clientInfo": {"name": "runtime-model-acceptance", "version": "1"}}), 30)
        servers = [] if args.entry == "acp" or args.case == "ping" else [{
            "name": "jazyk", "command": binary,
            "args": ["mcp", "generate,verify", "--ephemeral", "--serve-files"], "env": []}]
        sid = acp.receive(acp.send("session/new", {"cwd": str(root), "mcpServers": servers}), 30)["sessionId"]
        for index in range(2 if args.repeat else 1):
            prompt = PROMPTS[args.case] if index == 0 else "Run it once more in a fresh container. Inspect the new receipt and log and confirm the cache was reused."
            reply = acp.receive(acp.send("session/prompt", {"sessionId": sid,
                         "prompt": [{"type": "text", "text": prompt}]}), args.deadline)
            assert reply.get("stopReason") == "end_turn", reply
        names = [call.get("title") for call in acp.calls]
        # Discover durable records without depending on the supervisor's directory layout.
        context = cli("tasks")
        (evidence / "context.json").write_text(json.dumps(context, indent=2))
        runs = context.get("execution", {}).get("runs", [])
        if args.case == "ping":
            assert "".join(acp.text).strip() == "READY", acp.text
        else:
            assert "task_context" in names, names
        if args.case in ("ping", "discover"):
            assert "run_task" not in names and "update_tasks" not in names, names
        else:
            assert "run_task" in names and "read_run_log" in names, names
            assert runs, context
            assert all(run.get("terminated") for run in runs), runs
            if args.case in ("run", "create", "repair"):
                receipt = json.loads((root / "products/receipt.json").read_text())
                assert receipt == {"input": "violet", "count": 2 if args.repeat else 1}, receipt
                assert not (root / "scratch.txt").exists()
                assert not (root / ".cache/count").exists()
                assert "read_run_changes" in names, names
            if args.case in ("create", "repair"):
                assert "update_tasks" in names, names
            if args.case == "repair":
                assert {r["state"] for r in runs} >= {"succeeded", "failed"}, runs
            if args.case == "cancel":
                assert "cancel_run" in names and runs[0]["state"] == "cancelled", runs
            if args.case == "timeout":
                assert runs[0]["state"] == "timed_out", runs
        result = {"case": args.case, "engine": args.engine, "model": args.model,
                  "codec": args.codec, "entry": args.entry, "seconds": round(time.monotonic()-started, 2),
                  "calls": names, "summary": "".join(acp.text), "passed": True}
        (evidence / "result.json").write_text(json.dumps(result, indent=2))
        print("PASS", json.dumps(result), flush=True)
    except Exception as error:
        result = {"case": args.case, "engine": args.engine, "model": args.model,
                  "codec": args.codec, "entry": args.entry, "passed": False,
                  "seconds": round(time.monotonic()-started, 2), "error": str(error),
                  "calls": [call.get("title") for call in acp.calls], "summary": "".join(acp.text)}
        (evidence / "result.json").write_text(json.dumps(result, indent=2))
        raise
    finally:
        acp.close(sid)
        # Close/cancel the owner first; also clean up explicitly for direct MCP sessions.
        try:
            for run in cli("tasks").get("execution", {}).get("runs", []):
                if not run.get("terminated"):
                    cli("run-cancel", run["id"])
        except Exception as error:
            print("CLEANUP ERROR", str(error), flush=True)
        print("Evidence:", evidence, flush=True)


if __name__ == "__main__":
    main()
