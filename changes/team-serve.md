Added
The plan and team requests are served: `PlanTasks` pages a chat's plan, `Team` reads its team, and `ConfirmTask`, `StopTeam` and `SetTeamBudget` change it as the user, each announcing `TaskChanged` or `Team` to the clients that may read the chat. A task now carries all its fields on the board too, with its steps, tokens and changed files counted from its turns.
