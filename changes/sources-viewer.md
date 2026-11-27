Added
A chat's sources open to read. In the desktop app a click on one in the Sources module
opens a window: markdown rendered, code highlighted, other text in the mono font, a
picture fitted, a binary file by its size with a way to open it in its own app, and a Copy
button. The web page gains a Sources tab that does the same, and the terminal's `/sources`
panel opens one with Enter (space now pins). Text comes from the core in 64 KiB parts with
Show more, so a large paste does not freeze the window. The new `work.source_text` request
raises the protocol to 40; 39 is still accepted.
