# Architecture Decision Records

This directory captures the key architectural decisions made for the Ryvos project.
Each ADR documents the context, the decision, and its consequences, both positive
and negative. We keep these as a living record so future contributors understand
not just what we built, but why we built it this way.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-rust-runtime.md) | Rust for the agent runtime | Accepted |
| [002](002-passthrough-security.md) | Passthrough security instead of blocking | Accepted |
| [003](003-viking-hierarchical-memory.md) | Viking memory with FTS5 | Accepted |
| [004](004-cli-provider-pattern.md) | CLI provider for Claude Code and Copilot | Accepted |
| [005](005-event-driven-architecture.md) | Event-driven pub/sub | Accepted |
| [006](006-separate-sqlite-databases.md) | Separate SQLite DBs per subsystem | Accepted |
| [007](007-embedded-svelte-web-ui.md) | Embedded Svelte UI via rust_embed | Accepted |
| [008](008-mcp-integration-layer.md) | MCP as the integration layer | Accepted |
| [009](009-director-ooda-loop.md) | Director OODA loop for goals | Accepted |
| [010](010-channel-adapter-pattern.md) | Channel adapter trait | Accepted |

## Format

Each ADR follows a simple structure:

- **Status**: Accepted, Superseded, or Deprecated
- **Context**: What problem or situation led to this decision
- **Decision**: What we decided and why
- **Consequences**: What follows, both good and bad

## Contributing

When making a significant architectural decision, create a new ADR with the next
number in sequence. Don't modify accepted ADRs. If a decision changes, create a
new ADR that supersedes the old one and update the old one's status.
