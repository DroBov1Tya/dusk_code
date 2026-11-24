Removed
A team has no step, token or time budget any more: nothing it spends ends it. What it used
is still shown (steps, tokens, minutes), and members at once stay a setting the user may
change while the team runs, from 1 up to the setting (`work.set_team_at_once` replaces
`work.set_team_budget`; desktop, browser and terminal have a stepper in place of the
budget form). Old configs'
`limits.team_max_steps`, `team_max_tokens` and `team_max_minutes` are read and dropped;
a team that ended on a budget under an older core still reads with that reason. Clients
older than protocol 38 are no longer accepted.
