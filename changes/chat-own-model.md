Fixed
Each chat keeps its own model and reasoning effort. Choosing a model in one chat no longer
switches the others, their running turns, their subagents or their teams; the last choice
only seeds chats started afterwards. Reopening a chat shows and uses its own model, and
chats from before this change keep the model they last ran on.
