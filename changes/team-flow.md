Changed
A team works the way a careful lead runs one. The TEAM lead reads the project itself
(read_file, search_files, project_graph, recall) and hands work out only through the
plan: it no longer has ask_agent, commands or write tools, and a large search is a task
of the new read-only `scout` role. Every task of an `add` needs `accept` and, unless it is
a scout's, `files`; two tasks of one add that share a file need an `after` between them.
`run` takes the goal, which the Team page shows. The core gives out one wave at a time: a
wave starts when the one before is all done or skipped, and a failed or blocked task holds
the next wave until the lead reopens, edits or skips it (`skip`, new). Members end with a
five-line footer (Status, Result, Checked, Not checked, Open), kept as the task's report
that `show <id>` prints; an answer with no status waits for the lead. A task that changed
files is reviewed by a fresh member (`team.reviewer`, `code-reviewer` by default) who gets
the card, the report and the diff and ends with a verdict; a fix goes back to the same
member once, then a second fresh review, then the lead decides. At the end a critic
(`team.critic`) lists what the request asked that no task did or verified, and the end
line hands it to the lead. `limits.team_review: false` switches reviews and the critic
off. A task without files that changes one waits for the lead. Desktop shows a review as
a chip on its task, the critic as the last column, skipped tasks struck through, and a
task's report in its details; plan counts count the lead's tasks only.
