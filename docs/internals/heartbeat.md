# Heartbeat

The **[Heartbeat](../glossary.md#heartbeat)** is the only part of Ryvos that
runs the agent without being asked. Every `interval_secs` seconds (default
1800 — thirty minutes) it wakes up, constructs a prompt out of `HEARTBEAT.md`
and any recent safety incidents, creates a dedicated **[session](../glossary.md#session)**,
runs the **[agent runtime](../glossary.md#agent-runtime)** once, inspects the
response, and either stays quiet or publishes an alert to a channel. It is the
proactive half of the system — the **[Guardian](../glossary.md#guardian)**
reacts to events, the Heartbeat creates them.

This document walks the implementation across
`crates/ryvos-agent/src/heartbeat.rs:1-366`, the embedded prompt template at
`src/onboard/templates/HEARTBEAT.md`, and the channel router at
`crates/ryvos-channels/src/dispatch.rs:101-138` that turns heartbeat events
into outbound messages.

## Why a proactive loop

The agent loop in `agent_loop.rs` only runs when something sends it a message:
a Telegram DM, a TUI keystroke, an API call, or a cron tick. Nothing in the
system watches the workspace otherwise. A disk that slowly fills up, a git
remote that has diverged, a background service that silently died — none of
these produce events the agent can react to. The Heartbeat fixes that gap by
becoming the thing that sends the message. It runs the agent against a
prompt that says, in effect, "Look around and tell me if anything is wrong."

The tradeoff is noise. A dumb polling loop would produce a wall of "everything
looks fine" messages every thirty minutes and the user would mute the channel
within a day. The Heartbeat's ack-suppression mechanism (documented below) is
the answer: silence when nothing is wrong, a short and actionable alert when
something is. The slogan in the template is "bark when there's a burglar, not
when a leaf falls".

## Struct and dependencies

The `Heartbeat` struct is defined at
`crates/ryvos-agent/src/heartbeat.rs:41`:

```rust
pub struct Heartbeat {
    config: HeartbeatConfig,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    workspace: PathBuf,
    session_meta: Option<Arc<SessionMetaStore>>,
    audit_trail: Option<Arc<crate::AuditTrail>>,
}
```

Seven fields, only five of which are required. `session_meta` and
`audit_trail` are injected via setters (`set_session_meta`,
`set_audit_trail`) after construction — both are optional so tests and
lightweight deployments can run the Heartbeat without a full
**[audit trail](../glossary.md#audit-trail)** or CLI session store.

The `HeartbeatConfig` at `crates/ryvos-core/src/config.rs:555-592` carries
eight knobs:

- `enabled: bool` — master off switch, defaults to `false`. Opt-in.
- `interval_secs: u64` — default 1800 (30 min). Driving the loop cadence.
- `target_channel: Option<String>` — which channel adapter should receive
  `HeartbeatAlert`. `None` means broadcast to every connected adapter.
- `active_hours: Option<ActiveHoursConfig>` — optional time window.
- `ack_max_chars: usize` — default 300. The ack-suppression threshold.
- `heartbeat_file: String` — default `"HEARTBEAT.md"`, resolved relative to
  the workspace.
- `prompt: Option<String>` — overrides the built-in default prompt when set.

The `Arc<AgentRuntime>` means the Heartbeat shares the same runtime as every
other caller. It does not spawn its own runtime; there is exactly one in the
process and the Heartbeat just holds a handle.

## The main loop

`Heartbeat::run` is a plain `tokio::select!` over a sleep timer and the
cancellation token. See `crates/ryvos-agent/src/heartbeat.rs:81`:

```rust
pub async fn run(&self) {
    let interval = Duration::from_secs(self.config.interval_secs);
    info!(interval_secs = self.config.interval_secs,
          heartbeat_file = %self.config.heartbeat_file, "Heartbeat started");

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = self.cancel.cancelled() => {
                info!("Heartbeat shutting down");
                break;
            }
        }
        // ... cycle body ...
    }
}
```

Two observations about the shape of this loop. First, the sleep happens
*before* the first cycle, not after it — a freshly-started Heartbeat with a
30-minute interval waits 30 minutes before its first fire. This is
deliberate: the daemon has just started, the user probably ran
`ryvos soul` or `ryvos onboard` minutes ago, and firing the Heartbeat on top
of that initialization traffic would be noise. Second, the cancellation
branch is checked every iteration, not polled inside the cycle body. A
running heartbeat cycle finishes before the loop tears down, even if the
cancel token fires mid-cycle. For a 30-minute interval this is fine; for a
10-second test interval, shutdown can lag by one cycle.

## Active hours

Before every cycle, `is_within_active_hours` at
`crates/ryvos-agent/src/heartbeat.rs:231` gates the fire:

```rust
fn is_within_active_hours(&self) -> bool {
    let active = match self.config.active_hours {
        Some(ref ah) => ah,
        None => return true, // No restriction
    };
    is_within_window(Utc::now(), active.start_hour, active.end_hour,
                     active.utc_offset_hours)
}
```

If `active_hours` is `None`, the Heartbeat always fires. Otherwise
`is_within_window` applies the UTC offset and compares against the window.
The function supports two window shapes, visible at
`crates/ryvos-agent/src/heartbeat.rs:261`:

```rust
if start_hour <= end_hour {
    // Normal window: e.g., 9..22
    local_hour >= start_hour && local_hour < end_hour
} else {
    // Wrapping window: e.g., 22..06 means 22-23 + 0-5
    local_hour >= start_hour || local_hour < end_hour
}
```

The wrapping case is the overnight-watch pattern. A system administrator
who wants the Heartbeat to run 22:00 through 06:00 (when they are asleep and
problems have to be caught by the agent alone) sets `start_hour = 22,
end_hour = 6`, and the inequality flips to a logical OR so every hour after
22 and every hour before 6 counts as inside. The `end_hour` bound is
exclusive in both shapes; `9..22` does not fire at 22:00 exactly. Outside
the window the loop logs "Heartbeat skipped — outside active hours" and
`continue`s to the next tick.

## Session key and prompt assembly

When the window check passes, the cycle begins. The session id uses a
timestamp format that guarantees one session per fire:

```rust
let now = Utc::now();
let session_id = SessionId::from_string(
    &format!("heartbeat:{}", now.format("%Y%m%d-%H%M%S"))
);

self.event_bus.publish(AgentEvent::HeartbeatFired { timestamp: now });
```

The `heartbeat:` prefix is a convention — the **[session manager](session-manager.md)**
uses that prefix to distinguish heartbeat runs from user runs in the UI, and
the `YYYYMMDD-HHMMSS` timestamp means two heartbeats cannot collide even if
one fires a second after another. `HeartbeatFired` is the first event the
cycle emits, and the gateway UI uses it to show a pulse in the status bar
before any response arrives.

Prompt assembly happens in `build_prompt` at
`crates/ryvos-agent/src/heartbeat.rs:202`. Three steps:

1. Resolve the path: `workspace_dir / heartbeat_file` (default
   `HEARTBEAT.md`).
2. If the file does not exist, create it from the embedded template. This
   is the v0.8.1 auto-bootstrap behavior — the first time a fresh workspace
   fires a heartbeat, the template is materialized on disk so the user can
   edit it. The template is pulled in at compile time via
   `include_str!("../../../src/onboard/templates/HEARTBEAT.md")`, so there is
   no filesystem dependency at build time.
3. Read the file contents and prepend them to the prompt as a `## Workspace
   Context (HEARTBEAT.md)` block, then append either `config.prompt` or the
   `DEFAULT_PROMPT` constant.

The `DEFAULT_PROMPT` is one sentence:

```rust
const DEFAULT_PROMPT: &str =
    "Review the workspace. If everything is fine, respond with HEARTBEAT_OK. \
     If anything needs attention, describe it concisely.";
```

This is the contract with the LLM: produce `HEARTBEAT_OK` when nothing is
wrong, a short description otherwise. The ack-suppression logic (next
section) then acts on that contract.

The embedded template is richer. It is a five-part checklist at
`src/onboard/templates/HEARTBEAT.md`: system health (`uptime`, `free`, `df`),
git status, **[Viking](../glossary.md#viking)** memory writes, a self-reflection
section pointing at **[Reflexion](../glossary.md#reflexion)**, and a report
step. The template instructs the agent to persist observations via
`viking_write`, check for prior lessons via `viking_search`, and log each
cycle via `daily_log_write`. The user is expected to edit this file to add
their own checks; the template is a starting point, not a fixed contract.

## Safety retrospective

After the basic prompt is built, the Heartbeat optionally injects a
"safety retrospective" drawn from the audit trail. See
`crates/ryvos-agent/src/heartbeat.rs:114`:

```rust
if let Some(ref trail) = self.audit_trail {
    if let Ok(entries) = trail.recent_entries("", 50).await {
        let flagged: Vec<_> = entries
            .iter()
            .filter(|e| {
                !matches!(e.outcome, crate::safety_memory::SafetyOutcome::Harmless)
            })
            .collect();
        if !flagged.is_empty() {
            prompt.push_str("\n\n## Safety Retrospective\n\n");
            prompt.push_str(
                "The following recent actions had non-harmless safety outcomes. \
                 Evaluate whether corrective lessons should be recorded via viking_write \
                 to viking://agent/lessons/:\n\n",
            );
            for entry in flagged.iter().take(10) {
                prompt.push_str(&format!(
                    "- **{}** `{}`: {:?}\n",
                    entry.tool_name,
                    entry.input_summary.chars().take(80).collect::<String>(),
                    entry.outcome
                ));
            }
        }
    }
}
```

The logic walks the fifty most recent audit entries, filters out anything
classified `Harmless` by the **[security gate](../glossary.md#security-gate)**,
and, if there is anything left, pushes a block onto the prompt asking the
agent to decide whether to record a corrective lesson. This ties the
Heartbeat into the **[SafetyMemory](../glossary.md#safetymemory)** learning
loop: near-misses and incidents from the last half hour become reading
material for the next check, and the agent is explicitly told it has
`viking_write` at its disposal to turn them into durable lessons. The
filter is a `matches!` negation against `SafetyOutcome::Harmless`, so
`NearMiss`, `Incident`, and `UserCorrected` outcomes all flow through. The
flagged list is truncated to ten entries to keep the prompt bounded.

## CLI session resumption

**[CLI providers](../glossary.md#cli-provider)** (Claude Code, Copilot) can
resume an existing upstream session across Ryvos runs via `--resume`. The
Heartbeat uses a fixed session key for this, `"heartbeat:default"`, which
means every heartbeat cycle resumes the *same* upstream conversation
regardless of the current cycle's session id. See
`crates/ryvos-agent/src/heartbeat.rs:141`:

```rust
let session_key = "heartbeat:default";

info!(session = %session_id, "Heartbeat firing");

// Look up CLI session ID for resumption
if let Some(ref meta_store) = self.session_meta {
    if let Ok(Some(meta)) = meta_store.get(session_key) {
        if let Some(cli_id) = meta.cli_session_id {
            info!(cli_session = %cli_id, "Resuming CLI session");
            self.runtime.set_cli_session_id(Some(cli_id));
        }
    }
}
```

The lookup reads the `cli_session_id` stashed in `SessionMetaStore` under
`"heartbeat:default"` and, if present, calls
`AgentRuntime::set_cli_session_id`. That setter drops the id into
`cli_session_override`, which the next `agent_loop.rs` run will consume and
splice into `ModelConfig`. After the run, the Heartbeat captures the
provider-emitted `last_message_id` and writes it back:

```rust
if let Some(ref meta_store) = self.session_meta {
    if let Some(new_cli_id) = self.runtime.last_message_id() {
        meta_store
            .get_or_create(session_key, &session_id.0, "heartbeat")
            .ok();
        if let Err(e) = meta_store.set_cli_session_id(session_key, &new_cli_id) {
            warn!(error = %e, "Failed to persist CLI session ID");
        }
    }
}
```

The effect is that a CLI-provider heartbeat conversation keeps accumulating
memory on the upstream side across fires, without Ryvos having to manage
that memory itself. On a run failure, the store is cleared with
`clear_cli_session_id` so a broken upstream session does not poison the next
cycle.

## Ack suppression

The response classification is where the Heartbeat earns its keep. See
`crates/ryvos-agent/src/heartbeat.rs:277`:

```rust
fn evaluate_response(response: &str, ack_max_chars: usize) -> HeartbeatResult {
    if response.len() > ack_max_chars {
        return HeartbeatResult::Alert;
    }

    let lower = response.to_lowercase();
    for pattern in ACK_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            return HeartbeatResult::Ok;
        }
    }

    HeartbeatResult::Alert
}
```

The logic is a two-gate AND: the response must be *short* (below
`ack_max_chars`) *and* contain one of the ack patterns. The patterns live in
a static slice at `crates/ryvos-agent/src/heartbeat.rs:26`:

```rust
const ACK_PATTERNS: &[&str] = &[
    "HEARTBEAT_OK",
    "heartbeat_ok",
    "all good",
    "no issues",
    "nothing to report",
    "everything is fine",
    "all clear",
];
```

Both conditions matter. A long response like "HEARTBEAT_OK but also disk is
at 95%" is an alert — the alert content is buried in a long message and the
user should see it. A short response like "Check the logs" is also an alert
because there is no ack phrase, so the model probably does mean "check the
logs". Only `"HEARTBEAT_OK"` on its own, or "All good, nothing to report"
on its own, survives the gate.

The `ack_max_chars` default is 300, defined at
`crates/ryvos-core/src/config.rs:597`. There is a historical wobble here:
the threshold was bumped to 2000 briefly, then reverted to 300. The reason
is that 2000 chars lets a chatty model smuggle alerts through the gate
inside an otherwise-ack-flavored response, which defeats the whole point.
Three hundred is big enough to accommodate "HEARTBEAT_OK — I checked the
workspace, all services are running, disk at 62%, git clean" and small
enough that an actionable observation cannot hide.

The two outcomes are published as distinct events. See
`crates/ryvos-agent/src/heartbeat.rs:170`:

```rust
match evaluate_response(&response, self.config.ack_max_chars) {
    HeartbeatResult::Ok => {
        info!(session = %session_id, chars = response.len(),
              "Heartbeat OK (suppressed)");
        self.event_bus.publish(AgentEvent::HeartbeatOk {
            session_id,
            response_chars: response.len(),
        });
    }
    HeartbeatResult::Alert => {
        warn!(session = %session_id, "Heartbeat alert");
        self.event_bus.publish(AgentEvent::HeartbeatAlert {
            session_id,
            message: response,
            target_channel: self.config.target_channel.clone(),
        });
    }
}
```

`HeartbeatOk` carries only the response length (for UI counters), not the
content. Nothing useful would be in it. `HeartbeatAlert` carries the full
response text plus the optional `target_channel` hint.

## Channel routing

`HeartbeatAlert` and `HeartbeatOk` are **[EventBus](../glossary.md#eventbus)**
events, and the channel subsystem subscribes to them. The router lives in
`ChannelDispatcher::run` at `crates/ryvos-channels/src/dispatch.rs:101`:

```rust
let mut hb_rx = self.event_bus.subscribe();
let adapters = self.adapters.clone();
let hb_cancel = self.cancel.clone();
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = hb_cancel.cancelled() => break,
            event = hb_rx.recv() => {
                let (content, target_channel) = match event {
                    Ok(AgentEvent::HeartbeatAlert { message, target_channel, .. }) => {
                        (MessageContent::Text(format!("[Heartbeat Alert] {}", message)),
                         target_channel)
                    }
                    Ok(AgentEvent::HeartbeatOk { response_chars, .. }) => {
                        (MessageContent::Text(format!(
                            "[Heartbeat] All clear ({} chars)", response_chars)), None)
                    }
                    Ok(AgentEvent::CronJobComplete { name, response, channel }) => {
                        let msg = format!("[Cron: {}] {}", name, response);
                        (MessageContent::Text(msg), channel)
                    }
                    _ => continue,
                };

                if let Some(ref channel) = target_channel {
                    if let Some(adapter) = adapters.get(channel) {
                        adapter.broadcast(&content).await.ok();
                    }
                } else {
                    for adapter in adapters.values() {
                        adapter.broadcast(&content).await.ok();
                    }
                }
            }
        }
    }
});
```

Two things to note. First, `HeartbeatOk` passes `None` as the target
channel, so any subscriber that happens to route ok events would broadcast
to every adapter — but in practice, the channel dispatcher only routes ok
events when a *specific* channel is configured, and most deployments leave
`target_channel` unset. The result: `HeartbeatOk` is almost always
suppressed at the publishing side (nothing subscribes that cares) and the
user sees nothing. Second, the alert path prepends `[Heartbeat Alert]` to
the message so users scanning a busy Telegram channel can tell a heartbeat
report apart from a reply to their own question. The same router also
handles `CronJobComplete` (see [cron-scheduler.md](cron-scheduler.md)),
which is why three event arms are matched in one `match`.

## Interaction with the Guardian

The Heartbeat and the **[Guardian](../glossary.md#guardian)** both touch
the same runtime but at different levels. The Guardian is a background
watchdog that subscribes to `RunStarted`, `TurnComplete`, `ToolStart`,
and the other run-scoped events and reacts to pathologies (doom loops,
stalls, budget overruns) by injecting hints or cancelling runs. The
Heartbeat is a timer-driven generator that produces `RunStarted` events
from its own cycles. The Guardian does not distinguish heartbeat runs
from user runs — it watches them the same way, and it will terminate a
stalled or looping heartbeat run with the same logic it uses on a
Telegram-initiated conversation. This is why the heartbeat prompt is
short and the template is structured as a checklist: a long, open-ended
prompt would be more likely to trip the Guardian's stall timer.

Budget events are also shared. A heartbeat run that pushes monthly spend
over the warn or hard threshold will fire `BudgetWarning` or
`BudgetExceeded` just like any other run. On the hard threshold, the
Guardian cancels the run through the shared cancellation token — so a
running heartbeat cycle gets cut off mid-LLM-call if it happens to be
the straw that breaks the budget. This is a deliberate design choice:
the Heartbeat is not privileged over user runs for budget purposes, and
the user can disable the Heartbeat entirely via `enabled = false` if
they want a strict budget ceiling.

## The cancel token chain

The `CancellationToken` passed into `Heartbeat::new` is a child of the
daemon's root token. When the daemon shuts down (`ryvos stop`, systemd
stop, `SIGTERM`), the root token fires, the child tokens on the
Heartbeat, Cron scheduler, channel dispatcher, and gateway all fire,
and each subsystem exits its loop cleanly. The Heartbeat in particular
will finish the current cycle before exiting if it was already running
the agent when the cancel fired — the `tokio::select!` at the top of
the loop is the only place cancellation is checked, and a running cycle
does not re-enter the select until the agent run returns. For a 30-
minute interval this is fine; for integration testing with short
intervals, tests use the cancel token directly rather than waiting for
graceful shutdown.

## Cross-references

- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — crate overview;
  Heartbeat is one of five top-level subsystems.
- [../crates/ryvos-channels.md](../crates/ryvos-channels.md) — channel
  adapter trait and the dispatcher that routes heartbeat events.
- [../architecture/data-flow.md](../architecture/data-flow.md) — where the
  Heartbeat sits in the event flow.
- [guardian.md](guardian.md) — the reactive counterpart: the Guardian
  subscribes to events rather than generating them.
- [safety-memory.md](safety-memory.md) — the store the safety retrospective
  reads from and writes to.
- [session-manager.md](session-manager.md) — the owner of the
  `heartbeat:` session-id prefix.
