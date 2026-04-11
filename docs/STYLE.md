# Documentation Style Guide

This guide is the writer's contract for every Markdown file under `docs/`. Follow
it so the whole set reads as one voice and the cross-links keep working as the
codebase evolves.

## Voice and tone

Declarative, third person, present tense. Describe what Ryvos **does**, not what
the author did. Avoid "we", "our", "us". Avoid apologetic hedging ("perhaps",
"arguably"). When a design tradeoff needs to be explained, state it plainly and
link to the ADR that records the decision.

Do not use emoji anywhere in prose, headings, diagrams, or code fences. Do not
use marketing adjectives ("powerful", "cutting-edge", "blazing"). Ryvos speaks
for itself through behavior and benchmarks.

## File structure

Every doc has exactly one H1, which is the document title and matches the
filename's intent. Use H2 for major sections and H3 for subsections. Avoid H4
and below; if a subsection is deep enough to need H4, it probably belongs in a
separate file.

Do not use YAML front-matter. GitHub renders Markdown directly and front-matter
produces visible noise in the default viewer.

Do not wrap the whole file in a `<div>` or other HTML container. Plain Markdown
only.

## Headings

- H1: title, exactly one per file, no trailing punctuation.
- H2: major sections, stable text (other docs may anchor-link to them).
- H3: subsections, shorter and more flexible.
- Do not skip levels (no H1 followed directly by H3).
- Do not use title-case. Use sentence case: "Agent loop", not "Agent Loop".

## Target word counts

| Document type | Target range |
|---|---|
| Crate reference (`crates/*.md`) | 1500–3000 words |
| Internals (`internals/*.md`) | 2000–4000 words |
| Architecture (`architecture/*.md`) | 2500–4000 words |
| Guides (`guides/*.md`) | 800–1500 words |
| API reference (`api/*.md`) | 1000–2500 words |
| Operations (`operations/*.md`) | 800–2000 words |

A doc that is consistently under the floor is probably missing context or
examples. A doc that is over the ceiling should be split.

## Glossary discipline

Before introducing any domain term, check `glossary.md`. If the term is not
there, add it to the glossary first and then use it in the prose. The first use
of a glossary term in each document is bolded and linked, like this:
**[Director](../glossary.md#director)**. Subsequent uses in the same document
are plain text.

Never redefine a glossary term inline. If the existing definition is wrong or
incomplete, fix the glossary and leave the prose clean.

## File references

When pointing at source code, use the form
`crates/ryvos-agent/src/agent_loop.rs:142`. The path is always from the repo
root, not from the current doc's directory. Include the line number when it is
meaningful (a specific function, struct, or match arm). Omit the line number
rather than guess — a wrong line is worse than no line.

Reference structs, functions, and traits with backticks:
`AgentRuntime::run_turn`, `SecurityGate`, `ToolContext`.

For crate names, use kebab-case everywhere (`ryvos-core`, `ryvos-agent`), even
though the Rust identifier is `ryvos_core`. Kebab-case is the form that appears
in `Cargo.toml`, in the binary output, and in ADRs.

## Cross-links

Use relative Markdown paths. A doc in `docs/crates/ryvos-agent.md` links to the
glossary as `../glossary.md#director`, not as an absolute path and never as a
`github.com` URL. Relative paths work for both local previews and the rendered
GitHub view, and they survive repo moves.

It is acceptable to link to a doc that does not yet exist if it is part of the
planned file set in `docs/README.md`. The filename must match the plan exactly,
so later writers can drop the file in without updating inbound links. Flag
unwritten targets in a review comment, not in the prose.

External links (to vendor docs, RFCs, papers) are full URLs in normal Markdown
syntax. Prefer stable URLs (e.g. IETF RFC pages) over blog posts.

## Quoting source code

Quote source when the exact text is load-bearing and describe when it is not.

Quote when:
- A trait signature, function signature, or type definition is being explained.
- A match arm or error case is being documented.
- A subtle ordering (e.g. the event publication sequence) depends on the exact
  line order.

Keep quotes under 30 lines. Longer than that, describe the structure and link
to the file instead. Always prefix a quote with its file:line reference on the
line immediately before the fence:

```
See `crates/ryvos-agent/src/agent_loop.rs:142`:

```rust
pub async fn run(&self, session: &SessionId, prompt: &str) -> Result<String> {
    // …
}
```
```

Describe (rather than quote) when:
- The logic spans many files or many lines.
- The behavior is what matters, not the specific Rust syntax.
- The code changes frequently; a description ages better than a quote.

## Code fences

Always tag the language on the opening fence: `rust`, `toml`, `bash`, `json`,
`mermaid`, `text`. Never use an untagged triple-backtick fence; GitHub applies
language-specific syntax highlighting only when the tag is present.

For shell examples, use `bash` even on macOS or zsh users — the commands are
POSIX-compatible and `bash` renders consistently.

For configuration examples, use `toml` and match the real keys in
`ryvos.toml.example`. Do not invent config keys for illustration; if a key does
not exist, the example is fiction.

## Diagrams

Use Mermaid only. GitHub renders Mermaid natively; every other diagramming tool
either requires a build step, breaks dark mode, or produces binary artifacts
that clutter the repo.

Mermaid rules of thumb:
- Use simple node identifiers (`A`, `AgentLoop`, `Gate`). Avoid dashes, dots,
  or other special characters in identifiers.
- Quote labels that contain spaces or punctuation: `A["agent loop"]`.
- Prefer `flowchart TD` or `flowchart LR` over the legacy `graph` keyword.
- Keep node counts under 20 per diagram. Larger diagrams should be split.
- For sequence diagrams, name participants with short PascalCase labels.

## Tables vs prose

Use a table when:
- The data has two or more distinct columns with a shared row key.
- The reader will scan for a specific row (e.g. "which crate owns the Guardian?").
- Three or more rows share the same structure.

Use prose when:
- There is only one "column" of information.
- The relationships between items are narrative, not structural.
- Fewer than three items are being compared.

Do not use a table with a single data row. Do not use a table to lay out two
unrelated facts side by side.

## Version accuracy

Every doc must be accurate to the current release (currently v0.8.3). Do not
describe removed subsystems (e.g. tier-based blocking, pre-v0.6 approval flow)
as current. It is fine — often necessary — to mention a deprecated feature in
historical context; make the tense explicit: "Before v0.6.0, Ryvos used …".

When a feature is newly added, reference the CHANGELOG entry rather than
inventing a release date.

## What not to write

- No "Overview" sections that duplicate the H1 intro paragraph.
- No "Conclusion" or "Summary" sections at the end of a doc. Stop when the
  content stops.
- No "TODO" markers in committed docs. Either write the content or link to an
  issue tracking the gap.
- No absolute file paths to the author's machine.
- No screenshots of the Web UI. Use Mermaid or describe the UI in text; images
  age poorly and inflate the repo.
