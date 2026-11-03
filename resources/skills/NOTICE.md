# Built-in skills

dusk_code ships these skills into `skills/` next to the config. Those from others below
keep their own licenses, in each skill's folder as `LICENSE.txt` (and `NOTICE.txt` where
the upstream has one). The front matter of each `SKILL.md` names its `source`, `author`,
`license`, `upstream` and the `changes` dusk_code made.

| Skill | Source | Author | License | Changes |
|---|---|---|---|---|
| frontend-design | Anthropic | Anthropic | Apache-2.0 | front matter fields added |
| mcp-builder | Anthropic | Anthropic | Apache-2.0 | front matter fields added |
| skill-creator | Anthropic | Anthropic | Apache-2.0 | front matter fields added |
| webapp-testing | Anthropic | Anthropic | Apache-2.0 | front matter fields added |
| gh-address-comments | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| gh-fix-ci | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| jupyter-notebook | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| playwright | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| security-best-practices | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| security-threat-model | OpenAI | OpenAI | Apache-2.0 | front matter fields added, Codex icons removed |
| spike | Nous Research | Hermes Agent, from gsd-build/get-shit-done by Lex Christopherson | MIT | tool sections rewritten for dusk |
| systematic-debugging | Nous Research | Hermes Agent, from obra/superpowers by Jesse Vincent | MIT | tool sections rewritten for dusk |
| test-driven-development | Nous Research | Hermes Agent, from obra/superpowers by Jesse Vincent | MIT | tool sections rewritten for dusk |

Upstreams: https://github.com/anthropics/skills, https://github.com/openai/skills,
https://github.com/NousResearch/hermes-agent.

`duskcode`, `project-tree`, `skill-improve`, `skill-from-practice`, `design-patterns`,
`security-assessment` and its `web-`, `api-`, `mobile-`, `ad-` and
`infrastructure-security-assessment` are dusk_code's own and share its MIT license, as do
the agents in `agents/`. `design-patterns` takes ideas, not text, from
[unslop](https://github.com/heydamir/unslop) by Damir Sokolovsky (CC BY 4.0).
