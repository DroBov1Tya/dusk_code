---
name: skill-creator
description: "Create or revise a Dusk skill when the user asks for a reusable procedure, including its description, Markdown references and concrete validation."
source: "dusk_code"
author: "Neil Ermak"
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
Create `<skill-root>/<short-name>/SKILL.md` using `write_file`, or make a focused edit
using `edit_file`. Successful file-tool saves produce Dusk's skill-save notice;
shell writes do not. Do not claim a save succeeded before the tool confirms it.

Use a short lowercase name with digits and hyphens. Dusk identifies skills by folder;
`description` is required. For a user-owned skill, omit `source`, `author`, and `license`
unless the user supplies them. Preserve provenance when revising imported material.

```markdown
---
description: "Perform a specific procedure when its concrete trigger occurs."
tags: [topic]
---
# Procedure

State the input and result. Describe the actions, how to check the result, and the
known stopping conditions. Name only tools and commands verified in this environment.
```

Keep every file in the skill directory Markdown (`.md`). Put substantial conditional
guidance in linked `references/<topic>.md` files and explain when to read each one.
Do not bundle scripts, binary templates, archives, or provider-specific launcher
metadata. A procedure may operate on non-Markdown user artifacts outside its own
skill directory, using the enabled tools and installed project dependencies.

## Check the result

Read back the saved files. Confirm frontmatter parses through `load_skill`, every
local reference exists, and no non-Markdown file was added. Check a realistic request
against the stated outcome when the available tools and authorization permit it.
For an edit, include the case that exposed the old failure. Report an unexecuted
procedure as unverified; a valid description alone proves no task behavior.

Finish with the saved path, what the skill handles, checks actually performed, and
remaining dependencies. Do not promise automated benchmarks, packaging, description
optimization, or background runs: this skill ships no helpers for those capabilities.
