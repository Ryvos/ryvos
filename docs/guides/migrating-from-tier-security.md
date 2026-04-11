# Migrating from tier security

## When to use this guide

Before v0.6.0, Ryvos used a tier-based blocking security model.
Tools were classified into five tiers — `T0` (read-only) through
`T4` (system) — and the `SecurityGate` refused to dispatch any tool
above an operator-configured threshold without explicit approval.
Configurations set `auto_approve_up_to = "T1"` and `deny_above = "T3"`
to shape where the gate blocked and where it escalated.

v0.6.0 switched to **[passthrough security](../glossary.md#passthrough-security)**.
The gate no longer blocks on classification; safety is provided by
four cooperating mechanisms — constitutional AI in the system
prompt, **[SafetyMemory](../glossary.md#safetymemory)** learning
from outcomes, **[Reflexion](../glossary.md#reflexion)** hints on
repeat failures, and the full **[audit trail](../glossary.md#audit-trail)**
— with an opt-in soft-checkpoint list for tools that need human
approval. The rationale is recorded in
[ADR-002](../adr/002-passthrough-security.md).

Use this guide if you are upgrading from a pre-v0.6 config and want
to understand what to remove, what to add, and how the new model
recreates the safety properties the old one provided. Nothing in
this guide reintroduces blocking — the switch is philosophical, not
configurable. Operators who need absolute gates on specific tools
get them through the `pause_before` list and a channel that
guarantees human delivery.

## What the old tier system did

The pre-v0.6 `SecurityGate` consulted `SecurityPolicy` fields in
this order for every tool call:

1. **`auto_approve_up_to`** — a `SecurityTier` threshold. Calls at
   or below this level ran without intervention. Typical value `T1`.
2. **`deny_above`** — a second threshold. Calls strictly above this
   level were refused outright with a `ToolBlocked` error. Typical
   value `T3`.
3. **Middle zone** — calls between the thresholds triggered an
   approval request that the gate awaited synchronously; no response
   meant denial.

ADR-002 records the three problems this produced: agent autonomy
died whenever the middle zone was large, users rubber-stamped the
approvals that did fire, and static tiers never captured the fact
that `rm -rf /tmp/scratch` and `rm -rf /home/me` classify identically
but carry radically different risk. The `T0`–`T4` labels remain as
informational metadata on every tool, but they no longer gate
execution. The `ToolBlocked` error variant is retained for backward
compatibility and is never raised by current code.

## What to remove

Three field sets in the old `[security]` section do nothing in
v0.6.0 and later. They are safe to leave in place but are clutter.
Remove them when you do a config cleanup:

```toml
# These are parsed but ignored. Safe to delete.
[security]
auto_approve_up_to = "T1"   # ignored
deny_above = "T3"           # ignored
dangerous_patterns = [...]  # deprecated; use the constitution instead
```

The `dangerous_patterns` list compiled into a
`DangerousPatternMatcher` that the old gate used to refuse tool
calls containing destructive regex matches. The matcher still
exists and still compiles patterns, but it now warns on matches
without blocking, and SafetyMemory's built-in destructive-command
detection already covers the common cases. Prefer baking principles
into the system prompt (see
[configuring-safety.md](configuring-safety.md)) or adding specific
tools to `pause_before` instead.

## What to add

Three additions recreate the safety properties the old config was
trying to provide.

### 1. `pause_before` for tools that need human approval

If your old config had `deny_above = "T3"` specifically to make sure
a human saw every T3 and T4 call, the new equivalent is a
`pause_before` list that names those tools:

```toml
[security]
pause_before = ["bash", "git_commit", "write", "file_delete", "spawn_agent"]
approval_timeout_secs = 300
```

Each entry is a tool name, not a tier. The gate publishes an
`ApprovalRequested` event on the **[EventBus](../glossary.md#eventbus)**
and waits for a decision from any connected channel — Telegram,
Discord, Slack, WhatsApp, Web UI, or TUI. The decision is delivered
through the `ApprovalBroker`'s oneshot channel mechanism, which is
exactly the same plumbing the old middle-zone approval used.

Unlike the old system, an expired approval lets the call through.
Passthrough means the gate cannot hold the run forever. If absolute
gates are essential, lower the timeout and route approvals to a
channel with pager semantics, but understand that the design
choice is to never strand a run waiting for a human who is asleep.

### 2. Constitutional principles in the system prompt

The eight principles in the `DEFAULT_SYSTEM_PROMPT` constant at the
top of `crates/ryvos-agent/src/context.rs` — Preservation, Intent
Match, Proportionality, Transparency, Boundaries, Secrets, Learning,
and Untrusted Data — are the in-prompt expression of Ryvos's
constitutional-AI layer. They are the LLM's first line of defense
and replace the static classification the old tiers provided.

If you want to customize them — add domain-specific rules, tighten
the Preservation language for a production deployment, add a
"Never touch `/etc/`" principle — override the system prompt in
`ryvos.toml`:

```toml
[agent]
system_prompt = "file:prompts/constitution.md"
```

The `file:` prefix is resolved against the workspace directory. Copy
the default principles from `crates/ryvos-agent/src/context.rs` as a
starting point and add your own alongside. The full path for
authoring the override is in
[configuring-safety.md](configuring-safety.md) and
[customizing-the-soul.md](customizing-the-soul.md) — tone
customization and principle customization often happen together.

### 3. Let SafetyMemory accumulate lessons

The third addition is not a config field but a new habit. The old
blocking model assumed the operator had enough information up front
to classify every tool correctly. Passthrough assumes the agent
will learn from outcomes.

`SafetyMemory` records every tool-call outcome as one of four
values: `Harmless`, `NearMiss`, `Incident`, or `UserCorrected`. Non-
harmless outcomes turn into `SafetyLesson` rows in `safety.db` with
an action pattern, a reflection, an optional violated principle, a
corrective rule, a confidence score, and a times-applied counter.
On future runs, `SafetyMemory::relevant_lessons` fetches top-ranked
lessons for the tools available in the current run and injects them
into the narrative layer of the **[onion context](../glossary.md#onion-context)**.

The practical effect is that the agent gets safer over time. A
command that produced a near miss last week shows up in this
week's system prompt as a learned rule. A tool that leaked a
credential becomes an `Incident` lesson with high severity that
the agent treats as a strong negative signal for similar
situations. The growth is bounded: pruning sweeps stale lessons
periodically, and contradictory lessons get superseded rather than
accumulated.

Nothing in this migration requires you to configure SafetyMemory
explicitly — it is on by default and runs for the lifetime of the
daemon. Review the lessons it has accumulated by reading
`safety.db` through the `audit_query` MCP tool or by checking the
Web UI Safety page (if enabled).

## Tier values still mean something

The `T0`–`T4` labels remain on every tool's `tier()` method. They
are used by:

- **The audit trail**, which stores the tier for display.
- **Operator tooling**, which groups tools by risk category on
  dashboards and in reports.
- **The MCP bridge**, which accepts a `tier_override` in the server
  config and falls back to `T1` when missing.
- **Reporting and analytics**, including the Web UI Safety page,
  which shades entries by severity.

What they no longer do:

- Gate execution. The `SecurityGate` never denies a call based on
  `tier()`.
- Map to budget categories. The old "T3 costs more" heuristic is
  gone; budgets are tracked per run and per model, not per tier.
- Determine approval behavior. `pause_before` is a list of tool
  names, not tier thresholds.

Treat tier values as informational metadata that helps humans
reason about the system, not as a runtime control plane.

## Reviewing after migration

The audit trail is the easiest way to confirm the new model is
working:

1. Run a handful of turns that exercise tools across tiers —
   `read`, `bash`, `git_commit`, `file_delete`, `spawn_agent`. All
   should succeed; none should produce a `ToolBlocked` error.
2. `ryvos audit query` shows every call with a `SafetyOutcome` —
   usually `Harmless`, occasionally `NearMiss` for destructive-
   pattern matches that completed cleanly, rarely `Incident` for
   outputs containing credential patterns.
3. Induce a deliberate near-miss: a command matching one of the
   destructive patterns but targeting a harmless path
   (`rm -rf /tmp/test-scratch-dir`). The audit entry flags
   `NearMiss` and a lesson lands in `safety.db`; the next bash run
   sees it injected into the system prompt.
4. Set `pause_before = ["bash"]`, re-run a bash command, and
   confirm the approval prompt routes through configured channels.

## Verification

1. `ryvos doctor` reports the loaded `SecurityConfig`. The
   deprecated fields should either be absent or reported as
   ignored.
2. No `ToolBlocked` errors appear in the audit trail or the run
   log after the upgrade.
3. `pause_before` triggers an `ApprovalRequested` event on the
   expected tool and resolves through the expected channel.
4. The first few runs after the upgrade populate `safety.db` with
   at least a few `Harmless` outcomes, confirming the pipeline is
   live.
5. `ryvos audit stats` shows the distribution of outcomes and the
   tier breakdown as informational metadata only — nothing in the
   report reflects a blocked call.

For the passthrough-security rationale and the full list of
tradeoffs the switch involved, read
[../adr/002-passthrough-security.md](../adr/002-passthrough-security.md).
For the SafetyMemory internals that the migration makes
load-bearing, read
[../internals/safety-memory.md](../internals/safety-memory.md). For
the ongoing safety configuration story — soft checkpoints,
constitutional customization, sandbox setup — read
[configuring-safety.md](configuring-safety.md). For the `Tool`
trait itself and where the tier values still surface, read
[../crates/ryvos-core.md](../crates/ryvos-core.md).
