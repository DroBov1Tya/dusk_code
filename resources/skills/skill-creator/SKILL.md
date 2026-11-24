---
name: skill-creator
description: "Write or revise a Dusk skill: its name, description, steps, Markdown references and a check that it works. Use when the user asks to make, save or fix a skill or a reusable procedure; to change a skill on evidence from the current work, load skill-evolve."
source: "Duskcode"
author: "Duskcode"
license: MIT
tags: [skills, authoring]
---
# Creating a Dusk skill

Write instructions that another Dusk agent can apply with the tools actually enabled
in its session. A skill is knowledge loaded by `load_skill`, not executable tooling.

## Choose the outcome

Use the user's request and demonstrated workflow to identify the input, expected
result, trigger, and a check that distinguishes success from failure. Ask only about
missing decisions that affect the result. Edit a matching existing skill instead of
creating another entry for the same procedure.

## Save it

The skill root is the parent of this skill's folder, supplied when it was loaded.
Create `<skill-root>/<short-name>/SKILL.md` with `run_command` (a quoted heredoc,
`cat > file <<'EOF'`), or make a focused edit with a short script. When the command
leaves a valid skill, Dusk ends its result with a `[dusk_skill_saved]` line; without that
line the file does not read as a skill. Do not claim a save succeeded before it.

Use a short lowercase name with digits and hyphens. Dusk identifies skills by folder;
`description` is required. For a user-owned skill, omit `source`, `author`, and `license`
unless the user supplies them. Preserve provenance when revising imported material.

The description is all an agent sees before it loads the skill, so it decides whether
the skill is used. In one line, say what the skill does, then when to use it (the
situations and the words a user would say) and, where a neighbour skill is close, when
not to. Keep it under 400 characters: the list cuts the rest.

```markdown
---
description: "Perform a specific procedure when its concrete trigger occurs."
tags: [topic]
---
# Procedure

State the input and result. Describe the actions, how to check the result, and the
known stopping conditions. Name only tools and commands verified in this environment.
```

Write the body as ordered, imperative steps; add a checklist where a step is easy to
skip and an example only when it was run. Leave out what an agent does anyway. The
optional `links` and `entry` fields are described in the `duskcode` skill.

Keep every file in the skill directory Markdown (`.md`). Put substantial conditional
guidance in linked `references/<topic>.md` files and explain when to read each one.
Do not bundle scripts, binary templates, archives, or provider-specific launcher
metadata. A procedure may operate on non-Markdown user artifacts outside its own
skill directory, using the enabled tools and installed project dependencies.

## Check the result

Read back the saved files. Confirm the front matter parses through `load_skill`, every
local reference exists, and no non-Markdown file was added. Check a realistic request
against the stated outcome when the available tools and authorization permit it.
For an edit, include the case that exposed the old failure. Report an unexecuted
procedure as unverified; a valid description alone proves no task behavior.

Finish with the saved path, what the skill handles, checks actually performed, and
remaining dependencies. Do not promise automated benchmarks, packaging, description
optimization, or background runs: this skill ships no helpers for those capabilities.
