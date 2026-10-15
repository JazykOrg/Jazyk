"""Real container reports through MCP bindings into requirement/criterion evidence."""
import argparse
import json
from pathlib import Path

from runtime_lifecycle_acceptance import MCP


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--binary", required=True)
    p.add_argument("--engine", choices=["podman", "docker"], default="podman")
    p.add_argument("--evidence", required=True)
    a = p.parse_args()
    evidence = Path(a.evidence).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    root = evidence / "project"
    out = root / ".jazyk/ds_main"
    (out / "graph").mkdir(parents=True)
    (root / "tests").mkdir()
    (root / "docs").mkdir()
    (root / "source.txt").write_text("input")
    (root / "docs/main.md").write_text("# Runtime\n\nReport each criterion independently.\n")
    (root / ".jazyk/jazyk.toml").write_text('''version=1
[[sets]]
id="ds_main"
name="Main"
docs=["docs/**/*.md"]
source=["source.txt", "tests/**", "products/**"]
[gen]
deliverable="."
[[tasks]]
id="check"
name="Check"
description="Emit a controlled mixed test suite"
command="python tests/suite.py"
image="docker.io/library/python:3.12-alpine"
timeout_seconds=30
reports=["reports/*.xml"]
[[tasks]]
id="build"
name="Build"
description="Prepare the product before tests"
command="mkdir -p products; echo ready > products/built.txt"
image="docker.io/library/python:3.12-alpine"
products=["products/**"]
timeout_seconds=30
''')
    (out / "status.yaml").write_text(json.dumps({"version": 2}))
    (out / "graph/entities.yaml").write_text(json.dumps({"ent:receipt": {"name": "Receipt"}}))
    selectors = {"pass": "A::pass", "fail": "A::fail", "skip": "A::skip",
                 "missing": "A::missing", "ambiguous": "duplicate", "legacy": "legacy"}
    requirements = {"req:"+key: {"statement": "Observe "+key, "entities": ["ent:receipt"]}
                    for key in selectors}
    criteria = {"crit:"+key: {"requirement": "req:"+key, "statement": "Case "+key+" is observed", "kind": "positive"}
                for key in selectors}
    (out / "graph/requirements.yaml").write_text(json.dumps(requirements))
    (out / "graph/criteria.yaml").write_text(json.dumps(criteria))
    (root / "tests/suite.py").write_text('''from pathlib import Path
# Test names: pass fail skip missing duplicate legacy
assert Path('products/built.txt').read_text().strip() == 'ready'
assert Path('source.txt').read_text() == 'input'
Path('reports').mkdir(exist_ok=True)
Path('reports/cases.xml').write_text('<testsuite><testcase classname="A" name="pass"/><testcase classname="A" name="fail"><failure message="intentional"/></testcase><testcase classname="A" name="skip"><skipped/></testcase><testcase classname="A" name="duplicate"/><testcase classname="B" name="duplicate"/></testsuite>')
print('MIXED_REPORT_READY')
''')
    m = MCP(str(Path(a.binary).resolve()), root, a.engine, "generate,verify", timeout=120)
    try:
        for key, selector in selectors.items():
            m.tool("begin_binding", requirement="req:"+key)
            m.tool("record_binding", requirement="req:"+key, files=["source.txt"],
                   tests=[dict(criterion="crit:"+key, kind="programmatic", label="case",
                               artifact="tests/suite.py", name=selector, run="test -f products/built.txt && echo LEGACY_RAN > legacy-result.txt" if key == "legacy" else "task:check", verdict="pass",
                               evidence="Planted previous pass; the acceptance run must replace it.")],
                   verdict="pass")
        package = m.tool("begin_generation", entity="ent:receipt")
        m.tool("record_generation", entity="ent:receipt", factHash=package["factHash"],
               manifest={"files": ["source.txt"], "tests": [],
                         "build": {"run": "task:build", "produces": ["products/built.txt"]}})
        result = m.tool("run_tests", all=True)
        (evidence / "selection.json").write_text(json.dumps(result, indent=2))
        assert (root / "legacy-result.txt").read_text().strip() == "LEGACY_RAN", result
        runs = m.tool("task_context")["execution"]["runs"]
        assert len(runs) == 2, runs
        assert {r["task"]["id"] for r in runs} == {"check", "build"}, runs
        assert len(result["runs"]) == 1, result
        assert result["runs"][0]["state"] == "succeeded", result
        package = m.tool("begin_generation", entity="ent:receipt")
        (evidence / "package.json").write_text(json.dumps(package, indent=2))
        rows = {r["requirement"]: r for r in package["boundTests"]}
        expected = {"pass": "pass", "fail": "fail", "skip": "none", "missing": "none", "ambiguous": "none", "legacy": "pass"}
        for key, verdict in expected.items():
            row = rows["req:ds_main::"+key]
            assert row["status"] == {"pass": "verified", "fail": "failing", "none": "unverified"}[verdict], row
            assert row["criteriaTests"][0]["verdict"] == verdict, row
        # A new source revision must not receive evidence from a frozen old input.
        context = m.tool("task_context")
        run = m.tool("run_task", task_id="check", expected_revision=context["revision"], operation_key="stale-input")
        (root / "source.txt").write_text("newer input")
        run = m.wait(run)
        assert run["state"] == "succeeded" and run["import"]["applied"] is False, run
        (evidence / "stale-run.json").write_text(json.dumps(run, indent=2))
        # A successful command that forgets a promised product is a failed build.
        (root / "products/built.txt").unlink()
        context = m.tool("task_context")
        tasks = context["tasks"]
        next(t for t in tasks if t["id"] == "build")["command"] = "true"
        m.tool("update_tasks", expected_revision=context["revision"], tasks=tasks)
        rejected = m.tool("run_tests", all=True, error=True)
        assert "did not produce products/built.txt" in json.dumps(rejected), rejected
        print("PASS", a.engine, "one shared task, per-requirement and per-criterion pass/fail/skipped/missing/ambiguous evidence, exit-zero mixed failures, stale input import refusal", flush=True)
    finally:
        # Fixed task timeout bounds an interrupted fixture; stop any live fixture runs too.
        for run in m.tool("task_context")["execution"]["runs"]:
            if not run.get("terminated"):
                m.tool("cancel_run", run_id=run["id"])
                m.wait(run)
        m.close()
        print("Evidence:", evidence, flush=True)


if __name__ == "__main__":
    main()
