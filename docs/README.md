# Ryvos Developer Documentation

This directory is the developer reference for Ryvos, an open-source AI agent
runtime built in Rust. It is meant for contributors who want to understand the
codebase, extend it, operate it in production, or embed it in another system.
End-user installation and quick-start material lives in the top-level
[README.md](../README.md); the content here assumes you already have Ryvos
running and want to know how it works.

The docs are organized as a hierarchy. The top of the tree is this README plus
a handful of foundation documents (the glossary, the style guide, and the
architecture overview). Below those sit four reference sections: per-crate
deep dives, runtime internals, operational guides, and API references. The
Architecture Decision Records in [adr/](adr/) are a separate track that
documents *why* the system is built the way it is.

Ryvos is currently at v0.8.3. Feature coverage and code references in this doc
set are pinned to that release. When the codebase moves ahead of the docs, file
an issue — the doc set is intended to stay within one minor release of `main`.

## How to read this set

If you are new to Ryvos and want a grounding, read in this order:

1. [architecture/system-overview.md](architecture/system-overview.md) — the ten
   crates, how they stack, and what crosses the boundaries between them.
2. [architecture/execution-model.md](architecture/execution-model.md) — how a
   single user message becomes an agent run, and how the [Director](glossary.md#director),
   [Guardian](glossary.md#guardian), and [Heartbeat](glossary.md#heartbeat) run
   concurrently alongside the agent loop.
3. [crates/README.md](crates/README.md) — pick a crate that matches your
   interest and follow it to the detailed reference.
4. [glossary.md](glossary.md) — keep this open in another tab; every domain
   term used in the docs is defined there exactly once.

If you are trying to extend Ryvos, jump straight to:

- [internals/tool-registry.md](internals/tool-registry.md) — to add a built-in
  tool.
- [crates/ryvos-skills.md](crates/ryvos-skills.md) — to add a drop-in skill.
- [crates/ryvos-mcp.md](crates/ryvos-mcp.md) — to wire in an external MCP
  server.
- [crates/ryvos-channels.md](crates/ryvos-channels.md) — to add a new
  messaging channel.
- [crates/ryvos-llm.md](crates/ryvos-llm.md) — to add an LLM provider.

If you are operating Ryvos in production, start with
[operations/deployment.md](operations/deployment.md) and follow the guides.

## Foundation

| Document | Purpose |
|---|---|
| [README.md](README.md) | This file. Top-level index and reading path. |
| [STYLE.md](STYLE.md) | Writer's contract for everything under `docs/`. |
| [glossary.md](glossary.md) | Canonical definitions for every domain term. |
| [CHANGELOG.md](../CHANGELOG.md) | Release history. |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | How to contribute code and docs. |

## Architecture

High-level documents that describe Ryvos as a system rather than as a collection
of crates.

| Document | Purpose |
|---|---|
| [architecture/system-overview.md](architecture/system-overview.md) | The ten-crate dependency graph, the architectural layers, and the invariants every component upholds. |
| [architecture/execution-model.md](architecture/execution-model.md) | How the agent loop, Director, Guardian, and Heartbeat execute and interact at runtime. |
| [architecture/data-flow.md](architecture/data-flow.md) | End-to-end path of a user message through channel adapter, session manager, agent, LLM, tools, and back. |
| [architecture/context-composition.md](architecture/context-composition.md) | The three-layer [onion context](glossary.md#onion-context) and when each layer is rebuilt. |
| [architecture/concurrency-model.md](architecture/concurrency-model.md) | Tokio task topology, EventBus delivery, and cancellation semantics. |
| [architecture/persistence.md](architecture/persistence.md) | The seven SQLite databases, their schemas, and their isolation boundaries. |

## Crate reference

One document per workspace crate. The navigator is [crates/README.md](crates/README.md).

| Document | Crate | Purpose |
|---|---|---|
| [crates/ryvos-core.md](crates/ryvos-core.md) | `ryvos-core` | Traits, types, config, errors, [EventBus](glossary.md#eventbus), goal system. |
| [crates/ryvos-llm.md](crates/ryvos-llm.md) | `ryvos-llm` | LLM client abstraction and 18+ provider implementations. |
| [crates/ryvos-tools.md](crates/ryvos-tools.md) | `ryvos-tools` | Built-in tools and the [tool registry](glossary.md#tool-registry). |
| [crates/ryvos-agent.md](crates/ryvos-agent.md) | `ryvos-agent` | Agent loop, Director, Guardian, Judge, Reflexion, checkpoint store. |
| [crates/ryvos-memory.md](crates/ryvos-memory.md) | `ryvos-memory` | Session store, history store, and [Viking](glossary.md#viking) local store. |
| [crates/ryvos-mcp.md](crates/ryvos-mcp.md) | `ryvos-mcp` | MCP client and MCP server implementations. |
| [crates/ryvos-gateway.md](crates/ryvos-gateway.md) | `ryvos-gateway` | Axum HTTP/WebSocket gateway, Web UI, role-based auth. |
| [crates/ryvos-channels.md](crates/ryvos-channels.md) | `ryvos-channels` | Telegram, Discord, Slack, and WhatsApp adapters. |
| [crates/ryvos-skills.md](crates/ryvos-skills.md) | `ryvos-skills` | TOML skill loader and Lua/Rhai runtime. |
| [crates/ryvos-tui.md](crates/ryvos-tui.md) | `ryvos-tui` | Ratatui terminal UI. |
| [crates/ryvos-test-utils.md](crates/ryvos-test-utils.md) | `ryvos-test-utils` | Shared test fixtures and mock implementations. |

## Internals

Runtime subsystems and cross-crate concerns. These are the deep dives writers
link to when a crate reference mentions a subsystem in passing.

| Document | Purpose |
|---|---|
| [internals/agent-loop.md](internals/agent-loop.md) | State machine of the ReAct loop, from context build to turn end. |
| [internals/director-ooda.md](internals/director-ooda.md) | DAG generation, node evaluation, and plan evolution. |
| [internals/guardian.md](internals/guardian.md) | [Doom loop](glossary.md#doom-loop) detection, stall timers, and budget enforcement. |
| [internals/heartbeat.md](internals/heartbeat.md) | Timer-driven self-checks and alert suppression. |
| [internals/judge.md](internals/judge.md) | Level 0 deterministic checks and Level 2 LLM verdicts. |
| [internals/safety-memory.md](internals/safety-memory.md) | Lesson schema, outcome classification, reinforcement, and pruning. |
| [internals/reflexion.md](internals/reflexion.md) | Failure journal, hint construction, injection timing. |
| [internals/tool-registry.md](internals/tool-registry.md) | Tool trait, registration, dispatch, and parallel execution. |
| [internals/mcp-bridge.md](internals/mcp-bridge.md) | Transport selection, capability negotiation, and tool proxying. |
| [internals/event-bus.md](internals/event-bus.md) | Broadcast semantics, subscriber lifecycle, and filtered subscriptions. |
| [internals/checkpoint-resume.md](internals/checkpoint-resume.md) | Turn snapshots, crash recovery, and session continuity. |
| [internals/session-manager.md](internals/session-manager.md) | Session lifetime, routing, and per-channel isolation. |
| [internals/cost-tracking.md](internals/cost-tracking.md) | Per-turn cost accumulation, API vs subscription classification, and budget events. |
| [internals/cron-scheduler.md](internals/cron-scheduler.md) | Persistent cron, Director routing, and result dispatch. |
| [internals/graph-executor.md](internals/graph-executor.md) | DAG node execution, edge conditions, and handoff context. |
| [internals/output-validator.md](internals/output-validator.md) | Heuristic repair and optional LLM repair of structured output. |

## Guides

Task-oriented walkthroughs for common extensions and integrations.

| Document | Purpose |
|---|---|
| [guides/adding-a-tool.md](guides/adding-a-tool.md) | Implement the `Tool` trait and register a new built-in tool. |
| [guides/adding-a-skill.md](guides/adding-a-skill.md) | Package a TOML skill with a Lua or Rhai script. |
| [guides/adding-an-llm-provider.md](guides/adding-an-llm-provider.md) | Implement `LlmClient` for a new provider. |
| [guides/adding-a-channel.md](guides/adding-a-channel.md) | Implement `ChannelAdapter` for a new messaging platform. |
| [guides/wiring-an-mcp-server.md](guides/wiring-an-mcp-server.md) | Configure an external MCP server over stdio or streamable HTTP. |
| [guides/writing-a-goal.md](guides/writing-a-goal.md) | Define weighted success criteria and constraints. |
| [guides/customizing-the-soul.md](guides/customizing-the-soul.md) | Shape agent tone via `SOUL.md` and the soul interview. |
| [guides/configuring-safety.md](guides/configuring-safety.md) | Choose constitutional principles, soft checkpoints, and budget limits. |
| [guides/debugging-runs.md](guides/debugging-runs.md) | Read the JSONL run log, the audit trail, and the decision journal. |
| [guides/migrating-from-tier-security.md](guides/migrating-from-tier-security.md) | Move from pre-v0.6 blocking security to passthrough. |

## API reference

Surface-level docs for the HTTP and MCP interfaces that Ryvos exposes.

| Document | Purpose |
|---|---|
| [api/gateway-rest.md](api/gateway-rest.md) | 40+ REST endpoints exposed by `ryvos-gateway`. |
| [api/gateway-websocket.md](api/gateway-websocket.md) | WebSocket frames, [lanes](glossary.md#lane), and event subscriptions. |
| [api/mcp-server.md](api/mcp-server.md) | The nine tools Ryvos exposes when acting as an MCP server. |
| [api/auth-and-rbac.md](api/auth-and-rbac.md) | Role-based API keys (Viewer, Operator, Admin). |
| [api/webhook-format.md](api/webhook-format.md) | Payload schema for outbound webhooks (`callback_url`, `metadata`). |

## Operations

Running Ryvos in production or as a personal always-on daemon.

| Document | Purpose |
|---|---|
| [operations/deployment.md](operations/deployment.md) | Systemd, Docker, and Fly.io deployment patterns. |
| [operations/configuration.md](operations/configuration.md) | Every `ryvos.toml` key, grouped by subsystem. |
| [operations/environment-variables.md](operations/environment-variables.md) | `${VAR}` expansion and the full list of consumed env vars. |
| [operations/backup-and-restore.md](operations/backup-and-restore.md) | Backing up the seven SQLite databases. |
| [operations/monitoring.md](operations/monitoring.md) | Exporting events, reading logs, and health checks. |
| [operations/upgrading.md](operations/upgrading.md) | Version-to-version upgrade notes and migrations. |
| [operations/troubleshooting.md](operations/troubleshooting.md) | Common failures and how to diagnose them. |

## Architecture Decision Records

The ADRs live in [adr/](adr/) and are the canonical record of *why* Ryvos is
built the way it is. There are currently ten accepted ADRs. See
[adr/README.md](adr/README.md) for the full index. Foundation documents link to
ADRs rather than restating their arguments.

| # | Title |
|---|---|
| [001](adr/001-rust-runtime.md) | Rust for the agent runtime |
| [002](adr/002-passthrough-security.md) | Passthrough security instead of blocking |
| [003](adr/003-viking-hierarchical-memory.md) | Viking memory with FTS5 |
| [004](adr/004-cli-provider-pattern.md) | CLI provider for Claude Code and Copilot |
| [005](adr/005-event-driven-architecture.md) | Event-driven pub/sub |
| [006](adr/006-separate-sqlite-databases.md) | Separate SQLite DBs per subsystem |
| [007](adr/007-embedded-svelte-web-ui.md) | Embedded Svelte UI via `rust_embed` |
| [008](adr/008-mcp-integration-layer.md) | MCP as the integration layer |
| [009](adr/009-director-ooda-loop.md) | Director OODA loop for goals |
| [010](adr/010-channel-adapter-pattern.md) | Channel adapter trait |

## Conventions used throughout this set

- **Crate names** are kebab-case (`ryvos-agent`), even though the Rust
  identifier is `ryvos_agent`. This matches `Cargo.toml` and the ADRs.
- **Source references** use `crates/ryvos-agent/src/agent_loop.rs:142` form,
  always from the repo root.
- **Glossary terms** are bold-linked on first use in each document. See
  [STYLE.md](STYLE.md) for the full writer's contract.
- **Diagrams** are Mermaid only, rendered natively by GitHub.
- **No emoji** anywhere in the doc set.

## Contributing to the docs

Doc contributions follow the same flow as code contributions. Open a PR against
`main`, keep it within one minor release of the codebase, and run
`cargo fmt --check` plus `cargo clippy` before pushing — doc-only PRs still run
CI. If you are adding a new document, first check that the filename appears in
this README's tables; if not, add it to the relevant table in the same PR so
future writers can find it.
