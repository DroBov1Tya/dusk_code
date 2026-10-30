---
name: skill-from-practice
description: "Turn a way of working proven in practice into a new skill. Use when a procedure no skill covers has worked at least three times, confirmed by results or the user."
source: "dusk_code"
author: "Neil Ermak"
license: MIT
tags: [skills, learning]
---
# A skill from practice

A skill is knowledge that was tested, not a story. Write one only when all of these hold:

1. No skill covers it. If one is close, improve that one with `skill-improve` instead.
2. It worked in practice: commands ran and results were checked. What the user only
   told you is not enough.
3. It repeated. Once is chance, twice is coincidence, three times is a pattern. Count
   separate occasions, each confirmed by a passing check or by the user; use
   `read_history` to find the earlier ones.
4. It is not luck. Count an occasion only if it would succeed by chance less than one
   time in twenty (p < 0.05), such as working where the usual way failed. An easy task
   or a lucky retry does not count.
5. It will come back: the user or the project does this kind of task again.

When unsure, do not write it; tell the user what you noticed and how many times.

Steps:
1. Pick a short name of lowercase letters, digits and `-`. The skills folder is the one
   above this skill's folder; create `<name>/SKILL.md` there.
2. Write it:

```markdown
---
description: What it does and when to use it, one line.
tags: [topic]
---
# Title

When to use it. Then the steps, each with the exact command or check, how to tell it
worked, and the pitfalls met in practice.
```

3. Only verified steps, no guesses; keep it under 60 lines. Leave `source` out: the
   skill is the user's own.
4. Tell the user the skill was added, with the evidence: the occasions and their results.
   They can switch it off or edit it in `/settings`, `skills`.
