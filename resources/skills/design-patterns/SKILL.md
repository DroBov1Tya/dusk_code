---
name: design-patterns
description: Replace generic interface choices with patterns justified by the user's task, content and product constraints. Use for a visual redesign, a UI critique or a request to make a screen less templated; excludes backend architecture and software design patterns.
source: dusk_code
license: MIT
metadata:
  author: dusk_code contributors
  inspiration: https://github.com/heydamir/unslop
---

# Design Patterns

Make the interface explain its purpose through its organization and behavior.
An unusual font or a different accent cannot repair a screen whose structure hides
the work people came to do. Choose patterns by the information and decisions they
support, then give those patterns a coherent visual language.

## Establish the decision surface

Identify the screen, intended audience, most frequent task, and the consequence of
an incorrect action. Read the existing components and theme before proposing a
replacement. Preserve the user's chosen style, content, workflows and public
interfaces unless changing them belongs to the request.

Use real content lengths and representative data. For a monitoring console, ask
which signal deserves investigation; for a catalog, which attributes distinguish
items; for a form, what information a person needs before committing. These
answers determine prominence and grouping more reliably than a trend board.

If the input is an image or a public URL without editable source, separate a
critique or proposed implementation from changes to the original product. A
reconstruction is a new artifact and cannot establish the original's behavior.

## Keep a small defect ledger

For each material problem record: location, visible symptom, effect on a task,
proposed correction, and how it can be checked. Prioritize lost information,
ambiguous actions and inaccessible controls before decorative differences.
Distinguish an observed defect from a preference or an untested hypothesis.

Treat repeated card containers, competing accents, uniform type emphasis and
ornamental labels as questions to investigate. They are not automatic defects.
A familiar component is appropriate when it helps people recognize an action.
If the current design works, report that finding and limit edits to demonstrated
problems rather than manufacturing a redesign.

## Match the pattern to the work

| Information relationship | Candidate arrangement | Check before choosing |
|---|---|---|
| Many records compared by the same attributes | Aligned rows with shared headings | Can people scan one attribute without opening each record? |
| One decision requires supporting context | Main workspace beside an inspector | Does the selected item remain visible while details change? |
| A long task has dependent inputs | Sections with a persistent completion summary | Can people revise earlier input without losing later work? |
| Events must be interpreted over time | Time-based grouping with explicit units | Are gaps, ordering and time zones unambiguous? |
| A visitor needs to understand an unfamiliar service | Concrete demonstration beside a concise explanation | Is the demonstration truthful and readable at the intended size? |

These are candidates, not a mandatory template collection. Compare alternatives
only where the tradeoff matters. Record the chosen organization, the content
that earns emphasis, and the constraints it must survive. Proceed within existing
authorization; ask only when a missing decision would materially change scope.

## Express hierarchy consistently

Derive spacing from content groups and repeated alignment anchors. Use the
project's tokens where possible; introduce a new token only for a repeated role.
Distinguish body text, navigation, headings and quantitative values through their
reading needs. Verify line wrapping with actual strings, including localization.

Assign color a job: selection, status, grouping or brand recognition. A status
must remain understandable without color alone. Choose imagery for explanatory
value and available rights. Do not add a remote font, asset service, dependency
or generated image merely to make the result look different.

Write action labels around the resulting user operation. Keep the same operation
named consistently across controls and feedback. Preserve useful context in
empty, pending, failed and completed states. Animate only when the transition
helps someone understand a change, and respect reduced-motion preferences.

Apply the smallest coherent set of changes that resolves the ledger. Reuse
existing components and interaction contracts. Avoid broad framework or component
rewrites as a side effect of visual work.

## Judge the result in use

When execution is available, inspect the rendered screen at representative wide
and narrow sizes. Walk the primary task with keyboard and pointer, check focus
visibility, and exercise relevant loading, empty and error states. Verify zoom,
overflow and long content where they can affect the changed screen. For terminal
interfaces, inspect the actual terminal at representative dimensions; a browser
preview does not prove terminal input or layout behavior.

Compare each ledger item with the resulting screen and its interaction. Run the
project checks appropriate to the implementation. If a browser, terminal, service
or test account is unavailable, state the unverified behavior specifically;
source inspection and compilation do not establish visual or interaction quality.

Deliver the changed artifacts, the reasoning behind the consequential choices,
the observed checks, and unresolved ledger items. Include useful before/after
captures when available without exposing private data. Stop when the requested
outcome is evidenced; extra decoration is not an acceptance criterion.

## Provenance and reuse

The high-level motivation came from [unslop by Damir Sokolovsky
(@heydamir)](https://github.com/heydamir/unslop), distributed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
This skill uses newly written instructions, a different organization and new
examples; it does not include that project's scripts, assets or reference files.
It is a separate Dusk resource, without upstream endorsement. Its newly authored
instructions use the [project's MIT license](LICENSE-MIT.txt).
The upstream [license text](LICENSE.txt) is preserved as a reference for the cited
project; it does not designate these new instructions as CC BY 4.0 material.

A textual overlap score describes only the chosen comparison method. It neither
establishes legal independence nor changes applicable license terms.
