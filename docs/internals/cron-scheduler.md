# Cron scheduler

The cron scheduler is the third wake-up path in Ryvos, alongside
user-triggered channel messages and the **[Heartbeat](../glossary.md#heartbeat)**.
It reads a list of job specs from the config, parses each one's schedule
using the `cron` crate's standard five-field expressions, sleeps until
the next one fires, runs the agent against the job's prompt, and
publishes a completion event that the channel dispatcher routes to the
configured target. Since v0.8.2, jobs can optionally carry a goal
description, in which case the run goes through the
**[Director](../glossary.md#director)**'s OODA loop instead of the
standard ReAct path.

This document walks `crates/ryvos-agent/src/scheduler.rs:1-166`, the
config types at `crates/ryvos-core/src/config.rs:604-622`, the channel
routing in `crates/ryvos-channels/src/dispatch.rs:118-121`, and the
interaction with `Goal` and `run_with_goal`.

## Job spec

A cron job is defined by a `CronJobConfig` in
`crates/ryvos-core/src/config.rs:612`:

```rust
pub struct CronJobConfig {
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
}
```

Five fields. `name` is a user-facing label that appears in logs and in
the channel-delivered response; it is also used as the session id
suffix so each job has a stable conversation thread. `schedule` is a
five-or-six-field cron expression that the `cron` crate parses into a
`Schedule` iterator. `prompt` is the user message the agent will see
when the job fires. `channel` is the optional target channel adapter
name — `"telegram"`, `"discord"`, `"slack"`, `"whatsapp"` — and `None`
means broadcast to every adapter. `goal` is optional: when present, the
job goes through the Director; when absent, it runs through the
standard agent loop.

The parent `CronConfig` at `crates/ryvos-core/src/config.rs:605` is
just a wrapper around `Vec<CronJobConfig>`. The TOML shape is the
familiar array-of-tables:

```toml
[[cron.jobs]]
name = "morning-brief"
schedule = "0 0 9 * * *"
prompt = "Summarize what happened overnight."
channel = "telegram"

[[cron.jobs]]
name = "disk-check"
schedule = "0 */30 * * * *"
prompt = "Check disk usage and warn if any partition is over 90%."
goal = "A clear disk status summary with explicit pass/fail per partition."
```

The `cron` crate expects six fields (seconds-minutes-hours-dom-month-
dow), not the five-field POSIX shape. `"0 0 9 * * *"` is "at second 0,
minute 0, hour 9, every day of every month, any day of week" — nine in
the morning, every day. This is a behavior difference from system cron
and catches new users; the operations configuration guide
[../operations/configuration.md](../operations/configuration.md) flags
it explicitly.

## Internal job representation

`CronScheduler::new` parses the config into an internal `CronJob`
struct. See `crates/ryvos-agent/src/scheduler.rs:17`:

```rust
struct CronJob {
    name: String,
    schedule: Schedule,
    prompt: String,
    #[allow(dead_code)]
    channel: Option<String>,
    goal: Option<String>,
}
```

The `schedule` field is the parsed `cron::Schedule`, not the string.
Parsing happens once at construction and any invalid expression is
logged and skipped, not a fatal error. The skip means a malformed job
does not break the other jobs in the config — the scheduler continues
with whatever parsed successfully. See
`crates/ryvos-agent/src/scheduler.rs:43`:

```rust
for job_config in &config.jobs {
    match Schedule::from_str(&job_config.schedule) {
        Ok(schedule) => {
            jobs.push(CronJob {
                name: job_config.name.clone(),
                schedule,
                prompt: job_config.prompt.clone(),
                channel: job_config.channel.clone(),
                goal: job_config.goal.clone(),
            });
            info!(name = %job_config.name, schedule = %job_config.schedule,
                  "Cron job registered");
        }
        Err(e) => {
            warn!(
                name = %job_config.name,
                schedule = %job_config.schedule,
                error = %e,
                "Invalid cron expression, skipping job"
            );
        }
    }
}
```

Note the `#[allow(dead_code)]` on `channel`: the field is read when the
scheduler publishes the `CronJobComplete` event, but Rust's dead-code
analysis cannot see that because the field is moved into the event
payload rather than accessed via `job.channel` inside the scheduler's
`run` method. The annotation is a workaround, not a bug.

## The main loop

`CronScheduler::run` is a loop that finds the earliest upcoming fire,
sleeps until then, and dispatches. See
`crates/ryvos-agent/src/scheduler.rs:75`:

```rust
pub async fn run(&self) {
    if self.jobs.is_empty() {
        info!("No cron jobs configured, scheduler idle");
        self.cancel.cancelled().await;
        return;
    }

    info!(count = self.jobs.len(), "Cron scheduler started");

    loop {
        // Find the next job to fire
        let now = Utc::now();
        let mut next_fire: Option<(chrono::DateTime<Utc>, &CronJob)> = None;

        for job in &self.jobs {
            if let Some(next) = job.schedule.upcoming(Utc).next() {
                if next_fire.is_none() || next < next_fire.unwrap().0 {
                    next_fire = Some((next, job));
                }
            }
        }

        if let Some((fire_at, job)) = next_fire {
            let delay = (fire_at - now).to_std().unwrap_or(Duration::from_secs(1));
            info!(job = %job.name, fire_at = %fire_at.format("%H:%M:%S"),
                  delay_secs = delay.as_secs(), "Next cron job scheduled");
            /* ... tokio::select! on sleep and cancel ... */
        } else {
            // No jobs have upcoming times, wait until cancelled
            self.cancel.cancelled().await;
            break;
        }
    }
}
```

The idle-scheduler early return at the top handles the no-jobs case
cheaply: if the user has no cron configured, the scheduler blocks on
the cancellation token forever without spending CPU on empty iteration.
This is the common case — most Ryvos deployments do not use cron.

The main loop's selection pass iterates every job, asks
`Schedule::upcoming(Utc).next()` for the next fire time, and tracks the
earliest across all jobs. This is an O(N) scan per wake-up, which is
fine for the kind of numbers cron jobs come in (tens, maybe hundreds,
never thousands). The selection is recomputed every iteration, so
adding a new job would be handled if the scheduler rebuilt its
internal list — but the current implementation does not rebuild;
config changes require a daemon restart, covered in the restart
section below.

The `delay.to_std().unwrap_or(Duration::from_secs(1))` handles the
edge case where `fire_at` is already in the past (negative duration):
`to_std` returns `None` for negative durations, and the `unwrap_or`
collapses that to a 1-second delay. This happens when a job fires
faster than the scheduler can loop — rare but possible during heavy
load — and the 1-second fallback means the scheduler will not busy-
loop on a back-dated fire.

## Fire dispatch

The fire block is the inside of the `tokio::select!`. See
`crates/ryvos-agent/src/scheduler.rs:107`:

```rust
tokio::select! {
    _ = tokio::time::sleep(delay) => {
        info!(job = %job.name, "Firing cron job");

        self.event_bus.publish(AgentEvent::CronFired {
            job_id: job.name.clone(),
            prompt: job.prompt.clone(),
        });

        let session_id = SessionId::from_string(&format!("cron:{}", job.name));

        // Use Director orchestration when goal is configured
        let run_result = if let Some(ref goal_desc) = job.goal {
            let goal = Goal {
                description: goal_desc.clone(),
                success_criteria: vec![SuccessCriterion {
                    id: "llm_judge".into(),
                    criterion_type: CriterionType::LlmJudge {
                        prompt: format!("Did the agent achieve this goal: {}?", goal_desc),
                    },
                    weight: 1.0,
                    description: "Goal achievement".into(),
                }],
                constraints: vec![],
                success_threshold: 0.7,
                version: 0,
                metrics: Default::default(),
            };
            info!(job = %job.name, "Cron job using Director orchestration");
            self.runtime.run_with_goal(&session_id, &job.prompt, Some(&goal)).await
        } else {
            self.runtime.run(&session_id, &job.prompt).await
        };

        match run_result {
            Ok(response) => {
                info!(job = %job.name, "Cron job completed");
                self.event_bus.publish(AgentEvent::CronJobComplete {
                    name: job.name.clone(),
                    response,
                    channel: job.channel.clone(),
                });
            }
            Err(e) => error!(job = %job.name, error = %e, "Cron job failed"),
        }
    }
    _ = self.cancel.cancelled() => {
        info!("Cron scheduler shutting down");
        break;
    }
}
```

Three things happen after the sleep returns. First, a `CronFired`
event is published. This is a "job is starting" signal that the
gateway UI and the JSONL run log subscribe to — neither needs to act
on it, but it is useful for visualizing scheduled activity. Second,
the session id is built as `cron:{job.name}`. This is a stable id:
two fires of the same job share a session, which means the agent's
conversation history persists across fires and the next fire can
reference what happened on the previous one. A daily morning brief
that accumulates context over a week works because of this session-
id stability. Third, the run dispatch branches on whether `job.goal`
is set.

## Goal-driven cron

The goal branch is the v0.8.2 feature. When `goal` is set, the
scheduler builds a `Goal` object on the fly and passes it to
`runtime.run_with_goal`. The goal it builds is minimal: one
`LlmJudge` criterion, weight 1.0, threshold 0.7, no constraints. The
LLM-judge prompt is templated as `"Did the agent achieve this goal:
{goal_desc}?"`, which is deliberately loose — the goal description is
whatever the user wrote in the TOML, and the judge is asked to make
a pass/fail determination against it.

The threshold of 0.7 is lower than the default 0.9 used elsewhere in
the goal system. Cron jobs are generally best-effort and the user does
not want a 70% confidence result to be retried indefinitely. Setting
the threshold to 0.7 means the judge only retries when it is fairly
sure the job failed, rather than when it is not completely sure the
job succeeded.

The important consequence of the goal branch is that the run goes
through the Director. See `AgentRuntime::run_with_goal` at
`crates/ryvos-agent/src/agent_loop.rs:233`: when a goal is provided
and `[agent.director] enabled = true`, control transfers to
`run_with_director`, which runs the full OODA loop described in
[director-ooda.md](director-ooda.md). A cron job with a goal can
therefore do all the Director things — DAG planning, evolution,
failure diagnosis, plan retry — without the user writing any Rust.
It is the simplest path from TOML to an autonomous multi-step
workflow.

The no-goal branch uses `runtime.run`, which is the standard ReAct
loop. This is the right default for "run this prompt every morning"
where the prompt already describes what to do and no goal inference
is needed.

## Completion and channel routing

On success, the scheduler publishes a `CronJobComplete` event with
the response, the job name, and the optional target channel. The
channel dispatcher subscribes to this event alongside heartbeat
events. See `crates/ryvos-channels/src/dispatch.rs:118`:

```rust
Ok(AgentEvent::CronJobComplete { name, response, channel }) => {
    let msg = format!("[Cron: {}] {}", name, response);
    (MessageContent::Text(msg), channel)
}
```

The message is prefixed with `[Cron: {name}]` so the user can tell at
a glance which job produced it. The `channel` field from the event
drives the routing: `Some("telegram")` sends only to the Telegram
adapter, `None` broadcasts to every adapter. The routing logic is
shared with `HeartbeatAlert` — see [heartbeat.md](heartbeat.md) for
the full `if let Some(ref channel) = target_channel` dispatch block.

On failure, the scheduler logs an error and does *not* publish a
completion event. See `crates/ryvos-agent/src/scheduler.rs:150`:

```rust
Err(e) => error!(job = %job.name, error = %e, "Cron job failed"),
```

A failed job simply produces a log line. The loop continues to the
next iteration and the job will fire again at its next scheduled time.
This is deliberate: cron jobs are expected to be resilient, and a
transient failure (network blip, upstream LLM 503) should not cause
any kind of escalation. Users who want alerting on cron failures
should put the alerting logic in the prompt itself — "if anything
goes wrong, send a message via the tele tool" — not in the scheduler.

## Timezones

The scheduler evaluates cron expressions in UTC. Every call to
`Schedule::upcoming(Utc)` produces UTC times, and `Utc::now()` is the
reference for "now". There is no configuration for local-time cron.
This is a simplification that aligns with standard cron practice on
servers (system cron usually runs in UTC in production) but catches
desktop users who want "every morning at 9 AM local time".

The workaround is to convert by hand. A user in UTC+2 who wants a
9 AM local fire should schedule `"0 0 7 * * *"` (7 AM UTC). The
configuration guide [../operations/configuration.md](../operations/configuration.md)
documents this explicitly with a conversion table for common offsets.
Supporting local-time cron would require threading a timezone into
the scheduler and using `Schedule::upcoming(tz)` for a non-UTC
timezone, which is a small change but has not been prioritized.

## Adding and removing jobs

Cron config is part of `ryvos.toml` and is loaded once at daemon
startup. The web UI exposes a PUT `/api/config` endpoint that writes
the TOML back to disk, but the scheduler does not watch for config
changes — adding or removing a job requires a daemon restart to pick
up the new schedule. This is the same restart requirement as every
other config section; the ADR on configuration hot-reloading (still
an open issue at v0.8.3) documents why.

The symptom for users who miss this: they add a job through the UI,
save the config successfully, wait for it to fire, nothing happens.
The fix is `systemctl restart ryvos` or equivalent. The web UI surfaces
a "restart required" banner when it detects a config change that is
not yet reflected in the running daemon's state.

## Cross-references

- [agent-loop.md](agent-loop.md) — `runtime.run` and `runtime.run_with_goal`,
  the two dispatch targets for the scheduler.
- [director-ooda.md](director-ooda.md) — what happens inside
  `run_with_goal` when `[agent.director]` is enabled, which is the full
  behavior of goal-driven cron.
- [heartbeat.md](heartbeat.md) — the sibling timer-driven subsystem;
  shares the channel dispatch routing.
- [../architecture/data-flow.md](../architecture/data-flow.md) — where
  the scheduler sits in the wake-up paths.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — crate overview.
- [../crates/ryvos-channels.md](../crates/ryvos-channels.md) — the
  dispatcher that routes `CronJobComplete` to adapters.
- [../operations/configuration.md](../operations/configuration.md) —
  the `[cron.jobs]` TOML reference and timezone guidance.
- [../api/gateway-rest.md](../api/gateway-rest.md) — PUT `/api/config`
  for adding and removing jobs via the web UI.
