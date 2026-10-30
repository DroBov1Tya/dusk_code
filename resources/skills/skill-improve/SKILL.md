---
name: skill-improve
description: "Improve an existing skill once practice showed it is outdated or wrong, or a verified faster way exists. Use after a skill's steps failed or were bettered in this work."
source: "dusk_code"
author: "Neil Ermak"
license: MIT
tags: [skills, maintenance]
---
# Improving a skill

Change a skill on evidence from real work, never on taste.

Improve it when:
- a step failed because a command, flag, API or path changed, and you found what works;
- you did the same job another way and checked that it is faster, shorter or more reliable;
- the skill missed a step you had to add to succeed.

Leave it when the user only said it could be better, the change is style, or you have
not run the new way yourself.

Steps:
1. The skill's folder was named when you loaded it; its file is `SKILL.md` there.
2. Write down the evidence: what the skill says, what happened, what worked instead, and
   the command and result that prove it.
3. Edit the smallest part: replace the wrong step, keep the rest and the front matter.
   Keep `description` true. Do not let it grow; for each line added, look for one to cut.
4. For a skill with a `source` other than `dusk_code`, add what you changed to its
   `changes:` field, so the credit stays honest.
5. Read the file again and, when cheap, run the changed step once more.
6. Tell the user in one or two lines what changed and why.
