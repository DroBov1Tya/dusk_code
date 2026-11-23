Fixed
A tool call that a model writes as text inside its reasoning now runs, even when the closing `</parameter>` or `</function>` tags are missing. When the call cannot run, for example because it names a tool that does not exist, the model is told why and which offered tool comes closest, and it gets up to two more tries; a turn that still has no answer then ends as failed.
