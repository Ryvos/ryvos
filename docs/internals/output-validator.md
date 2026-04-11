# Output validator

LLMs mostly do what you ask, but when you ask for structured JSON output
they sometimes get it subtly wrong. A model will decide that markdown is
"nicer" and wrap the whole response in a triple-backtick fence. It will
forget the closing brace on a nested object and hand back a half-finished
block. It will emit a trailing comma because JavaScript tolerates one and
JSON does not. Any of these makes a strict `serde_json::from_str` fail
and blows up a **[goal](../glossary.md#goal)**-driven run that was
otherwise on track.

The output validator exists to catch these common failures cheaply. It
has two sides: `OutputValidator` detects issues (length, required keys,
parseability), and `OutputCleaner` repairs them — first heuristically,
then optionally via an LLM call when heuristics are not enough. The
whole module is 375 lines and lives at
`crates/ryvos-agent/src/output_validator.rs`.

This document walks the module top to bottom. For the
**[agent runtime](../glossary.md#agent-runtime)** call sites that apply
it, see [agent-loop.md](agent-loop.md).

## ValidationResult

The detection side is a two-variant enum at
`crates/ryvos-agent/src/output_validator.rs:22`:

```rust
pub enum ValidationResult {
    Valid,
    Invalid { issues: Vec<String> },
}
```

`Valid` is a no-op that says "the output passed every check." `Invalid`
carries a `Vec<String>` of issue descriptions, one per failed check.
The variants are structured rather than stringly-typed so callers can
pattern-match on the shape without parsing a string, but the issues
themselves are strings because they are destined for the LLM repair
prompt, the run log, or the user-facing error message — all consumers
that want prose.

## OutputValidator

`OutputValidator` at `crates/ryvos-agent/src/output_validator.rs:11`
holds the configuration for what counts as valid:

```rust
pub struct OutputValidator {
    pub required_keys: Vec<String>,
    pub max_length: usize,
    pub schema: Option<serde_json::Value>,
}
```

`required_keys` is a list of JSON object keys that must be present in
the parsed output. It is the workhorse of goal-driven validation: a
goal that demands `{"status", "result", "reason"}` can set
`required_keys = ["status", "result", "reason"]` and the validator
will reject anything missing. `max_length` is a hard character cap —
outputs longer than this are rejected without a parse attempt, because
megabyte-scale outputs are almost always wrong regardless of content.
The default is 100,000 characters, which is large enough for any
plausible structured response and small enough to catch runaway
generation. `schema` is a reserved field for full JSON Schema
validation; the current implementation does not evaluate it, but the
field is present so a future version can plug in a schema validator
without breaking the struct layout.

`validate(output)` at
`crates/ryvos-agent/src/output_validator.rs:40` runs the checks in
order:

```rust
pub fn validate(&self, output: &str) -> ValidationResult {
    let mut issues = Vec::new();

    if output.len() > self.max_length {
        issues.push(format!(
            "Output exceeds max length: {} > {}",
            output.len(),
            self.max_length
        ));
    }

    if !self.required_keys.is_empty() {
        match serde_json::from_str::<serde_json::Value>(output) {
            Ok(val) => {
                if let Some(obj) = val.as_object() {
                    for key in &self.required_keys {
                        if !obj.contains_key(key) {
                            issues.push(format!("Missing required key: '{}'", key));
                        }
                    }
                } else {
                    issues.push("Expected JSON object but got non-object".to_string());
                }
            }
            Err(e) => {
                issues.push(format!("Output is not valid JSON: {}", e));
            }
        }
    }

    if issues.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid { issues }
    }
}
```

Three observations. First, the length check runs before the parse
attempt so that a 10 MB blob does not get handed to `serde_json`
(which is fast but not free on huge inputs). Second, the required-
keys check is *skipped* when no keys are declared — a validator
without `required_keys` is effectively a length checker, suitable
for unstructured responses where the only concern is not running
away. Third, issues are accumulated rather than short-circuited, so
a single `validate` call surfaces every problem at once, which makes
for better repair prompts.

## OutputCleaner

The repair side lives in `OutputCleaner` at
`crates/ryvos-agent/src/output_validator.rs:87`:

```rust
pub struct OutputCleaner {
    llm: Option<Arc<dyn LlmClient>>,
    config: Option<ModelConfig>,
}
```

Two fields, both optional. The cleaner can be instantiated in one of
two modes: full-fat via `new(llm, config)` with an LLM client and
model config, or stripped-down via `heuristic_only()`:

```rust
pub fn new(llm: Arc<dyn LlmClient>, config: ModelConfig) -> Self {
    Self { llm: Some(llm), config: Some(config) }
}

pub fn heuristic_only() -> Self {
    Self { llm: None, config: None }
}
```

Most call sites use `heuristic_only()` because heuristics are free
and fix the common cases. The LLM repair path is reserved for
situations where heuristics leave the output still malformed and the
extra round trip is worth it.

## heuristic_repair

The heuristic path is a static method — it does not need `self` —
because it is pure string manipulation with no I/O. See
`crates/ryvos-agent/src/output_validator.rs:113`:

```rust
pub fn heuristic_repair(output: &str) -> String {
    let mut result = output.to_string();
    result = strip_code_fences(&result);
    result = result.trim().to_string();
    if result.starts_with('{') || result.starts_with('[') {
        result = balance_braces(&result);
    }
    result
}
```

Three passes. First, strip markdown code fences. Second, trim leading
and trailing whitespace. Third, if the result looks like it wanted
to be JSON (starts with `{` or `[`), balance any missing closing
braces and brackets. Each pass runs unconditionally; the `if
starts_with` guard on `balance_braces` avoids corrupting non-JSON
text with spurious appended `}`s.

## strip_code_fences

The markdown-stripper at
`crates/ryvos-agent/src/output_validator.rs:182`:

```rust
fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();

    // Try ```json ... ``` first
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // Try ``` ... ```
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let content_start = after.find('\n').map_or(0, |p| p + 1);
        let after = &after[content_start..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    trimmed.to_string()
}
```

The two passes handle the two common LLM behaviors. First, models
that know they are producing JSON usually wrap the output as
` ```json ... ``` ` — the stripper finds the opening tag, skips the
seven characters, finds the closing ` ``` `, and returns the
slice between. Second, models that just fence the response
generically produce ` ``` ... ``` ` with an optional language tag on
the first line (e.g. ` ```javascript ` ); the second pass finds the
opening fence, skips any language tag on the same line, and extracts
the content up to the closing fence.

If neither pattern matches, the trimmed input is returned unchanged.
This is important: a response that is *already* bare JSON should not
be mangled by the stripper. The fallback is an identity function on
anything without fences.

The algorithm is intentionally simple and does not handle nested
fences perfectly. A JSON value containing a ` ``` ` substring would
cause the outer fence extractor to stop early. The tests
(`test_strip_code_fences_nested_backticks`) cover the specific case
of single-backtick substrings inside a JSON string, which works
because `strip_code_fences` looks for ` ``` ` specifically and a
lone backtick does not match. Fully nested ` ``` ` sequences in
JSON strings are rare enough in practice that the extra complexity
is not justified.

## balance_braces

`balance_braces` is the most interesting pass. It scans the text
character by character, tracks brace and bracket depth while
respecting string escape state, and appends whatever closers are
missing at the end. See
`crates/ryvos-agent/src/output_validator.rs:208`:

```rust
fn balance_braces(text: &str) -> String {
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in text.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
    }

    let mut result = text.to_string();
    for _ in 0..bracket_depth {
        result.push(']');
    }
    for _ in 0..brace_depth {
        result.push('}');
    }
    result
}
```

The state machine has three boolean flags. `in_string` tracks
whether the scan is currently inside a double-quoted JSON string —
braces inside strings do not count. `escape_next` handles JSON's
backslash escape: a `\` inside a string causes the next character
to be treated as literal, not as a delimiter. Outside strings,
every `{`, `}`, `[`, and `]` adjusts its respective depth counter.

At the end of the scan, the two depth counters tell you how many
closers are missing. The repair appends exactly that many, with
brackets first and braces second. The ordering matches typical JSON
structure: an unbalanced input is usually a truncated object
containing a truncated array, so the array closes first and then
the object.

The algorithm cannot fix everything. Extra closers (more `}` than
`{`) will produce a negative depth, and the repair loop will emit
no extra closers — the output stays invalid. Trailing commas
(`{"a": 1,}`) are not touched by the balancer and will still fail
`serde_json::from_str`. A value that is malformed in other ways
(unescaped newlines in strings, single quotes instead of double) is
also left alone. The balancer is a cheap, high-precision fix for
the specific problem of truncated output, not a general JSON
repair engine.

The test `test_brace_balancing_with_strings` at
`crates/ryvos-agent/src/output_validator.rs:366` specifically
verifies that braces inside strings are ignored:

```rust
#[test]
fn test_brace_balancing_with_strings() {
    let input = r#"{"msg": "use { and }", "open": true"#;
    let result = balance_braces(input);
    assert!(result.ends_with('}'));
    assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
}
```

The `{` and `}` inside `"use { and }"` are inside a string, so
they do not affect the depth counter. The one unbalanced `{` that
opens the object is balanced by appending a single `}` at the end,
and the result parses.

## llm_repair

When heuristics are not enough, `OutputCleaner::llm_repair` at
`crates/ryvos-agent/src/output_validator.rs:131` escalates to the
LLM:

```rust
pub async fn llm_repair(&self, output: &str, issues: &[String]) -> Result<String, String> {
    let llm = self.llm.as_ref().ok_or_else(|| "No LLM configured for repair".to_string())?;
    let config = self.config.as_ref().ok_or_else(|| "No model config for repair".to_string())?;

    let issues_text = issues.join("\n- ");
    let prompt = format!(
        r#"The following output has issues that need to be fixed:

Issues:
- {}

Original output:
{}

Fix the output to resolve these issues. Return ONLY the corrected output, nothing else."#,
        issues_text, output
    );

    let messages = vec![ChatMessage::user(prompt)];
    let stream_result = llm.chat_stream(config, messages, &[]).await
        .map_err(|e| format!("LLM repair call failed: {}", e))?;

    let mut stream = stream_result;
    let mut repaired = String::new();
    while let Some(delta) = stream.next().await {
        if let Ok(StreamDelta::TextDelta(text)) = delta {
            repaired.push_str(&text);
        }
    }

    if repaired.is_empty() {
        Ok(output.to_string())
    } else {
        Ok(Self::heuristic_repair(&repaired))
    }
}
```

Four observations. First, the method bails out immediately if the
cleaner was created in heuristic-only mode — both `llm` and
`config` must be present, and missing either produces a descriptive
error rather than a crash. Second, the repair prompt is a
hand-authored template that names the issues and the original
output verbatim, ending with "Return ONLY the corrected output,
nothing else." This last instruction is necessary because otherwise
the model sometimes wraps the corrected output in prose, which
would defeat the purpose. Third, the repair stream is consumed
with an empty tool list (`&[]`) because repair is a pure text
transformation; the model should not call tools during repair.
Fourth, and most importantly, the LLM's response is *itself* run
through `heuristic_repair` before being returned. This is defensive:
the model might wrap its fix in new markdown fences or add trailing
whitespace, and running the heuristics again normalizes the output
even if the model did its job imperfectly.

If the LLM returns an empty response — which happens rarely but can
be triggered by extended-thinking models that forgot to produce a
text delta — the method returns the original output unchanged. The
run log records a warning so operators can notice the pattern, but
the run is not failed; the validator will simply find the same
issues again and the loop will move on.

## Call sites in the agent loop

The cleaner is invoked from two places in the per-turn loop. Both
use the heuristic-only path. See
`crates/ryvos-agent/src/agent_loop.rs:806`:

```rust
if is_final_response {
    let repaired = OutputCleaner::heuristic_repair(&text_content);
    final_text = repaired;
    // ... continue to Judge evaluation ...
}
```

And at `crates/ryvos-agent/src/agent_loop.rs:895`:

```rust
warn!("LLM hit max tokens");
if is_final_response {
    final_text = OutputCleaner::heuristic_repair(&text_content);
    self.event_bus.publish(AgentEvent::RunComplete { ... });
}
```

The first site runs on a normal `EndTurn` stop when the model has
produced its final response (no more tool calls). The repaired
text is what the **[Judge](../glossary.md#judge)** evaluates. The
second site runs on a `MaxTokens` stop, which is a truncation
event — the model ran out of output budget. Heuristic repair is
especially useful here because truncated JSON is exactly what
`balance_braces` was written for.

Neither site calls `llm_repair`. The LLM repair path exists for
callers that want to spend an extra LLM round trip on output
quality, but the default agent loop does not do that automatically.
The rationale is that the heuristics catch the common cases
(markdown fences, truncation, whitespace) and the Judge will
retry the run anyway if its evaluation fails. Spending another
LLM call on repair is a second-order optimization that has not
been needed in practice; the hook is there for callers that do
need it (the Director's output normalization step is a candidate
for future use).

## What it does not try to do

The output validator is not a schema validator, not a semantic
checker, and not a safety filter. It answers a narrow question:
"is this text well-formed enough to hand to `serde_json` without
crashing?" It does not check that the keys have the right types,
that the values are in valid ranges, that the required keys
contain the expected data, or that the content is appropriate to
the user's goal. Those checks happen elsewhere — in the Judge's
criterion evaluators, in the Director's semantic-failure diagnoser,
in user-supplied validation code that the goal system can invoke.

Nor does it attempt to repair semantic errors. A hallucinated
field name, a swapped value, an off-by-one number — none of these
are fixable by brace-balancing or code-fence stripping, and the
repair pipeline correctly does nothing about them. The validator's
purpose is to get malformed text back into a parseable shape so
that *other* systems (Judge, Director, downstream code) can do
their own semantic work without tripping on a syntax error first.

## Tests

The test module at
`crates/ryvos-agent/src/output_validator.rs:249` covers the
heuristic paths and the validator at three levels of brokenness:

- Clean JSON: round-trips unchanged through `heuristic_repair`.
- Markdown-wrapped JSON: fences are stripped, content is returned.
- Language-tagged code fences (non-JSON): fences are stripped
  correctly, the body is returned.
- Truncated JSON (missing closing brace or bracket): the balancer
  appends missing closers and the result parses.
- Bracket-only truncation: `[1, 2, [3, 4` becomes `[1, 2, [3, 4]]`.
- Balanced input: no changes.
- Non-JSON plain text: no changes.
- Braces inside strings: ignored by the depth counter.
- Required-keys validation: detects missing keys without short-
  circuiting on the first failure.
- Max-length validation: detects overlong inputs without parsing.
- Non-JSON when JSON is expected: detects a parse error cleanly.
- Nested backticks inside JSON strings: the strip-fences pass
  does not eat the inner backticks.

The `llm_repair` path is not covered by unit tests because it
requires an `LlmClient` implementation, which the test module
does not mock. Integration coverage comes from the Judge test
suite, which exercises the full round trip in goal-driven runs.

## Where to go next

- [agent-loop.md](agent-loop.md) — the two call sites in the
  per-turn loop that invoke `heuristic_repair` on final output.
- [judge.md](judge.md) — the downstream consumer of the repaired
  text, including Level 0 deterministic checks and Level 2 LLM
  verdicts.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — the
  `OutputValidator` and `OutputCleaner` entries in the crate's
  type table.
- [director-ooda.md](director-ooda.md) — the goal-driven path
  where output validation matters most, and where `llm_repair`
  is a candidate for future use.
