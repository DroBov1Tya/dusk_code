---
name: "jupyter-notebook"
description: "Use when the user asks to create, scaffold, or edit Jupyter notebooks (`.ipynb`) for experiments, explorations, or tutorials."
source: "OpenAI"
author: "OpenAI"
license: Apache-2.0
upstream: https://github.com/openai/skills/tree/main/skills/.curated/jupyter-notebook
changes: "dusk_code: Markdown-only edition; replaced bundled scaffold and templates with installed nbformat workflow"
---


# Jupyter Notebook Skill

Create clean, reproducible Jupyter notebooks for two primary modes:

- Experiments and exploratory analysis
- Tutorials and teaching-oriented walkthroughs

Use an installed notebook library such as `nbformat` to create valid notebook documents; no scaffold helper or notebook templates are bundled.

## When to use
- Create a new `.ipynb` notebook from scratch.
- Convert rough notes or scripts into a structured notebook.
- Refactor an existing notebook to be more reproducible and skimmable.
- Build experiments or tutorials that will be read or re-run by other people.

## Decision tree
- If the request is exploratory, analytical, or hypothesis-driven, choose `experiment`.
- If the request is instructional, step-by-step, or audience-specific, choose `tutorial`.
- If editing an existing notebook, treat it as a refactor: preserve intent and improve structure.

## Workflow

1. Identify experiment or tutorial intent, audience, and expected result.
2. Check whether the target project already provides notebook tooling. If `nbformat`
   is installed, use `nbformat.v4.new_notebook`, `new_markdown_cell`, and `new_code_cell`,
   then `nbformat.write` to create the requested `.ipynb` outside the skill directory.
   If unavailable, report the dependency before proposing an installation.
3. Keep code cells small and add Markdown cells explaining the purpose and result.
4. Follow `references/experiment-patterns.md` or `references/tutorial-patterns.md`.
5. Preserve existing notebook metadata and ordering during edits. Read
   `references/notebook-structure.md` before changing raw notebook JSON.
6. Validate document structure with the installed notebook library, then execute
   top-to-bottom when the environment supports the required dependencies and data.
   Use `references/quality-checklist.md`; distinguish format validation from execution.

No scripts or templates are bundled. Keep user notebooks and intermediate artifacts
in the target project's chosen output directory, outside this skill folder.
