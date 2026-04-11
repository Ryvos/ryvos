# Reflexion

**[Reflexion](../glossary.md#reflexion)** is Ryvos's answer to the "stuck
on the same tool" failure mode. When an LLM repeatedly invokes the same
tool with inputs that keep failing, the right response is not to try
harder — it is to *change strategy*. The agent does not figure that out on
its own. Reflexion figures it out for the agent by counting consecutive
failures per tool, and after `N` of them it injects a user-role message
into the conversation that spells out what has been failing and suggests
a different approach. The injected message is advisory: it never blocks
the run, never forces a tool choice, never rewrites history. It is just a
nudge.

Reflexion is distinct from the **[Guardian](../glossary.md#guardian)**'s
**[doom loop](../glossary.md#doom-loop)** detector. Doom-loop detection is
turn-level and ephemeral — the Guardian watches a rolling window of tool
calls for identical JSON fingerprints and fires a one-shot hint. Reflexion
is tool-level and persistent — failure counts are kept across turns within
a run by `FailureTracker`, and historical patterns are queried from a
SQLite `FailureJournal` that spans runs. Doom loop catches "same tool,
same args, three times in a row"; Reflexion catches "bash has failed three
times, try something else".

This document walks `crates/ryvos-agent/src/healing.rs:1-469`,
`crates/ryvos-agent/src/intelligence.rs:222-289`, and the injection point
at `crates/ryvos-agent/src/agent_loop.rs:1034-1108`.

## Two trackers, two lifetimes

Reflexion uses two counter stores with different scopes.

`FailureTracker` in `crates/ryvos-agent/src/intelligence.rs:273` is a
per-run, in-memory `HashMap<String, usize>`. It lives inside the agent
loop on the stack, gets reset when the run ends, and exposes two methods.
See the full definition:

```rust
#[derive(Debug, Default)]
pub struct FailureTracker {
    counts: HashMap<String, usize>,
}

impl FailureTracker {
    pub fn record_success(&mut self, tool_name: &str) {
        self.counts.remove(tool_name);
    }

    pub fn record_failure(&mut self, tool_name: &str) -> usize {
        let count = self.counts.entry(tool_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }
}
```

Three things to notice. First, `record_success` *removes* the entry,
which means a single success fully resets the counter to zero. A tool that
failed twice, then succeeded, then failed again reports its new failure
count as one, not three. This is the critical semantic — "consecutive" is
defined strictly. Second, `record_failure` returns the new count so the
caller can compare against the threshold without a second map read. Third,
there is no pruning: a tool that fails, then is never retried, sits in the
map forever. Since the tracker is per-run and runs are bounded by
`max_turns`, this is fine.

`FailureJournal` in `crates/ryvos-agent/src/healing.rs:41` is the
persistent counterpart. It owns a SQLite connection and holds three
tables.

```rust
CREATE TABLE IF NOT EXISTS failure_journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    session_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    error TEXT NOT NULL,
    input_summary TEXT NOT NULL,
    turn INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS success_journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    session_id TEXT NOT NULL,
    tool_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS decisions (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn INTEGER NOT NULL,
    description TEXT NOT NULL,
    chosen_option TEXT NOT NULL,
    alternatives_json TEXT NOT NULL DEFAULT '[]',
    outcome_json TEXT
);
```

The schema lives in the `execute_batch` call at
`crates/ryvos-agent/src/healing.rs:56`. The database is `healing.db` in
the Ryvos data directory, opened in WAL mode. The three tables serve three
different purposes:

- `failure_journal` is the reflexion source-of-truth: every tool failure
  ever observed, with enough context to recognize similar future failures.
- `success_journal` is a lightweight counter store used for health
  reporting (the `ryvos health` CLI and the web UI dashboard). Each entry
  is just a timestamp plus tool name — no result payload.
- `decisions` is the decision audit trail described later in this
  document, not specific to reflexion but stored in the same file because
  both are "what did the agent do and how did it go" data.

## Recording failures and successes

The `FailureRecord` struct is the write unit for `failure_journal`. See
`crates/ryvos-agent/src/healing.rs:30`:

```rust
pub struct FailureRecord {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub tool_name: String,
    pub error: String,
    pub input_summary: String,
    pub turn: usize,
}
```

All six fields are written on every failure. `input_summary` is capped at
200 characters by the caller (see the injection site below) so a bash tool
call with a huge command string does not blow out the row. `error` is the
full error text from the tool result; this is the single most useful field
for reflexion because it is what the next hint will show the agent.

`record` is an unconditional insert — there is no deduplication. Two runs
that both hit `"File not found: /etc/foo"` will produce two rows, because
knowing that a failure happened twice is information. The index on
`(tool_name, timestamp)` at `crates/ryvos-agent/src/healing.rs:70` keeps
the `find_patterns` query cheap even as the table grows.

`record_success` writes a row to `success_journal`:

```rust
pub fn record_success(&self, session_id: &str, tool_name: &str) -> Result<(), String> {
    let conn = self.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO success_journal (timestamp, session_id, tool_name) VALUES (?1, ?2, ?3)",
        params![Utc::now().to_rfc3339(), session_id, tool_name],
    )
    .map_err(|e| format!("Failed to record success: {}", e))?;
    Ok(())
}
```

Success rows are used only by `tool_health` — reflexion itself reads
nothing from `success_journal`.

## Querying past patterns

`find_patterns` is the read path that powers rich reflexion hints. See
`crates/ryvos-agent/src/healing.rs:135`:

```rust
pub fn find_patterns(&self, tool: &str, limit: usize) -> Result<Vec<FailureRecord>, String> {
    let conn = self.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT timestamp, session_id, tool_name, error, input_summary, turn
             FROM failure_journal
             WHERE tool_name = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
        )
        .map_err(|e| format!("Failed to query patterns: {}", e))?;
    /* ... row mapping ... */
}
```

The query is plain: filter by tool name, order by most recent, cap at
`limit`. No pattern matching over errors, no input-similarity scoring, no
clustering. The hint generator (next section) takes the raw list and the
agent does the pattern recognition. This is a deliberate simplification:
structured clustering would require engineering effort and tuning, while
handing a list of three recent errors to an LLM and asking it to spot the
pattern works essentially for free.

`limit` is always 5 at the call site (the agent loop passes `5`). That is
the truncation applied to the journal rows; the hint generator further
truncates to the top 3 inside `reflexion_hint_with_history`, because a
hint with ten past errors in it is longer than useful.

## Hint generation

There are two hint constructors. The simple one is in
`intelligence.rs:222`:

```rust
pub fn reflexion_hint(tool_name: &str, failure_count: usize) -> ChatMessage {
    let text = format!(
        "The tool `{}` has failed {} times in a row. \
         Try a different approach or use a different tool to accomplish the task.",
        tool_name, failure_count
    );
    ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text { text }],
        timestamp: Some(chrono::Utc::now()),
        metadata: None,
    }
}
```

No history, just the count and the suggestion. This is used when
`FailureJournal::find_patterns` returns an empty vector — for example on a
fresh install with no prior runs, or for a tool that has never failed
before in this database.

The rich one is `reflexion_hint_with_history` at
`crates/ryvos-agent/src/healing.rs:312`:

```rust
pub fn reflexion_hint_with_history(
    tool_name: &str,
    failure_count: usize,
    past: &[FailureRecord],
) -> ryvos_core::types::ChatMessage {
    let mut text = format!(
        "The tool `{}` has failed {} times in a row.",
        tool_name, failure_count
    );

    if !past.is_empty() {
        text.push_str("\n\nIn past sessions, this tool failed with these patterns:");
        for (i, rec) in past.iter().take(3).enumerate() {
            text.push_str(&format!(
                "\n  {}. [{}] Error: {}",
                i + 1,
                rec.timestamp.format("%Y-%m-%d %H:%M"),
                truncate_str(&rec.error, 150),
            ));
        }
        text.push_str(
            "\n\nBased on these patterns, try a different approach or use a different tool.",
        );
    } else {
        text.push_str(" Try a different approach or use a different tool to accomplish the task.");
    }

    ryvos_core::types::ChatMessage { /* ... */ }
}
```

The structure is: one sentence opener, then a numbered list of up to three
past errors with timestamps and 150-char truncated error strings, then a
closing sentence asking the agent to adapt. Both hints use `Role::User`
because that is how the agent loop expects to inject follow-up messages;
an assistant-role message would break conversation structure.

Why is user role correct? Because from the model's perspective, receiving
a message that says "the tool you just tried has been failing in these
ways, please reconsider" is functionally identical to a human operator
looking over its shoulder and saying the same thing. The LLM's next
generation will weight the hint alongside the rest of the conversation and
adjust. There is no special handling on the Ryvos side — no magical
metadata, no tool-level reweighting — just a plain user message that
happens to carry useful context.

## Injection point in the agent loop

The dispatch site is in `process_tool_results` at
`crates/ryvos-agent/src/agent_loop.rs:1034`:

```rust
// Process results: compact output, track failures, build content blocks
let threshold = self.config.agent.reflexion_failure_threshold;
let mut tool_result_blocks = Vec::new();

let tool_exec_elapsed_ms = tool_exec_start.elapsed().as_millis() as u64;
for (idx, (_name, _id, tool_result)) in tool_results.iter().enumerate() {
    // Backfill decision outcome
    if let (Some(ref journal), Some(dec_id)) = (&self.journal, decision_ids.get(idx)) {
        let outcome = DecisionOutcome {
            tokens_used: 0, // not tracked per-tool
            latency_ms: tool_exec_elapsed_ms,
            succeeded: !tool_result.is_error,
        };
        journal.update_decision_outcome(dec_id, &outcome).ok();
    }
}
```

The threshold comes from `config.agent.reflexion_failure_threshold` which
defaults to 3. It is the number of consecutive failures of the same tool
that must occur before a hint is injected. The first pass over the results
handles decision outcome backfill (covered in the next section).

The second pass is where the failure tracker runs. See
`crates/ryvos-agent/src/agent_loop.rs:1064`:

```rust
// Track failures and inject reflexion hint when threshold exceeded
if tool_result.is_error {
    let count = failure_tracker.record_failure(&name);
    // Persist to journal
    if let Some(ref journal) = self.journal {
        let input_summary = serde_json::to_string(
            &tool_calls
                .iter()
                .find(|tc| tc.name == name)
                .map(|tc| &tc.input_json)
                .unwrap_or(&String::new()),
        )
        .unwrap_or_default();
        journal
            .record(FailureRecord {
                timestamp: chrono::Utc::now(),
                session_id: session_id.0.clone(),
                tool_name: name.clone(),
                error: tool_result.content.clone(),
                input_summary: input_summary.chars().take(200).collect(),
                turn,
            })
            .ok();
    }
    if count >= threshold {
        // Query past patterns for smarter hint
        let past = self
            .journal
            .as_ref()
            .and_then(|j| j.find_patterns(&name, 5).ok())
            .unwrap_or_default();
        let hint = if past.is_empty() {
            reflexion_hint(&name, count)
        } else {
            reflexion_hint_with_history(&name, count, &past)
        };
        messages.push(hint);
    }
} else {
    failure_tracker.record_success(&name);
    // Record success for health tracking
    if let Some(ref journal) = self.journal {
        journal.record_success(&session_id.0, &name).ok();
    }
}
```

Control flow per tool result:

1. If the result is an error, increment the in-memory tracker and get
   back the new count.
2. If a journal is configured, persist a `FailureRecord` with
   timestamp, session id, tool name, full error, input summary (JSON-
   stringified, truncated to 200 chars), and the current turn number.
3. If the count has reached the threshold, query the journal for
   `find_patterns(tool, 5)` — the five most recent prior failures of this
   tool. If the query returns nothing, use the simple hint; otherwise use
   the rich hint. Push the hint onto the message list.
4. If the result is not an error, clear the tracker entry for that tool
   and record a success row in the journal.

Step 2 is unconditional — every failure goes to the journal, not just the
ones that cross the threshold. This is important because the next run's
reflexion might query failures from *this* run even though this run
never got rich enough to fire its own hint. The threshold is a hint-firing
gate, not a data-capture gate.

Step 3 pushes the hint straight into `messages`, which is the turn
buffer. The very next iteration of the outer turn loop will see the hint
as part of the conversation and the LLM will generate its response against
a context that includes it. There is no delay, no extra event emission,
no consent mechanism — reflexion is pure context injection.

The reset in step 4 uses `HashMap::remove` which means a tool that
failed twice and then succeeded is back at zero. This is why the hint
only ever reflects *consecutive* failures; any break in the chain clears
the runway.

## Decision tracking

The `decisions` table and the `Decision`/`DecisionOutcome` types together
form a structured record of every tool choice the agent made. This is not
reflexion strictly speaking — reflexion only reads from `failure_journal` —
but it lives in the same database and is populated by the same loop, and
the data is useful for the same class of post-hoc analysis.

The write happens at `crates/ryvos-agent/src/agent_loop.rs:931`:

```rust
// Record decisions for tool calls
let decision_ids: Vec<String> = tool_calls
    .iter()
    .map(|tc| {
        let decision = Decision {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            session_id: session_id.0.clone(),
            turn,
            description: format!("Tool call: {}", tc.name),
            chosen_option: tc.name.clone(),
            alternatives: if tool_calls.len() > 1 {
                tool_calls
                    .iter()
                    .filter(|other| other.id != tc.id)
                    .map(|other| DecisionOption {
                        name: other.name.clone(),
                        confidence: None,
                    })
                    .collect()
            } else {
                vec![]
            },
            outcome: None,
        };
        if let Some(ref journal) = self.journal {
            journal.record_decision(&decision).ok();
        }
        self.event_bus.publish(AgentEvent::DecisionMade {
            decision: decision.clone(),
        });
        decision.id
    })
    .collect();
```

One decision per tool call in the batch. The `alternatives` list is the
*other* tool calls in the *same batch* — not every tool the agent could
have picked, but every tool it actually did pick in parallel. This is a
narrower interpretation of "alternatives" than the docstring might
suggest, and it is what the code actually does. For a single-tool batch,
`alternatives` is empty.

`record_decision` inserts the row with `outcome_json = NULL`. The outcome
is backfilled after execution in the loop at
`crates/ryvos-agent/src/agent_loop.rs:1038`:

```rust
if let (Some(ref journal), Some(dec_id)) = (&self.journal, decision_ids.get(idx)) {
    let outcome = DecisionOutcome {
        tokens_used: 0, // not tracked per-tool
        latency_ms: tool_exec_elapsed_ms,
        succeeded: !tool_result.is_error,
    };
    journal.update_decision_outcome(dec_id, &outcome).ok();
}
```

`update_decision_outcome` at `crates/ryvos-agent/src/healing.rs:197`
is a simple `UPDATE ... SET outcome_json = ? WHERE id = ?`. Note that
`tokens_used` is hardcoded to zero — per-tool token tracking would
require threading usage numbers through the executor, which the current
implementation does not do. `latency_ms` is the elapsed time for the
*whole* tool batch, not the individual call, because batch execution is
parallel and splitting the latency per tool would be meaningless.

`load_decisions` at `crates/ryvos-agent/src/healing.rs:213` is the read
counterpart; the web UI's "Decisions" view and the `debugging-runs` guide
both use it to retrieve the decision log for a session.

## tool_health

`tool_health` at `crates/ryvos-agent/src/healing.rs:255` aggregates
successes and failures per tool over a time window:

```rust
pub fn tool_health(
    &self,
    since: DateTime<Utc>,
) -> Result<HashMap<String, (usize, usize)>, String> {
    /* COUNT from success_journal, then COUNT from failure_journal,
       stitched together in a single HashMap */
}
```

The return value maps `tool_name → (successes, failures)` for every tool
that had any activity since `since`. This is the data powering the
`ryvos health` CLI command and the "Tool Health" panel in the web UI
dashboard. A tool that is 99 successful / 1 failed shows as green, a tool
that is 5/10 shows as red. Reflexion and health reporting share the same
raw data but interpret it at different granularities: reflexion looks at
a single tool's consecutive recent failures (what happens inside a run),
health looks at every tool's aggregate reliability (what has been
happening across runs).

## Distinction from doom-loop

The doom loop detector described in [guardian.md](guardian.md) catches a
narrower pattern. Guardian fingerprints each tool call as
`(name, input_json)` and fires when three fingerprints in the last N
calls are identical. Reflexion does not look at inputs at all — three
calls to bash with different commands, all failing, still count as three
bash failures and still trip the threshold. Conversely, three calls to
bash with the *same* command that all succeed do not concern reflexion at
all (success clears the counter), but they do concern the doom-loop
detector because calling the same thing twice is often a model bug even
when it works. The two systems address adjacent but non-overlapping
pathologies, which is why they exist separately.

## Cross-references

- [agent-loop.md](agent-loop.md) — the loop that drives the failure
  tracker and injects hints.
- [guardian.md](guardian.md) — the doom-loop detector, the reactive
  counterpart to reflexion.
- [safety-memory.md](safety-memory.md) — a different self-learning store
  that also reads the audit trail.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — crate overview.
- [../architecture/persistence.md](../architecture/persistence.md) — the
  `healing.db` schema in the persistence catalogue.
