# Blocks

A block is structured content a chat shows that is not markdown. The model makes one with
the `show` tool; a coloured label inside its text needs no tool. Types and helpers are in
`projects/protocol/src/thread/block/` and `projects/protocol/src/thread/transcript/mark.rs`,
the tool in `projects/core/src/tools/harness/show.rs`.

## Kinds

`show` takes exactly one of `flow`, `checks`, `choices`. Unknown fields are refused at
every level.

| Kind | Fields | Caps |
| --- | --- | --- |
| `flow` | `layers[]`: `title`, `subtitle?`, `cards[]` (`title`, `line?`), `emphasis?` | 2-8 layers, 0-8 cards a layer; title 60, subtitle 120, card title 40, card line 80 characters; at most one layer with `emphasis` |
| `checks` | `items[]`: `name`, `state` (`queued`, `running`, `passed`, `failed`, `skipped`), `counts?` (`passed`, `failed`, `total`), `detail?` | 1-20 checks; name 80, detail 120 characters; `passed + failed <= total` |
| `choices` | `labels[]` | 2-4 labels of 24 characters, all different |

Every text is a `Line`: trimmed, not empty, one line, no control or text-direction
characters, capped in characters. A client draws a `Line` as it is: never as markdown or
HTML, and a URL in it is not a link.

`choices` are reply buttons under the answer. A click sends the label as the user's next
message, the same as typing it. Nothing waits for it and the turn ends as usual; `ask_user`
is the tool that asks and waits. Buttons are live only while they are the last thing in
the thread; after any later message a client draws them disabled or not at all.

## Labels in text

`{{tone: label}}` in an assistant message is a coloured label. The tones are `neutral`,
`info`, `success`, `warning`, `danger`, `accent`; each client gives them colours of its
theme, and the model cannot name a colour. A label is at most 40 characters on one line,
with no leading or trailing space and none of `` { } ` * _ [ ] < > \ ``. One space may
follow the colon. Anything else stays the text it was written as.

`thread::marks(text)` splits text into `Span::Text` and `Span::Mark { tone, text }`;
`thread::unmarked(text)` drops the syntax and keeps the words. A client calls `marks` on
the text runs its markdown parser gives and never on code spans or code blocks. A label
holds no markdown characters, so one run holds it whole.

## Where a block lives

Nowhere new. The arguments of the `show` call are the block, and the thread already stores
them with the assistant message (`Message::tool_calls`). The tool checks them and answers
with `Block::text()`, the plain text of the block under a heading (`Flow:`, `Checks:`,
`Choices:`). That text is what the model reads, what `read_history` returns, and what a
client that does not draw a kind shows: the usual tool step.

There is no event and no table for blocks, so the protocol version did not change.

## Drawing one

- Live: on `ThreadEvent::ToolStarted(call)` keep `Block::asked(&call)`; on `ToolFinished`
  draw it if `block.shown_by(&result.content)`.
- History: the same with a call in `Message::tool_calls` and the `content` of the tool
  message whose `tool_call_id` is the call's.
- With both at hand, `Block::shown(&call, content)` does it in one step.

A block that was not shown (out of caps, tool switched off, denied by a plugin,
interrupted) gives `None` or `false`, and the call stays a plain step with its error.
Match on `Block` exhaustively, so a new kind fails to compile until it is drawn or sent to
the plain text on purpose.

## Adding a kind

1. A file in `thread/block/` with the type: `Line<N>` for every text, `deny_unknown_fields`,
   a `check` for what types do not hold, a `write` for the plain text.
2. A variant in `Block`, a field in `Show`, an arm in `TryFrom<Show>`, `heading` and `text`.
   A heading never changes once released: stored results are recognised by it.
3. Tests at every cap, for unknown fields, the text and the round trip (`block/tests.rs`).
4. A phrase in the description of `show`; keep it short, every request carries it.
5. An arm in each client, and a row in the table above.

Changing a released kind: add optional fields only. Stored calls are read again on every
reload, and one that no longer parses turns back into a plain step.
