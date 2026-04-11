# Customizing the soul

## When to use this guide

Ryvos builds every system prompt from a three-layer
**[onion context](../glossary.md#onion-context)**: an innermost
**identity layer** that defines who the agent is and who it serves, a
**narrative layer** of recent memory and sustained context, and an
outermost **focus layer** carrying the current goal and constraints.
The identity layer is where `SOUL.md` lives. Editing it is how you
change the agent's tone, proactivity, values, relationship to the
user, and voice without touching code.

Use this guide when the agent's responses feel generic, too formal,
too chatty, off-brand, out of character, or when the agent does not
know enough about the operator to make useful judgments on its
behalf. The **[SOUL.md](../glossary.md#soulmd)** file is loaded into
every run's system prompt and never pruned, so changes take effect on
the next `ryvos run` call with no restart needed.

This guide walks the soul interview, the file layout, manual editing,
the companion `IDENTITY.md` file, how these files are injected into
the system prompt, and how the evolving **[SafetyMemory](../glossary.md#safetymemory)**
and daily logs work alongside them over time.

## The onion context in one paragraph

The `ContextBuilder` in `crates/ryvos-agent/src/context.rs` assembles
the system prompt fresh at the start of every turn. The innermost
layer reads `SOUL.md` and `IDENTITY.md` from the workspace and prepends
them to the default system prompt. The narrative layer loads
`AGENTS.toml`, `TOOLS.md`, recent daily logs, and
**[Viking](../glossary.md#viking)** recall fragments relevant to the
current user message. The focus layer adds the immediate goal, any
hard and soft constraints, and just-in-time tool documentation. Inner
layers rarely change; outer layers are rebuilt every turn. See
[../architecture/context-composition.md](../architecture/context-composition.md)
for the full assembly rules.

The soul file sits inside the innermost layer, which is why editing it
reshapes every response the agent produces.

## Running the soul interview

The fastest path to a personalized SOUL.md is the interactive
interview:

```bash
ryvos soul
```

This runs a 15-question onboarding that covers four areas:

- **Four questions about you, the operator.** Your name, how you want
  to be addressed, what you do for a living, and what kind of help you
  want from the agent (reactive, proactive, watchdog, pair
  programmer).
- **Five questions about tone and voice.** Formal vs casual, verbose
  vs terse, professional vs playful, use of humor, preferred pronouns
  and greetings, how the agent should respond to praise or criticism.
- **Three questions about projects and context.** The main projects
  the agent will assist with, the dev environment (editor, shell,
  language stack), and any existing personas in other tools the agent
  should be consistent with.
- **Three questions about character.** The values the agent should
  prioritize (honesty, speed, thoroughness, kindness), the quirks or
  catchphrases it can use, and the limits it should respect.

The answers are compiled into a `SOUL.md` file written to the
workspace root. `ryvos init` bundles the soul interview with broader
onboarding — config setup, channel tokens, MCP server wiring, and a
first heartbeat run — so if this is a fresh install, start there.

## File structure

A generated `SOUL.md` has five top-level sections. Manual edits are
expected; the file is meant to be a living document.

```markdown
# Soul

## Persona
A short paragraph describing the agent. First person, present tense.
"I am a calm, precise collaborator who prefers to act rather than
narrate. I work for Alex, who values terse answers and quick action."

## Voice
Three or four bullet points that capture tone.
- Casual but precise. Never stuffy, never filler.
- Use code when code is the answer; use prose when it is not.
- Short paragraphs. One idea per line.
- No hedging unless uncertainty is the point.

## Values
What the agent optimizes for in any tradeoff.
- Correctness over speed.
- Reversibility over cleverness.
- Transparency when a choice is ambiguous.

## Quirks
Catchphrases, preferred greetings, specific words to avoid.
- Greet with "On it."
- Never say "powerful" or "cutting-edge".
- Use football metaphors when the operator is venting.

## Relationship to user
How the agent thinks about the operator.
- I trust Alex's judgment on priorities.
- I will push back on suggestions that contradict stated values.
- I keep a running mental model of ongoing projects in Viking memory.
```

The generator emits sensible defaults for every section. Editing the
file directly after the interview is normal and expected.

## The `IDENTITY.md` companion

`IDENTITY.md` is the sibling file that records the agent's
self-awareness: its name (if given), its self-described limitations,
and scoped facts it should treat as true about itself. A typical
file has three sections — Name, Nature, and Limits — covering who
the agent is, that it is a long-lived daemon with persistent
memory, and the boundaries it respects (no real-time information
without a tool, no access to unauthorized accounts, no impersonation
in outbound messages). The two files are injected together.

Both are optional. A workspace with only `SOUL.md` works; a
workspace with neither falls back to the `DEFAULT_SYSTEM_PROMPT` at
the top of `crates/ryvos-agent/src/context.rs`.

## How these files reach the model

At the start of every turn, the agent loop calls
`ContextBuilder::build` in `crates/ryvos-agent/src/context.rs`. The
builder:

1. Reads `SOUL.md` from the workspace root. If the file is missing,
   the identity layer is empty and the default system prompt provides
   the base tone.
2. Reads `IDENTITY.md` from the same place. If present, the contents
   are appended to the identity layer.
3. Loads the narrative layer: `AGENTS.toml`, `TOOLS.md`, the last two
   daily logs, Viking recall fragments, and
   **[SafetyMemory](../glossary.md#safetymemory)** lessons relevant to
   the tools available on this run.
4. Builds the focus layer: the current goal (if any), any constraints,
   and tool schemas.
5. Concatenates the three layers into a single `ChatMessage` with
   role `System` and hands it to the LLM as the first message of the
   conversation.

No part of `SOUL.md` is truncated, summarized, or pruned. It is
treated as foundational — if the file is long, every turn pays the
token cost. Keep it concise (under 500 words is a good target) and
push longer operator-specific context into Viking memory under
`viking://user/profile/`, which the narrative layer pulls on demand.

## Editing manually vs re-running the interview

Three patterns work well:

- **Small tweaks.** Open the file, edit the bullet points, save. The
  next `ryvos run` picks up the change. No restart needed because the
  context is built fresh per turn.
- **Major redirection.** Re-run `ryvos soul` to get a fresh set of
  answers. The interview overwrites the existing file after prompting
  for confirmation; keep a git-tracked backup if you want the old
  version back.
- **Gradual evolution.** Let the agent write to Viking memory
  (`viking://user/preferences/`) as it learns about you over time, and
  periodically distill the accumulated notes into SOUL.md edits
  manually. This is the pattern heavy users converge on after a few
  weeks: the interview starts you off, Viking captures the drift, and
  SOUL.md is updated during a reflective moment rather than a fresh
  interview.

## The safety constitution and memory

The soul file does not need to encode safety rules. The default
identity layer already carries the seven-principle safety constitution
— Preservation, Intent Match, Proportionality, Transparency,
Boundaries, Secrets, Learning, plus a Learning principle — as the
foundation of [passthrough security](../glossary.md#passthrough-security).
`SafetyMemory` accumulates lessons from outcomes and injects the
top-ranked ones into the system prompt alongside `SOUL.md`. Tone and
personality sit in the soul; judgment and restraint evolve in
SafetyMemory and the constitution. Do not try to mirror safety rules
in `SOUL.md`; the constitution is already there and
[configuring-safety.md](configuring-safety.md) covers how to
customize it.

## Example: formal to casual

Suppose the default interview produced a SOUL.md with a professional
Voice section. Rewriting the bullets from "Professional and
courteous; always greet formally; complete sentences" to "Casual,
direct, terse; skip greetings; fragments fine; elaborate only on
follow-up" and saving the file is enough. Run
`ryvos run "what is 2+2?"` — the first response should be noticeably
shorter. If not, the onion context has not rebuilt; check that the
daemon is reading from the right workspace with `ryvos doctor`.

## Verification

1. Edit `SOUL.md` to change a specific phrase. For a quick test, add
   a line to Voice like "Always start with 'On it.'".
2. Run `ryvos run "say hi"`. The response should contain the new
   phrase.
3. Check `ryvos doctor` — it reports which `SOUL.md` path the agent
   loaded. Mismatches are almost always a workspace directory issue.
4. Review the run log at `~/.ryvos/logs/{session}/{timestamp}.jsonl`.
   The first entry captures the assembled system prompt (at log level
   3) so you can confirm the identity layer matches the file on disk.

For the full context composition rules, read
[../architecture/context-composition.md](../architecture/context-composition.md).
For the agent loop's per-turn context build, read
[../internals/agent-loop.md](../internals/agent-loop.md). For the
crate-level reference that owns the builder, see
[../crates/ryvos-agent.md](../crates/ryvos-agent.md). To customize the
safety layer itself rather than the tone layer, read
[configuring-safety.md](configuring-safety.md).
