#!/usr/bin/env python3
"""Generate a Markdown report from run_loop.py JSON output."""

import argparse
import json
import re
import sys
from pathlib import Path


def fenced(text: str) -> str:
    fence = "`" * max(3, max((len(m.group()) + 1 for m in re.finditer(r"`+", text)), default=3))
    return f"{fence}\n{text}\n{fence}"


def generate_markdown(data: dict, skill_name: str = "") -> str:
    lines = [f"# {skill_name + ': ' if skill_name else ''}Skill description optimization", ""]
    for key in ("original_description", "best_description", "best_score", "best_train_score", "best_test_score", "exit_reason", "holdout", "train_size", "test_size"):
        if data.get(key) is not None:
            lines += [f"**{key}**", "", fenced(str(data[key])), ""]
    lines += ["The report is rewritten after each iteration; reopen it to see updates.", ""]
    for entry in data.get("history", []):
        lines += [f"## Iteration {entry['iteration']}", "", fenced(entry.get("description", "")), ""]
        for split in ("train", "test"):
            results = entry.get(f"{split}_results", entry.get("results", []) if split == "train" else None)
            if results is None:
                continue
            passed = entry.get(f"{split}_passed", entry.get("passed", 0) if split == "train" else 0)
            total = entry.get(f"{split}_total", entry.get("total", 0) if split == "train" else 0)
            lines += [f"### {split.title()}: {passed}/{total}", ""]
            for result in results:
                lines += [f"- {'PASS' if result.get('pass') else 'FAIL'}; expected trigger: {result.get('should_trigger')}; triggers: {result.get('triggers', 0)}/{result.get('runs', 0)}", "", fenced(result["query"]), ""]
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Markdown report from run_loop output")
    parser.add_argument("input", help="Path to JSON output, or - for stdin")
    parser.add_argument("-o", "--output", help="Output Markdown file; defaults to stdout")
    parser.add_argument("--skill-name", default="")
    args = parser.parse_args()
    data = json.load(sys.stdin) if args.input == "-" else json.loads(Path(args.input).read_text())
    report = generate_markdown(data, skill_name=args.skill_name)
    if args.output:
        Path(args.output).write_text(report, encoding="utf-8")
        print(f"Report written to {args.output}", file=sys.stderr)
    else:
        print(report)


if __name__ == "__main__":
    main()
