#!/usr/bin/env python3
"""Generate a Markdown review of eval outputs; no server or browser required.

Usage: python generate_review.py <workspace> [--output review.md]
Feedback is edited manually in feedback.json, or collected in conversation.
"""

import argparse
import json
import re
from pathlib import Path

METADATA_FILES = {"transcript.md", "user_notes.md", "metrics.json"}


def find_runs(workspace: Path) -> list[dict]:
    """Recursively find directories that contain an outputs/ subdirectory."""
    runs: list[dict] = []
    _find_runs_recursive(workspace, workspace, runs)
    runs.sort(key=lambda r: (str(r.get("eval_id") or ""), r["id"]))
    return runs


def _find_runs_recursive(root: Path, current: Path, runs: list[dict]) -> None:
    if not current.is_dir():
        return

    outputs_dir = current / "outputs"
    if outputs_dir.is_dir():
        run = build_run(root, current)
        if run:
            runs.append(run)
        return

    skip = {"node_modules", ".git", "__pycache__", "skill", "inputs"}
    for child in sorted(current.iterdir()):
        if child.is_dir() and child.name not in skip:
            _find_runs_recursive(root, child, runs)


def build_run(root: Path, run_dir: Path) -> dict | None:
    """Build a run dict with prompt, outputs, and grading data."""
    prompt = ""
    eval_id = None

    # Try eval_metadata.json
    for candidate in [run_dir / "eval_metadata.json", run_dir.parent / "eval_metadata.json"]:
        if candidate.exists():
            try:
                metadata = json.loads(candidate.read_text())
                prompt = metadata.get("prompt", "")
                eval_id = metadata.get("eval_id")
            except (json.JSONDecodeError, OSError):
                pass
            if prompt:
                break

    # Fall back to transcript.md
    if not prompt:
        for candidate in [run_dir / "transcript.md", run_dir / "outputs" / "transcript.md"]:
            if candidate.exists():
                try:
                    text = candidate.read_text()
                    match = re.search(r"## Eval Prompt\n\n([\s\S]*?)(?=\n##|$)", text)
                    if match:
                        prompt = match.group(1).strip()
                except OSError:
                    pass
                if prompt:
                    break

    if not prompt:
        prompt = "(No prompt found)"

    run_id = str(run_dir.relative_to(root)).replace("/", "-").replace("\\", "-")

    # Collect output files
    outputs_dir = run_dir / "outputs"
    output_files: list[dict] = []
    if outputs_dir.is_dir():
        for f in sorted(outputs_dir.iterdir()):
            if f.is_file() and f.name not in METADATA_FILES:
                output_files.append({"name": f.name, "path": str(f.resolve())})

    # Load grading if present
    grading = None
    for candidate in [run_dir / "grading.json", run_dir.parent / "grading.json"]:
        if candidate.exists():
            try:
                grading = json.loads(candidate.read_text())
            except (json.JSONDecodeError, OSError):
                pass
            if grading:
                break

    return {
        "id": run_id,
        "prompt": prompt,
        "eval_id": eval_id,
        "outputs": output_files,
        "grading": grading,
    }


def fenced(text: str, language: str = "") -> str:
    fence = "`" * max(3, max((len(m.group()) + 1 for m in re.finditer(r"`+", text)), default=3))
    return f"{fence}{language}\n{text}\n{fence}"


def output_links(outputs: list[dict]) -> list[str]:
    # Link to original artifacts instead of duplicating or embedding binary data.
    links = []
    for output in outputs:
        label = output["name"].replace("[", "\\[").replace("]", "\\]")
        links.append(f"- [{label}]({Path(output['path']).as_uri()})")
    return links


def generate_markdown(runs: list[dict], skill_name: str, previous: dict | None = None, benchmark: dict | None = None) -> str:
    lines = [f"# {skill_name}: eval review", "", "Open the linked original artifacts in their native applications. Record feedback by run ID in feedback.json or in conversation.", "", "This report has no forms, autosave, inline document previews, or automatic refresh."]
    for run in runs:
        lines += ["", f"## {run['id']}", "", "### Prompt", "", fenced(run["prompt"]), "", "### Outputs", ""]
        lines += output_links(run["outputs"]) or ["No output files."]
        if run.get("grading") is not None:
            lines += ["", "### Formal grades", "", fenced(json.dumps(run["grading"], indent=2, ensure_ascii=False), "json")]
        old = (previous or {}).get(run["id"])
        if old:
            lines += ["", "### Previous iteration", "", fenced(old.get("feedback", ""))]
            lines += output_links(old.get("outputs", []))
    if benchmark is not None:
        lines += ["", "## Benchmark", "", fenced(json.dumps(benchmark, indent=2, ensure_ascii=False), "json")]
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Markdown eval review")
    parser.add_argument("workspace", type=Path)
    parser.add_argument("--skill-name", "-n")
    parser.add_argument("--previous-workspace", type=Path)
    parser.add_argument("--benchmark", type=Path)
    parser.add_argument("--output", "-o", type=Path)
    parser.add_argument("--static", "-s", type=Path, help="Alias for --output; writes Markdown")
    args = parser.parse_args()
    if args.output and args.static:
        parser.error("use either --output or --static")
    workspace = args.workspace.resolve()
    if not workspace.is_dir():
        parser.error(f"not a directory: {workspace}")
    runs = find_runs(workspace)
    if not runs:
        parser.error(f"no runs found in {workspace}")
    previous = {}
    if args.previous_workspace:
        previous_path = args.previous_workspace.resolve()
        feedback_path = previous_path / "feedback.json"
        feedback = json.loads(feedback_path.read_text()) if feedback_path.exists() else {}
        feedback_map = {r["run_id"]: r.get("feedback", "") for r in feedback.get("reviews", [])}
        previous = {r["id"]: {"outputs": r["outputs"], "feedback": feedback_map.get(r["id"], "")} for r in find_runs(previous_path)}
    benchmark = json.loads(args.benchmark.read_text()) if args.benchmark else None
    output = args.output or args.static or workspace / "review.md"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(generate_markdown(runs, args.skill_name or workspace.name, previous, benchmark), encoding="utf-8")
    # Never overwrite an existing human review, including partially completed feedback.
    feedback_path = workspace / "feedback.json"
    if not feedback_path.exists():
        feedback_path.write_text(json.dumps({"reviews": [{"run_id": r["id"], "feedback": ""} for r in runs], "status": "pending"}, indent=2) + "\n", encoding="utf-8")
    print(f"Review: {output}\nFeedback: {feedback_path} (edit manually; set status to complete)")


if __name__ == "__main__":
    main()
