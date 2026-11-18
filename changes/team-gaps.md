Changed
TEAM mode: when the lead runs the plan in a chat whose approval mode asks, the user is asked once, in the usual approval card, to let the plan's gate commands run unasked; the yes covers exactly the listed lines for that team, a gate added or changed later waits for the lead until the next run asks again, and a no leaves gated tasks to the lead's review as before.
A chat keeps the mode the user last wrote to it in: switching the mode for another chat no longer pauses a team, and a lead woken by its team keeps TEAM mode.
A member's hand-in that changed files outside its task's `files` waits for the lead with the paths named, and its gate does not run; ignored paths such as build output never count.
Stopping a team kills the gate commands it is running. A gate runs at most `limits.team_gate_seconds` (300 by default) and a team at most `limits.team_max_gate_runs` gates (128 by default).
Desktop: a long agent report in the chat list no longer makes the list jump when it scrolls back into sight.
