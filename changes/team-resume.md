Changed
A team task whose member lost it (a failed turn, a restart, the provider, a wind-down
hand-over) goes back to that same member, which keeps its transcript and gets a short
resume note instead of a fresh card (migration 0049: `tasks.last_owner_thread_id`,
`tasks.resumes`). A context too long, a refused request, a second counted loss, a turn
still being cut off, a review or the critic, and a task the lead edits or reopens go to
a fresh member, whose card now says how the last attempt ended when there was no
hand-over. Members carry a one-line summary of what they do (`TeamMember.summary`),
shown where the plan lacks their task, in the Team page's member list, and in the lead's
lines (`#id (Ember-0c, code-writer: add the login form) failed: …`). The Team page's
`×n` mark reads `attempt n`, with a tooltip.
