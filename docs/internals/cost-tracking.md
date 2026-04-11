# Cost tracking

Ryvos tracks the cost of every agent **[run](../glossary.md#run)** in
dollars (cents, actually) and enforces a monthly budget against the
accumulated total. That sounds like a single job, but it is actually three
jobs stitched together: classifying the current provider as
**[API](../glossary.md#api-billing)** or
**[Subscription](../glossary.md#subscription-billing)** billing, estimating
cost from token counts using a pricing table, and recording both
per-event and per-run rollups in SQLite so the **[Guardian](../glossary.md#guardian)**
and the web UI can read them. Subscription-billed providers produce $0.00
rows regardless of token count because the user already paid upstream.

This document walks `crates/ryvos-memory/src/cost_store.rs:1-337`, the
pricing table at `crates/ryvos-memory/src/pricing.rs:1-138`, the
`BillingType` classification in `crates/ryvos-core/src/types.rs:383-422`,
the recording calls in `crates/ryvos-agent/src/agent_loop.rs`, and the
budget enforcement branch of `crates/ryvos-agent/src/guardian.rs:270-328`.

## Two tables, two granularities

`CostStore` owns two SQLite tables with different purposes. See
`crates/ryvos-memory/src/cost_store.rs:39`:

```rust
CREATE TABLE IF NOT EXISTS cost_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cost_cents INTEGER DEFAULT 0,
    billing_type TEXT DEFAULT 'api',
    model TEXT NOT NULL,
    provider TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cost_ts ON cost_events(timestamp);

CREATE TABLE IF NOT EXISTS run_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    total_turns INTEGER DEFAULT 0,
    billing_type TEXT DEFAULT 'api',
    model TEXT NOT NULL,
    provider TEXT NOT NULL,
    cost_cents INTEGER DEFAULT 0,
    status TEXT DEFAULT 'running'
);
```

`cost_events` holds one row per LLM call: the atoms of cost accounting.
This is what the Guardian queries with `SUM(cost_cents)` to get monthly
spend, and what the web UI queries with `GROUP BY` to build breakdown
charts. Each row carries the run id, the session id, the timestamp, token
counts, a per-call cost in cents, the billing type, and the model and
provider strings. There is only one index (on timestamp) because all the
read patterns are either time-ranged or run-id-scoped.

`run_log` is the per-run rollup: start time, end time, cumulative tokens,
total turns, final cost in cents, and a status string (`running`,
`complete`, or `error`). It exists for two reasons. First, the Runs page
in the web UI wants to list runs as cards, and a card view wants one row
per run, not one row per LLM call. Second, the status field lets the UI
show in-flight runs distinctly from finished ones — `run_log` has a row
with `status = "running"` as soon as the run begins, long before the
first `cost_events` row shows up. `run_id` is the unique key, so restart
logic can safely re-record without duplicates.

## BillingType classification

The `BillingType` enum is two variants. See
`crates/ryvos-core/src/types.rs:384`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingType {
    Api,
    Subscription,
}
```

`Api` is the usual case: every LLM call has a token count, the user is
charged per token, and Ryvos estimates cents from the count. `Subscription`
covers the case where the user is paying a flat monthly fee upstream —
Claude Pro, Claude Max, GitHub Copilot — and the token count should
*not* produce a dollar cost because the dollars were already spent on the
subscription. Subscription runs still record tokens; they just record
them with `cost_cents = 0`.

Classification is not an enum lookup; it is decision tree against the
provider string. See `crates/ryvos-agent/src/agent_loop.rs:336`:

```rust
let billing_type = if self.config.model.provider == "claude-code"
    || self.config.model.provider == "claude-cli"
    || self.config.model.provider == "claude-sub"
{
    ryvos_llm::providers::claude_code::ClaudeCodeClient::detect_billing_type(
        &self.config.model,
    )
} else if self.config.model.provider == "copilot"
    || self.config.model.provider == "github-copilot"
    || self.config.model.provider == "copilot-cli"
{
    BillingType::Subscription
} else {
    BillingType::Api
};
```

Three branches. The `claude-code`, `claude-cli`, and `claude-sub` aliases
all map to the same decision helper, `ClaudeCodeClient::detect_billing_type`,
which inspects the model config to decide. The rule is: if
`config.api_key` is set, the provider is hitting Anthropic's API directly
and should be billed per token (`BillingType::Api`); if no API key is
set, the provider is spawning the local `claude` CLI binary and using the
user's Claude Pro or Max subscription (`BillingType::Subscription`). The
three aliases exist because the codebase has accumulated naming
alternatives as the **[CLI provider](../glossary.md#cli-provider)** was
refactored; they all point to the same client today.

The `copilot` branch is unconditional: Copilot is always subscription-
billed because there is no pay-per-token mode for GitHub Copilot.
`github-copilot` and `copilot-cli` are alias names.

The `else` branch catches everything — Anthropic, OpenAI, Gemini, Groq,
Cohere, Bedrock, OpenRouter, Together, Replicate, Ollama, Mistral,
Fireworks, Cerebras, DeepSeek, xAI, Z.AI, Qwen, and any future provider
Ryvos adds — as API-billed. Ollama in particular is a notable wart: the
user is running the model locally and paying nothing, but it gets
classified as API-billed and Ryvos estimates a dollar cost as if it were
Claude Sonnet. This is not a bug, it is a simplification — users running
Ollama don't have a budget to enforce anyway, and can override the
pricing table to zero if they want accurate reporting.

## The pricing table

The default pricing table holds eleven entries, all in cents-per-million-
tokens. See `crates/ryvos-memory/src/pricing.rs:7`:

```rust
fn default_pricing() -> Vec<(&'static str, u64, u64)> {
    vec![
        ("claude-sonnet-4", 300, 1500),
        ("claude-sonnet-4-20250514", 300, 1500),
        ("claude-opus-4", 1500, 7500),
        ("claude-opus-4-20250514", 1500, 7500),
        ("claude-haiku", 80, 400),
        ("claude-haiku-4-5-20251001", 80, 400),
        ("gpt-4o", 250, 1000),
        ("gpt-4o-mini", 15, 60),
        ("gpt-4-turbo", 1000, 3000),
        ("o1", 1500, 6000),
        ("o1-mini", 300, 1200),
    ]
}
```

Each tuple is `(model_id, input_cents_per_Mtok, output_cents_per_Mtok)`.
Claude Sonnet at 300/1500 means $3 per million input tokens, $15 per
million output tokens. Opus is five times more expensive, Haiku is roughly
four times cheaper. GPT-4o matches Sonnet on input and undercuts it
slightly on output. The table is intentionally minimal — it covers the
models Ryvos actually benchmarks against and leaves everything else to
the fallback path.

## Estimating cost

`estimate_cost_cents` at `crates/ryvos-memory/src/pricing.rs:28` is the
pure function that converts token counts to cents:

```rust
pub fn estimate_cost_cents(
    model: &str,
    _provider: &str,
    billing_type: BillingType,
    input_tokens: u64,
    output_tokens: u64,
    overrides: &HashMap<String, ModelPricing>,
) -> u64 {
    if billing_type == BillingType::Subscription {
        return 0;
    }

    let (input_rate, output_rate) = if let Some(pricing) = overrides.get(model) {
        (pricing.input_cents_per_mtok, pricing.output_cents_per_mtok)
    } else {
        match default_pricing().iter().find(|(m, _, _)| *m == model) {
            Some((_, i, o)) => (*i, *o),
            None => {
                let lower = model.to_lowercase();
                if lower.contains("opus") {
                    (1500, 7500)
                } else if lower.contains("haiku") {
                    (80, 400)
                } else if lower.contains("sonnet") {
                    (300, 1500)
                } else if lower.contains("gpt-4o-mini") {
                    (15, 60)
                } else if lower.contains("gpt-4o") {
                    (250, 1000)
                } else {
                    (300, 1500)
                }
            }
        }
    };

    let input_cost = input_tokens * input_rate / 1_000_000;
    let output_cost = output_tokens * output_rate / 1_000_000;
    input_cost + output_cost
}
```

Four lookup steps, in order:

1. **Subscription short-circuit.** If billing type is `Subscription`,
   return 0 regardless of token count. This is the contract that makes
   Claude Pro and Copilot show `$0.00` in the UI even though they ran
   large prompts.
2. **User override.** If the user has a `[budget.pricing."model-id"]`
   entry in `ryvos.toml`, use their rates. This is how a team running
   Anthropic with enterprise pricing can set their actual negotiated
   rates rather than list price.
3. **Exact match in the default table.** Eleven entries, checked
   linearly. The table is small enough that the linear scan is
   immaterial.
4. **Partial match fallback.** If the model id contains a known family
   name (`opus`, `sonnet`, `haiku`, `gpt-4o`, `gpt-4o-mini`), use the
   family's rate. The order of the checks matters: `gpt-4o-mini` is
   checked before `gpt-4o` so a `gpt-4o-mini-2024-07-18` model id is not
   miscategorized as `gpt-4o`. Similarly, `opus` is before `haiku` only
   because alphabetic order happens to also be safe order here; nothing
   is named both.
5. **Sonnet default.** Any model that does not match any family falls
   through to Sonnet-class pricing (300/1500). This is the "unknown model"
   fallback — it will be wrong for cheap models like Gemini Flash and
   wrong for expensive models like Claude Opus, but it will not be
   catastrophically wrong, and users who care can override.

Cost arithmetic is integer division by a million, which truncates. A run
with 500 input tokens at 300 cents/Mtok produces `500 * 300 / 1_000_000
= 0` cents — a fraction of a cent rounds to zero. This is deliberate:
integer cents are what SQLite stores and what the budget is specified in,
and fractional cents would require `REAL` columns with floating-point
imprecision. Over many runs, rounding-to-zero does accumulate lossiness,
but for typical usage (thousands to millions of tokens per run) the
per-run rounding is a rounding error on a rounding error.

The `ModelPricing` override struct lives in
`crates/ryvos-core/src/config.rs` and is loaded into a `HashMap<String,
ModelPricing>` at startup from `[budget.pricing]`. The map is passed by
reference to `estimate_cost_cents`, which makes it trivially cheap to
call per event.

## Run lifecycle

The run lifecycle has three cost-tracking calls. The first runs at the
top of `AgentRuntime::run`, the second runs continuously inside the
Guardian loop, and the third runs at the end of the run.

Step one: run start. See `crates/ryvos-agent/src/agent_loop.rs:351`:

```rust
if let Err(e) = cost_store.record_run(
    &run_id,
    &session_id.0,
    &self.config.model.model_id,
    &self.config.model.provider,
    billing_type,
) {
    warn!(error = %e, "Failed to record run start");
}
```

`record_run` (at `crates/ryvos-memory/src/cost_store.rs:196`) inserts a
row into `run_log` with `status = 'running'` and empty end time. The
`INSERT OR IGNORE` guard means re-recording the same `run_id` is a no-op,
which protects against the caller accidentally calling `record_run` twice
for the same run. The run id is a fresh UUID for every call to `run`, so
collision is not a concern unless the caller deliberately reuses ids.

Step two: token events. As the LLM stream progresses and usage data
arrives, the Guardian polls the runtime's token counts and records
synthesized `cost_events`. See `crates/ryvos-agent/src/guardian.rs:270`:

```rust
let event = CostEvent {
    run_id: run_id.clone(),
    session_id: session_id.clone(),
    timestamp: chrono::Utc::now(),
    input_tokens,
    output_tokens,
    cost_cents: cost,
    billing_type: BillingType::Api,
    model: "unknown".into(),
    provider: "unknown".into(),
};
let _ = cost_store.record_cost_event(&event);
```

The model and provider are "unknown" here because the Guardian does not
have direct visibility into the runtime's model config — it only has
token deltas. This is acceptable because the primary consumer of
`cost_events` (the monthly-spend aggregator) only needs the timestamp
and the cost cents. The breakdown-by-model view on the dashboard sources
its data from `run_log`, which *does* have the real model name.

Step three: run complete. At the end of a successful run, the agent loop
calls `complete_run` with final totals. See
`crates/ryvos-agent/src/agent_loop.rs:869`:

```rust
let cost = ryvos_memory::estimate_cost_cents(
    &self.config.model.model_id,
    &self.config.model.provider,
    BillingType::Api,
    total_input_tokens,
    total_output_tokens,
    &std::collections::HashMap::new(),
);
if let Err(e) = cost_store.complete_run(
    &run_id,
    total_input_tokens,
    total_output_tokens,
    turn + 1,
    cost,
    "complete",
) {
    warn!(error = %e, "Failed to record run completion");
}
```

`complete_run` at `crates/ryvos-memory/src/cost_store.rs:222` runs a
single `UPDATE` against `run_log`: set end time to now, overwrite the
aggregate token counts, set turn count, set cost cents, flip status to
`"complete"`. The identical path exists for error termination at line
1163, differing only in the status string (`"error"` instead of
`"complete"`) and the turn count (which is `max_turns` on the
`MaxTurnsExceeded` path).

Note the hardcoded `BillingType::Api` on both complete paths — this is
a subtle bug in the current code: a run on a subscription-billed provider
will compute a nonzero cost in this call and write it to the `run_log`
`cost_cents` column, even though the event-level accounting correctly
wrote zero. The `monthly_spend_cents` query reads from `cost_events`,
not `run_log`, so budget enforcement is not affected; only the per-run
rollup shows an erroneously nonzero dollar amount. The fix would be to
recompute billing type at completion time, or to thread it through from
the start-of-run calculation.

## Monthly spend

`monthly_spend_cents` is the Guardian's budget read. See
`crates/ryvos-memory/src/cost_store.rs:103`:

```rust
pub fn monthly_spend_cents(&self) -> Result<u64> {
    let now = Utc::now();
    let month_start = format!(
        "{}-{:02}-01T00:00:00+00:00",
        now.format("%Y"),
        now.format("%m")
    );

    let conn = self.conn.lock().unwrap();
    let cents: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(cost_cents), 0) FROM cost_events WHERE timestamp >= ?1",
            params![month_start],
            |row| row.get(0),
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;
    Ok(cents as u64)
}
```

Two things matter. First, the month start is always the first of the
current month at midnight UTC — this is the definition of "monthly"
that Ryvos uses. There is no configuration for billing cycle alignment
(every 30 days from the first run, say); it is always calendar months.
Second, `COALESCE(SUM(cost_cents), 0)` ensures that an empty month
returns zero, not NULL. The Guardian can call this on a fresh install
without any rows and get back `Ok(0)`.

## Breakdown queries

`cost_by_group` powers the cost dashboard in the web UI. See
`crates/ryvos-memory/src/cost_store.rs:152`:

```rust
pub fn cost_by_group(
    &self,
    from: &DateTime<Utc>,
    to: &DateTime<Utc>,
    group_by: &str,
) -> Result<Vec<(String, u64, u64, u64)>> {
    let group_col = match group_by {
        "model" => "model",
        "provider" => "provider",
        "day" => "DATE(timestamp)",
        _ => "model",
    };

    let sql = format!(
        "SELECT {col}, COALESCE(SUM(cost_cents), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
         FROM cost_events WHERE timestamp >= ?1 AND timestamp <= ?2
         GROUP BY {col} ORDER BY SUM(cost_cents) DESC",
        col = group_col
    );
    /* ... execute ... */
}
```

Three grouping modes. `model` and `provider` are direct column
references; `day` uses SQLite's `DATE()` to truncate the ISO 8601
timestamp. Any other value falls through to `model` — this is why a
bogus `?group_by=` query parameter does not error out. The ordering is
always descending cost, which gives the UI the natural "most expensive
first" layout for free.

String interpolation into the SQL looks like an injection risk but the
`match` above only allows three exact literal values, so the injected
string is one of three safe constants. A bounded whitelist is the
correct way to parameterize column names in SQLite, which does not
support bind parameters for identifiers.

`cost_summary` at `crates/ryvos-memory/src/cost_store.rs:123` is a
simpler variant: four aggregates (cost cents, input tokens, output
tokens, event count) over a time range with no grouping. It is the
number the dashboard shows at the top of the Costs page.

`run_history` at `crates/ryvos-memory/src/cost_store.rs:250` is the
paginated read for the Runs page — it returns a vector of JSON blobs
plus a total count. It is the only cost-store method that returns
`serde_json::Value` directly; every other method returns typed tuples.
The JSON shape is assembled inside the closure to match the UI's
TypeScript interface.

## Guardian budget enforcement

The Guardian is the subscriber that watches `monthly_spend_cents` and
turns overruns into events. See
`crates/ryvos-agent/src/guardian.rs:280`:

```rust
if let Ok(spent) = cost_store.monthly_spend_cents() {
    let warn_threshold = budget_cents * dollar_warn_pct / 100;
    let hard_threshold = budget_cents * dollar_hard_pct / 100;

    if !dollar_warned && spent >= warn_threshold {
        dollar_warned = true;
        let pct = (spent * 100 / budget_cents) as u8;
        warn!(spent_cents = spent, budget_cents = budget_cents, pct = pct,
              "Guardian: dollar budget warning");
        self.event_bus.publish(AgentEvent::BudgetWarning {
            session_id: session_id.clone(),
            spent_cents: spent,
            budget_cents,
            utilization_pct: pct,
        });
        let hint = format!(
            "[Guardian] Budget warning: ${:.2} / ${:.2} ({}%)",
            spent as f64 / 100.0,
            budget_cents as f64 / 100.0,
            pct
        );
        let _ = self.hint_tx.send(GuardianAction::InjectHint(hint)).await;
    }

    if spent >= hard_threshold {
        dollar_stopped = true;
        warn!(spent_cents = spent, budget_cents = budget_cents,
              "Guardian: dollar budget exceeded");
        self.event_bus.publish(AgentEvent::BudgetExceeded {
            session_id: session_id.clone(),
            spent_cents: spent,
            budget_cents,
        });
        let reason = format!(
            "Budget exceeded: ${:.2} / ${:.2}",
            spent as f64 / 100.0,
            budget_cents as f64 / 100.0
        );
        let _ = self.hint_tx.send(GuardianAction::CancelRun(reason)).await;
        self.cancel.cancel();
    }
}
```

Two thresholds. `warn_threshold` defaults to 80% — when the user hits
that, the Guardian publishes `BudgetWarning` and injects a hint into the
run's message stream via its hint channel so the agent knows it should
tread carefully. `hard_threshold` defaults to 100% — when the user hits
that, the Guardian publishes `BudgetExceeded`, queues a `CancelRun`
action on the hint channel, and triggers the cancellation token that
tears down the run. The `dollar_warned` and `dollar_stopped` flags
ensure each event fires at most once per run, so a run that crosses 80%
gets exactly one warning, not one per Guardian tick.

The warn and hard percentages come from `config.agent.guardian.budget.warn_pct`
and `budget.hard_stop_pct`. The `budget.monthly_limit_cents` is the total
budget in cents (so `$50/month` is `5000` in the config). Setting
`monthly_limit_cents = 0` disables budget enforcement entirely — the
Guardian skips the entire block when no budget is configured.

## UI integration

The gateway's `/api/metrics` endpoint reads `monthly_spend_cents` for
the dashboard header. `/api/costs` exposes `cost_summary`,
`cost_by_group`, and `run_history` under nested routes. Both endpoints
require the Viewer role or higher; see
[../api/gateway-rest.md](../api/gateway-rest.md) for the full list.
The web UI's Costs page is a thin shell over these three queries:
top-line summary, grouped bar charts by model and by day, and a
paginated run table.

## Cross-references

- [agent-loop.md](agent-loop.md) — the loop that opens the run, records
  cost events, and closes it out.
- [guardian.md](guardian.md) — the watchdog that reads monthly spend
  and fires budget events.
- [../crates/ryvos-memory.md](../crates/ryvos-memory.md) — crate
  overview including `CostStore`, `SessionStore`, and the Viking store.
- [../api/gateway-rest.md](../api/gateway-rest.md) — the REST endpoints
  that expose the cost data to the web UI.
- [../architecture/persistence.md](../architecture/persistence.md) — the
  `cost.db` schema alongside the other SQLite databases.
