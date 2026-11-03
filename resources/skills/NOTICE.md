# Built-in skills

dusk_code ships these skills into `skills/` next to the config. Those from others below
keep their own licenses, in each skill's folder as `LICENSE.md` (and `NOTICE.md` where
the upstream has one). The front matter of each `SKILL.md` names its `source`, `author`,
`license`, `upstream` and the `changes` dusk_code made.

| Skill | Source | Author | License | Changes |
|---|---|---|---|---|
| frontend-design | Anthropic | Anthropic | Apache-2.0 | front matter fields added |
| mcp-builder | Anthropic | Anthropic | Apache-2.0 | front matter fields added, Markdown-only workflow; executable helpers removed |
| webapp-testing | Anthropic | Anthropic | Apache-2.0 | front matter fields added, Markdown-only workflow; executable helpers removed |
| gh-address-comments | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed, Markdown-only workflow; executable helpers removed |
| gh-fix-ci | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed, Markdown-only workflow; executable helpers removed |
| jupyter-notebook | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed, Markdown-only workflow; executable helpers removed |
| playwright | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed, Markdown-only workflow; executable helpers removed |
| security-best-practices | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| security-threat-model | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |

Upstreams: https://github.com/anthropics/skills, https://github.com/openai/skills.

`skill-creator`, `duskcode`, `project-tree`, `skill-improve`, `skill-from-practice`, `design-patterns`,
`spike`, `systematic-debugging`, `test-driven-development`,
`security-assessment` and its `web-`, `api-`, `mobile-`, `ad-` and
`infrastructure-security-assessment` are dusk_code's own and share its MIT license, as do
the agents in `agents/`.
