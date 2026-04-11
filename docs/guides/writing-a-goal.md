# Writing a goal

## When to use this guide

A **[goal](../glossary.md#goal)** is a structured task definition with
weighted success criteria and optional constraints on time, cost,
safety, scope, and quality. When a run carries a goal, the
**[Director](../glossary.md#director)** takes over from the standard
ReAct **[agent runtime](../glossary.md#agent-runtime)**: it generates a
DAG of subtasks, executes them, evaluates the output against the
goal's criteria via the **[Judge](../glossary.md#judge)**, and on
failure diagnoses the cause, evolves the plan, and retries up to
`max_evolution_cycles` times before escalating. ADR-009 records the
OODA rationale.

Choose a goal when the task has an **observable outcome** the Judge
can evaluate — a file must exist, a test must pass, a summary must
contain certain phrases, a pull request must be opened. Choose the
reactive agent loop (`ryvos run "..."`) instead for open-ended
conversation, exploratory debugging, or any task where the definition
of done is subjective and best left to the user.

Goals are defined in TOML. They can live inline in a cron job, in a
standalone goal file passed to the gateway's `/api/goals/run`
endpoint, or embedded in a request body. This guide walks the fields,
the criterion and constraint taxonomies, the Judge's verdict
interpretation, and the Director's plan-evolution behavior.

## TOML structure

The full shape of a `Goal` is defined in
`crates/ryvos-core/src/goal.rs`. A goal is a description, a list of
success criteria with weights, a list of constraints, a success
threshold, and a version counter that the Director bumps on each
evolution cycle.

```toml
description = "Summarize every email that arrived today in one paragraph each"
success_threshold = 0.9

[[success_criteria]]
type = "OutputContains"
value = "Subject:"
case_sensitive = false
weight = 0.3

[[success_criteria]]
type = "OutputContains"
value = "From:"
case_sensitive = false
weight = 0.3

[[success_criteria]]
type = "LlmJudge"
prompt = "Does the output contain one paragraph per email, each one summarizing its contents in under 60 words?"
weight = 0.4

[[constraints]]
category = "Time"
kind = "Hard"
description = "Must complete within 5 minutes"
max_seconds = 300

[[constraints]]
category = "Cost"
kind = "Soft"
description = "Prefer runs under 50 cents"
max_cents = 50
```

Weights are normalized across whichever criteria actually ran, so a
goal where the `LlmJudge` criterion errored out still produces a
meaningful score from the remaining deterministic checks. A run passes
when two conditions hold: no hard constraint is violated, **and** the
weighted score of evaluated criteria clears `success_threshold`. A
soft constraint violation scores against the goal but does not by
itself fail it.

## Success criteria

`SuccessCriterion` has four types:

- **`OutputContains`** takes a `value` string and an optional
  `case_sensitive` flag. It passes if the final assistant message
  contains the value. This is the fastest check — deterministic,
  synchronous, no LLM call. Use it for stable tokens like `"Success:"`,
  `"OK"`, `"Created PR #"`, or any literal the agent should produce on
  a happy path.

- **`OutputEquals`** takes a `value` string. It passes if the output
  is exactly equal. Use it when the agent produces a canonical
  response for success — rarely useful for free-form tasks, but
  valuable for classification or structured-output work.

- **`LlmJudge`** takes a `prompt` string. The Judge sends the prompt
  and the agent's final output to the configured model and asks for a
  pass/fail verdict. Use it when "success" is subjective in a way a
  literal check cannot capture: "Is the summary faithful to the
  source?", "Does the refactored code preserve the original
  behavior?", "Is the explanation appropriate for a non-technical
  reader?". LLM judges are slow and cost tokens, so weight them
  accordingly.

- **`Custom`** is a named hook the caller evaluates. Ryvos does not
  evaluate `Custom` criteria internally; they are a hook point for
  embedding code that brings its own logic.

Each criterion has a `weight` (defaulting to 1.0). The Judge
normalizes weights across evaluated criteria — if three criteria have
weights 0.3, 0.3, 0.4 and the LLM judge errors out, the two
deterministic criteria are renormalized to 0.5 and 0.5. This keeps
goals scoreable even when some criteria cannot be checked.

## Constraints

`Constraint` categorizes soft and hard limits the agent must respect.
A constraint has a `category`, a `kind`, a `description` string, and
category-specific parameters.

- **Categories**: `Time`, `Cost`, `Safety`, `Scope`, `Quality`.
- **Kinds**: `Hard` (violation fails the run immediately) or `Soft`
  (violation scores against the goal but does not by itself fail it).

Typical uses:

- `Time` + `Hard`: "Must finish in 5 minutes." The Director tracks
  wall-clock time and escalates if the budget is exceeded.
- `Cost` + `Soft`: "Prefer runs under 50 cents." The cost tracker
  reports per-run spend; a soft breach is penalized in the score but
  the run continues.
- `Safety` + `Hard`: "No tool calls marked as destructive." The gate's
  **[SafetyMemory](../glossary.md#safetymemory)** annotations let the
  Judge see which calls were flagged.
- `Scope` + `Hard`: "Only touch files under `src/billing/`." The
  Director includes the scope in its plan-generation prompt and the
  Judge checks it at evaluation time.
- `Quality` + `Soft`: "Test coverage must not drop." Paired with a
  `LlmJudge` criterion that runs the coverage tool and compares.

## Example goals

### Simple deterministic goal

```toml
description = "Run cargo fmt and cargo clippy in the repo, then commit the fixes"
success_threshold = 0.8

[[success_criteria]]
type = "OutputContains"
value = "No changes needed"
weight = 0.5

[[success_criteria]]
type = "OutputContains"
value = "committed"
weight = 0.5

[[constraints]]
category = "Time"
kind = "Hard"
max_seconds = 180
```

### LLM-judged refactor goal

```toml
description = "Refactor the billing module to use the new async API while keeping every test passing"
success_threshold = 0.9

[[success_criteria]]
type = "OutputContains"
value = "test result: ok"
weight = 0.3

[[success_criteria]]
type = "LlmJudge"
prompt = "Does the final diff replace synchronous billing calls with async equivalents without introducing new blocking points?"
weight = 0.5

[[success_criteria]]
type = "LlmJudge"
prompt = "Are public API signatures preserved? A Yes means the external surface is unchanged."
weight = 0.2

[[constraints]]
category = "Safety"
kind = "Hard"
description = "No tests may be deleted or skipped"

[[constraints]]
category = "Scope"
kind = "Hard"
description = "Only touch files under crates/billing/"
```

## Triggering a goal run

Three surfaces accept a goal. Use whichever fits the integration.

- **REST**: POST to `/api/goals/run` with the goal TOML in the body
  and an optional `callback_url` for async completion. The gateway
  hands the request off to the Director and returns a run id
  immediately.
- **Cron**: add a `goal` field to a `[[cron.jobs]]` entry in
  `ryvos.toml`. Every fire resolves the schedule, loads the goal, and
  calls `AgentRuntime::run_with_goal`. See
  [../internals/cron-scheduler.md](../internals/cron-scheduler.md) for
  the schedule grammar.
- **CLI**: `ryvos run --goal path/to/goal.toml "initial message"`
  parses the TOML into a `Goal`, then runs the agent with it attached.

Any of these paths dispatches to the same `run_with_goal` entry point
on `AgentRuntime`, which delegates to the Director when
`[agent.director]` is enabled in config.

## Judge verdicts and Director evolution

After every Director cycle, the Judge returns one of four verdicts:

- **`Accept(confidence)`** — the goal is satisfied. The Director
  returns the successful output immediately.
- **`Retry(reason, hint)`** — criteria not yet satisfied but the
  Director has levers to pull. The hint is injected as a user message
  at the start of the next cycle, and the plan evolves: nodes are
  re-executed or rewritten based on the reason.
- **`Escalate(reason)`** — stuck in a way the agent cannot resolve
  (hard constraint breached, repeated same-failure pattern, evolution
  budget exhausted). The Director returns the partial output with a
  clear explanation and emits an `EvolutionTriggered` event so the
  user can intervene.
- **`Continue`** — no strong opinion yet. The Director keeps going.

Plan evolution happens up to `max_evolution_cycles` times (default 3).
Each cycle publishes `GraphGenerated`, `NodeComplete`, and
`JudgeVerdict` events on the **[EventBus](../glossary.md#eventbus)**
so the Web UI, the TUI, and the audit trail can follow along in real
time. The full state machine is in
[../internals/director-ooda.md](../internals/director-ooda.md).

## Verification

1. POST the goal to `/api/goals/run`:

   ```bash
   curl -X POST http://localhost:3000/api/goals/run \
     -H "Content-Type: application/json" \
     -H "X-API-Key: your-operator-key" \
     -d @goal.toml
   ```

2. Watch events on the WebSocket channel `/api/events` or open the
   Web UI Goals page. The first event is `RunStarted`, then
   `GraphGenerated` as the Director emits its plan, then
   `NodeComplete` for each subtask.
3. On failure, look for `JudgeVerdict` events with kind `Retry` or
   `Escalate`, then `EvolutionTriggered` as the Director rewrites the
   plan.
4. On success, the final `RunComplete` event carries the accepted
   output and confidence score.
5. Review the audit trail: `ryvos audit query --session {session_id}`
   lists every tool call the Director dispatched and every
   SafetyMemory lesson that was available at the time.

For the full crate-level reference on goals and the Judge, read
[../crates/ryvos-core.md](../crates/ryvos-core.md) (the `goal` module)
and [../internals/judge.md](../internals/judge.md). For how the
Director's DAG executor runs nodes in parallel with handoff context,
read [../internals/graph-executor.md](../internals/graph-executor.md).
The REST shape of `/api/goals/run` is documented in
[../api/gateway-rest.md](../api/gateway-rest.md).
