Added
Teams: the lead's `edit` changes any field of an open, blocked or failed task, the gate
and files included, and sets the same fields on several tasks at once with `ids`; a
running task is refused with what to do instead. A failed task is retried in place by
the lead's `reopen` or the user's Retry: it keeps its id and dependents, the tasks skipped
behind it come back, its try is counted and shown, and its review gets a fresh fix round.
