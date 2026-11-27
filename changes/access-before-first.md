Added
The access mode (default, ask, sandbox, auto, full access) can be chosen on a new chat before its first message, in the desktop app, the web page and the terminal (`/access`). The first message starts the chat with that mode, and each new chat begins at the default again. `ThreadRequest::New` and `NewTemporary` take an optional `approvals`; protocol 40, still accepting 39.
