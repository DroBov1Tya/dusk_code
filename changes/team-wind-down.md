Changed
A team that is stopped (the lead's `stop`, the Team page's Stop), that reached the quiet
cap, or whose provider keeps failing now winds down instead of cutting its members off:
nothing new is given out, the core asks each member at work to stop and hand over with
its footer, and the team ends once they have or after `limits.team_wrap_seconds` (180 s).
A task not finished goes back to open with that hand-over, and the next member's card
shows it under "Before you". Clients show "wrapping up" while it does (`Team.winding_since`).
A turn lost to a restart, the provider, the wind-down or the team's end no longer counts
as one of a task's three attempts (a task's fourth loss to the provider counts again); a
key or quota the provider refuses, or three such losses in a row, stop the team, and the
lead's line names the provider's reason. A stop of the chat or of a member by the user still ends it at once.
