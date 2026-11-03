# Trigger eval review

Skill: `<skill-name>`

Current description: `<description>`

Present the proposed queries as a Markdown table with columns `ID`, `Query`, and
`Should trigger`. Escape pipes and line breaks in table cells. Ask the user to
identify edits, additions, removals, and incorrect labels by ID. Apply their
feedback to the workspace's `trigger-eval.json` array:

```json
[
  {"query": "A realistic request", "should_trigger": true},
  {"query": "A realistic near miss", "should_trigger": false}
]
```

Confirm the approved query set before starting optimization. This is a manual
review process; there is no browser editor or export/download button.
