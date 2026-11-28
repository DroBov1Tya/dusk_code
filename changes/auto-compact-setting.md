Added
Settings has an auto-compaction switch and a slider for when it starts, from 60% to 80% of
the model's context window (80% by default). It applies to every chat, subagent and team
member at once, without a restart. With auto-compaction off, a chat whose context fills up
stops with an error that says to run /compact or turn auto-compaction back on.
