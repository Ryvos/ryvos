# Judge

When an agent run carries a **[goal](../glossary.md#goal)**, someone has to
decide whether the final output actually satisfies that goal. That decision
is what the **[Judge](../glossary.md#judge)** produces. It is a two-level
evaluator: Level 0 is a fast deterministic check that runs in microseconds,
Level 2 is a slow LLM check that costs an API call. The combined
`Judge::evaluate` method tries the fast path first and falls back to the
slow path only when the goal cannot be decided without a language model in
the loop.

The Judge is distinct from the **[Director](../glossary.md#director)** and
from the `GoalEvaluator` in `evaluator.rs`. The Director is the orchestrator
that replaces the agent loop for goal-bearing runs; the Judge is the verdict
producer that the agent loop *and* the Director both call at the end of a
cycle to decide what happens next. The `GoalEvaluator` is a lower-level
helper that the Director uses to compute per-criterion `LlmJudge` scores;
the Judge uses a single combined prompt and returns one
**[verdict](../glossary.md#verdict)**.

This document walks `crates/ryvos-agent/src/judge.rs:1-351`, the goal
evaluation primitives in `crates/ryvos-core/src/goal.rs:167-254`, the
LLM-as-judge details in `crates/ryvos-agent/src/evaluator.rs:108-222`, and
the dispatch code in `crates/ryvos-agent/src/agent_loop.rs:811-850` that
consumes the verdict and drives the next turn.

## Struct and entry points

The `Judge` struct is tiny — it owns the LLM client and the model config,
and nothing else. See `crates/ryvos-agent/src/judge.rs:17`:

```rust
pub struct Judge {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
}
```

The same model config used for the run is reused for the verdict call. In
practice this means a Sonnet-driven run is judged by Sonnet; there is no
mechanism in the current Judge to use a different or weaker model for the
verdict. The three public methods are `fast_check` (associated, no
`self`), `llm_judge` (method), and `evaluate` (method — the combinator).
`new` is a trivial constructor.

## Level 0: fast_check

`fast_check` is the deterministic path. It is the fast exit that avoids an
extra LLM round trip whenever the goal can be decided from pattern matching
alone. See `crates/ryvos-agent/src/judge.rs:32`:

```rust
pub fn fast_check(output: &str, goal: &Goal) -> Option<Verdict> {
    let has_llm_criteria = goal
        .success_criteria
        .iter()
        .any(|c| matches!(c.criterion_type, CriterionType::LlmJudge { .. }));

    if has_llm_criteria {
        return None; // Need LLM evaluation
    }

    let results = goal.evaluate_deterministic(output);
    let eval = goal.compute_evaluation(results, vec![]);

    if eval.passed {
        Some(Verdict::Accept {
            confidence: eval.overall_score,
        })
    } else {
        let failed: Vec<String> = eval
            .criteria_results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.reasoning.clone())
            .collect();

        Some(Verdict::Retry {
            reason: format!(
                "Score {:.0}% < threshold {:.0}%",
                eval.overall_score * 100.0,
                goal.success_threshold * 100.0,
            ),
            hint: if failed.is_empty() {
                "Try a different approach.".to_string()
            } else {
                format!("Failed: {}", failed.join("; "))
            },
        })
    }
}
```

Three control-flow branches matter. First, the early `None` return when any
criterion is `LlmJudge`: fast_check refuses to answer a goal that has any
LLM-evaluated criteria because the deterministic score would be incomplete.
The caller (`evaluate`) treats `None` as "fall through to `llm_judge`".
Second, the `Accept` branch: when all deterministic criteria pass and the
weighted score clears `success_threshold`, fast_check returns an `Accept`
carrying the score as `confidence`. The score *is* the confidence in the
deterministic case; there is no hidden uncertainty. Third, the `Retry`
branch: reason is a rendered percentage against threshold, hint is a
concatenated list of the failing criteria's reasoning strings. If no
criterion failed but the threshold still wasn't met (this shouldn't
happen given compute_evaluation's logic, but the code is defensive), the
hint degrades to the generic "Try a different approach."

Note that fast_check never returns `Escalate` or `Continue`. Those verdicts
only come from the LLM judge. A deterministic check can only accept or
retry.

## Goal evaluation primitives

fast_check delegates the actual scoring to two methods on `Goal`:
`evaluate_deterministic` and `compute_evaluation`. Both live in
`crates/ryvos-core/src/goal.rs:167-254`.

`evaluate_deterministic` walks the criteria list and evaluates only the two
deterministic variants. See `crates/ryvos-core/src/goal.rs:170`:

```rust
pub fn evaluate_deterministic(&self, output: &str) -> Vec<CriterionResult> {
    self.success_criteria
        .iter()
        .filter_map(|c| match &c.criterion_type {
            CriterionType::OutputContains {
                pattern,
                case_sensitive,
            } => {
                let found = if *case_sensitive {
                    output.contains(pattern)
                } else {
                    output.to_lowercase().contains(&pattern.to_lowercase())
                };
                Some(CriterionResult {
                    criterion_id: c.id.clone(),
                    score: if found { 1.0 } else { 0.0 },
                    passed: found,
                    reasoning: if found {
                        format!("Output contains '{}'", pattern)
                    } else {
                        format!("Output does not contain '{}'", pattern)
                    },
                })
            }
            CriterionType::OutputEquals { expected } => {
                let matches = output.trim() == expected.trim();
                Some(CriterionResult { /* ... */ })
            }
            CriterionType::LlmJudge { .. } | CriterionType::Custom { .. } => None,
        })
        .collect()
}
```

Two observations. `OutputContains` honours a `case_sensitive` flag that
defaults to `false`; most goal authors want "the answer mentioned the word
'success'" to match regardless of capitalization. `OutputEquals` trims
both sides before comparing — trailing whitespace is not a reason for a
goal to fail. `LlmJudge` and `Custom` return `None`, meaning the resulting
vector is strictly smaller than the full criteria list when those variants
are present.

`compute_evaluation` turns a vector of per-criterion results and a vector
of constraint violations into a single `GoalEvaluation`. See
`crates/ryvos-core/src/goal.rs:213`:

```rust
pub fn compute_evaluation(
    &self,
    criteria_results: Vec<CriterionResult>,
    constraint_violations: Vec<ConstraintViolation>,
) -> GoalEvaluation {
    let total_weight: f64 = self
        .success_criteria
        .iter()
        .filter(|c| criteria_results.iter().any(|r| r.criterion_id == c.id))
        .map(|c| c.weight)
        .sum();

    let weighted_score = if total_weight > 0.0 {
        criteria_results
            .iter()
            .filter_map(|r| {
                self.success_criteria
                    .iter()
                    .find(|c| c.id == r.criterion_id)
                    .map(|c| r.score * c.weight)
            })
            .sum::<f64>()
            / total_weight
    } else {
        0.0
    };

    let has_hard_violation = constraint_violations
        .iter()
        .any(|v| v.kind == ConstraintKind::Hard);

    let passed = !has_hard_violation && weighted_score >= self.success_threshold;

    GoalEvaluation {
        overall_score: weighted_score,
        passed,
        criteria_results,
        constraint_violations,
    }
}
```

Two subtleties. First, `total_weight` only counts criteria that actually
have a result in the input vector. This is why passing in a partial set of
results (for example, the output of `evaluate_deterministic` when
`LlmJudge` criteria were present) still produces a valid weighted score
over the subset — the criteria that were not evaluated simply drop out of
the denominator. It is also why fast_check's early `None` return on
`LlmJudge` is correct: accepting a goal based on a subscore would be
structurally wrong, because the LLM criterion might have weighted heavily.
Second, `passed` has the hard-constraint override: any hard
**[constraint](../glossary.md#goal)** violation forces `passed = false`
regardless of score. Soft violations are recorded in the result but do not
affect `passed`.

## Level 2: llm_judge

`llm_judge` is the slow path. It formats the full conversation, sends it
to the LLM with a rubric, and parses a structured verdict out of the JSON
response. See `crates/ryvos-agent/src/judge.rs:75`:

```rust
pub async fn llm_judge(
    &self,
    conversation: &[ChatMessage],
    goal: &Goal,
) -> Result<Verdict, String> {
    let conv_text = conversation
        .iter()
        .filter(|m| m.role != ryvos_core::types::Role::System)
        .map(|m| format!("[{:?}] {}", m.role, m.text()))
        .collect::<Vec<_>>()
        .join("\n");

    let criteria_text = goal
        .success_criteria
        .iter()
        .map(|c| format!("- {} (weight: {})", c.description, c.weight))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"You are a judge evaluating whether an AI agent achieved its goal.

Goal: {}

Success criteria:
{}

Success threshold: {:.0}%

Conversation:
{}

Evaluate the agent's output against the goal and criteria. Respond with ONLY valid JSON:
{{
  "verdict": "accept" | "retry" | "escalate" | "continue",
  "confidence": 0.0-1.0,
  "reason": "brief explanation",
  "hint": "actionable suggestion for retry (only if verdict is retry)"
}}"#,
        goal.description,
        criteria_text,
        goal.success_threshold * 100.0,
        conv_text,
    );

    let messages = vec![ChatMessage::user(prompt)];
    let mut stream = self
        .llm
        .chat_stream(&self.config, messages, &[])
        .await
        .map_err(|e| format!("Judge LLM call failed: {}", e))?;

    let mut response_text = String::new();
    while let Some(delta) = stream.next().await {
        if let Ok(StreamDelta::TextDelta(text)) = delta {
            response_text.push_str(&text);
        }
    }

    parse_verdict(&response_text)
}
```

Several things matter here. The system message is filtered out of the
conversation before formatting — the judge does not need to see the
**[onion context](../glossary.md#onion-context)** the agent was given, only
the actual exchange. Each remaining message is rendered as
`[Role] body-text`. The prompt packages three things: the goal description,
the criteria list (each with its weight so the judge knows which criteria
are load-bearing), the threshold, and the conversation. The rubric is the
literal JSON schema at the bottom of the prompt — four enum values for
`verdict` and three optional string fields.

The empty tool list (`&[]`) is deliberate: the judge is not allowed to
call tools. It is a pure language-model judgment. Temperature is whatever
the shared `ModelConfig` specifies, which in practice means the same
temperature as the run itself; there is no separate low-temperature config
for the judge.

Streaming is used because `chat_stream` is the only trait method on
`LlmClient` in the current crate — there is no non-streaming path. The
stream is drained into `response_text` and handed to `parse_verdict`,
which is where the interesting parsing lives.

## Verdict parsing

`parse_verdict` is the reason the Judge is robust to model misbehavior.
LLMs trained on natural language do not always respect "Respond with ONLY
valid JSON". They wrap it in prose, they fence it with markdown, they
hallucinate verdict names. The parser handles all three. See
`crates/ryvos-agent/src/judge.rs:168`:

```rust
fn parse_verdict(response: &str) -> Result<Verdict, String> {
    let json_str = extract_json(response);

    match serde_json::from_str::<JudgeResponse>(json_str) {
        Ok(resp) => match resp.verdict.to_lowercase().as_str() {
            "accept" => Ok(Verdict::Accept {
                confidence: resp.confidence.clamp(0.0, 1.0),
            }),
            "retry" => Ok(Verdict::Retry {
                reason: resp.reason,
                hint: if resp.hint.is_empty() {
                    "Try a different approach.".to_string()
                } else {
                    resp.hint
                },
            }),
            "escalate" => Ok(Verdict::Escalate {
                reason: resp.reason,
            }),
            "continue" => Ok(Verdict::Continue),
            other => {
                warn!(verdict = %other, "Unknown verdict from judge, treating as continue");
                Ok(Verdict::Continue)
            }
        },
        Err(e) => {
            warn!(error = %e, response = %response, "Failed to parse judge response");
            // Default to Continue on parse failure (don't block the agent)
            Ok(Verdict::Continue)
        }
    }
}
```

Two fallbacks protect the agent loop. An unknown verdict name degrades to
`Continue` with a warning, not an error, so a model that emits `"verdict":
"yes"` does not crash the run. A JSON parse failure also degrades to
`Continue` with a warning. `Continue` is the safest default because it
tells the agent loop "no opinion, keep going", and the loop will either
hit its `max_turns` cap or the next stop reason will re-invoke the judge.
The Judge explicitly does *not* default to `Accept` (which would let a
broken judge silently declare victory) or `Escalate` (which would
prematurely end runs).

`extract_json` is three passes: markdown-fenced `json` first, markdown-
fenced untagged second, balanced braces third. See
`crates/ryvos-agent/src/judge.rs:202`. It is the same pattern used in
`evaluator.rs` and in the Director's graph parser — Ryvos has a consistent
answer to "extract JSON from an LLM response" and it lives in three places.

The `JudgeResponse` DTO at `crates/ryvos-agent/src/judge.rs:156` uses
`#[serde(default)]` on `confidence`, `reason`, and `hint` so missing
fields are tolerated. Only `verdict` is required.

## Combined evaluation

`evaluate` is the chain-of-responsibility combinator at
`crates/ryvos-agent/src/judge.rs:139`:

```rust
pub async fn evaluate(
    &self,
    output: &str,
    conversation: &[ChatMessage],
    goal: &Goal,
) -> Result<Verdict, String> {
    // Level 0: fast check
    if let Some(verdict) = Self::fast_check(output, goal) {
        return Ok(verdict);
    }

    // Level 2: LLM judge
    self.llm_judge(conversation, goal).await
}
```

The short-circuit is the whole point. A goal with only `OutputContains`
criteria takes zero LLM calls to judge — the deterministic path returns
`Some(verdict)` and `evaluate` hands it back. A goal with any `LlmJudge`
criterion takes exactly one LLM call — fast_check returns `None` and
`llm_judge` is awaited. The Judge never calls the LLM redundantly.

## Verdict dispatch in the agent loop

The agent loop calls `Judge::evaluate` once per run, at the point the model
stops emitting tool calls. See `crates/ryvos-agent/src/agent_loop.rs:811`:

```rust
// Judge evaluation (if goal provided)
if let Some(goal) = goal {
    let judge = Judge::new(self.llm.clone(), self.config.model.clone());
    match judge.evaluate(&final_text, &messages, goal).await {
        Ok(verdict) => {
            self.event_bus.publish(AgentEvent::JudgeVerdict {
                session_id: session_id.clone(),
                verdict: verdict.clone(),
            });
            match &verdict {
                Verdict::Accept { confidence } => {
                    let results = goal.evaluate_deterministic(&final_text);
                    let eval = goal.compute_evaluation(results, vec![]);
                    self.event_bus.publish(AgentEvent::GoalEvaluated {
                        session_id: session_id.clone(),
                        evaluation: eval,
                    });
                    debug!(confidence, "Judge accepted output");
                }
                Verdict::Retry { reason, hint } if turn + 1 < max_turns => {
                    let retry_msg = format!(
                        "The judge determined your response needs improvement: {}. \
                         Hint: {}",
                        reason, hint
                    );
                    messages.push(ChatMessage::user(&retry_msg));
                    continue;
                }
                Verdict::Escalate { reason } => {
                    warn!(reason = %reason, "Judge escalated — returning output as-is");
                }
                _ => {} // Continue or Retry on last turn
            }
        }
        Err(e) => {
            warn!(error = %e, "Judge evaluation failed, proceeding");
        }
    }
}
```

Each verdict variant has a distinct effect on the loop. `Accept` publishes
`JudgeVerdict` and a legacy `GoalEvaluated` event (the second is for
backward compat with pre-Judge code that read `GoalEvaluated` directly),
and falls through to the normal run-complete path. `Retry` with budget
remaining pushes a synthesized user message containing reason and hint onto
the conversation and `continue`s the turn loop — this is the core
feedback-injection mechanic that turns a single turn into a multi-turn
self-correction sequence. `Escalate` logs a warning and falls through
without retrying; the agent's last output is returned to the caller as-is,
because the judge has signaled that further iteration will not help.
`Continue` and `Retry`-on-last-turn are no-ops: the outer loop's
`turn + 1 < max_turns` guard is what enforces the retry budget.

Two things are worth pointing out about this dispatch. First, the Retry
branch uses `messages.push(ChatMessage::user(&retry_msg))` rather than
injecting via any event or hint channel; the retry message becomes a real
conversation turn. The next iteration's LLM call will see it as "the user
said this" and the model's self-correction will flow as a normal response.
Second, there is no retry counter on the Judge side. The only bound is the
turn counter, which is shared with normal tool-driven turns. A goal that
keeps failing keeps eating turns until `max_turns` runs out, at which
point the loop exits on `MaxTurnsExceeded`. Runs with goals should
therefore set `max_turns` high enough to tolerate a few judge retries on
top of whatever tool-call budget the task needs.

The `JudgeVerdict` event is also how external observers (the gateway UI,
the TUI, the run log writer) learn about the outcome. The UI can render
"Judge: Accept (confidence 0.94)" in a run card, the TUI can flash a
colored bar, and the JSONL run log captures the decision for later
analysis.

## Judge versus GoalEvaluator

The `GoalEvaluator` in `crates/ryvos-agent/src/evaluator.rs:109-222` looks
similar to the Judge but serves a different role. The Director uses it to
compute *per-criterion* LLM scores when building a
`GoalEvaluation` — it runs one LLM call per `LlmJudge` criterion and
aggregates the results through `Goal::compute_evaluation`. The Judge, by
contrast, runs one LLM call for the *whole* goal and returns a single
`Verdict`. The two coexist because they serve different callers: the
Judge is for the standard ReAct path where "did the goal succeed" is a
binary accept/retry question, while the GoalEvaluator is for the Director
path where the per-criterion breakdown matters for failure diagnosis and
plan evolution. See [director-ooda.md](director-ooda.md) for the Director's
use of the evaluator.

## Why two levels

The two-level design is a latency and cost optimization. A goal with
only deterministic criteria — "the final message must contain the word
SUCCESS" — can be judged in microseconds by a string search. Spending
an LLM round trip and 500 tokens on that judgment would be absurd.
Conversely, a goal with fuzzy criteria — "the summary should accurately
reflect the source document" — cannot be judged by pattern matching
and has to go to a language model. Having one method that picks the
right path automatically is what makes `evaluate` the natural API for
the agent loop: the caller does not have to know whether the goal is
deterministic or LLM-judged, it just calls `evaluate` and gets back a
verdict.

The cost of the optimization is that a goal with *mixed* criteria —
one `OutputContains` and one `LlmJudge` — goes entirely to the LLM
path, even though the contains check could have provided a partial
score. The fast check returns `None` as soon as it sees any LLM
criterion, and the LLM path ignores the deterministic criteria
entirely. For mixed goals, the `GoalEvaluator` in `evaluator.rs` is the
right tool — it runs both paths and aggregates the results. The Judge
is the coarser tool meant for the common case where a goal is either
all deterministic or needs an LLM decision on the whole thing.

## Integration with the Director

The Director in `crates/ryvos-agent/src/director.rs` calls the Judge at
a different point than the standard agent loop does. In the standard
loop, `Judge::evaluate` runs once at the end of the run, after the
final assistant message. In the Director, the Judge is called at the
end of each OODA cycle against the assembled output of the graph
execution, and its verdict drives the Director's evolution decision.
An `Accept` verdict ends the Director early; a `Retry` or `Escalate`
verdict feeds the Director's `diagnose_failure` step, which turns the
verdict's reason into a `SemanticFailure` that becomes part of the
next cycle's generated graph prompt. The Judge's verdict is therefore
the single input that connects "did the run achieve the goal" to "how
should the plan be revised". See [director-ooda.md](director-ooda.md)
for the Director-side dispatch.

## Cross-references

- [agent-loop.md](agent-loop.md) — the loop that calls `Judge::evaluate`
  and dispatches the verdict.
- [director-ooda.md](director-ooda.md) — the Director uses the Judge at
  the end of each cycle and `GoalEvaluator` for per-criterion scoring.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — crate overview
  showing the Judge alongside other agent subsystems.
- [../crates/ryvos-core.md](../crates/ryvos-core.md) — `Goal`,
  `SuccessCriterion`, `Verdict`, and `GoalEvaluation` type definitions.
- [../guides/writing-a-goal.md](../guides/writing-a-goal.md) — how to
  write the goal specs the Judge evaluates against.
