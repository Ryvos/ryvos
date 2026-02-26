# Contributing to Ryvos

Thanks for your interest in contributing to Ryvos! This guide will help you get started.

## Getting Started

### Prerequisites

- [Rust 1.75+](https://rustup.rs/)
- [Docker](https://docs.docker.com/get-docker/) (optional, for sandbox testing)

### Setup

```bash
git clone https://github.com/Ryvos/ryvos.git
cd ryvos
cargo build
```

### Running Tests

```bash
cargo test                    # All tests
cargo test -p ryvos-agent     # Single crate
cargo test -- --nocapture     # With stdout
```

### Running Locally

```bash
cargo run -- init -y --provider ollama --model-id qwen2.5:7b
cargo run
```

## How to Contribute

### Reporting Bugs

Open a [bug report](https://github.com/Ryvos/ryvos/issues/new?template=bug_report.yml) with:
- Steps to reproduce
- Expected vs actual behavior
- Ryvos version (`ryvos --version`)
- OS and architecture

### Suggesting Features

Open a [feature request](https://github.com/Ryvos/ryvos/issues/new?template=feature_request.yml) describing:
- The problem you're trying to solve
- Your proposed solution
- Alternatives you've considered

### Pull Requests

1. Fork the repo and create a branch from `main`
2. If you've added code, add tests
3. Ensure `cargo test` passes
4. Ensure `cargo clippy` has no warnings
5. Run `cargo fmt` before committing
6. Open a PR with a clear description of the change

### First Time?

Look for issues labeled [`good first issue`](https://github.com/Ryvos/ryvos/labels/good%20first%20issue) — these are scoped, well-defined tasks designed for new contributors.

## Project Structure

Ryvos is a Cargo workspace with 10 crates:

| Crate | What it does |
|-------|-------------|
| `ryvos-core` | Config, error types, event bus, security policy, goal system |
| `ryvos-llm` | LLM client abstraction (Anthropic, OpenAI, Ollama) |
| `ryvos-tools` | Tool registry + 11 built-in tools |
| `ryvos-agent` | ReAct loop, SecurityGate, Judge, DAG engine |
| `ryvos-memory` | SQLite session and history storage |
| `ryvos-gateway` | HTTP/WS server + Web UI |
| `ryvos-channels` | Telegram, Discord, Slack adapters |
| `ryvos-mcp` | MCP client (stdio + SSE) |
| `ryvos-skills` | Lua/Rhai skill loader |
| `ryvos-tui` | Terminal UI (ratatui) |

## Code Style

- Follow standard Rust conventions
- Use `cargo fmt` (rustfmt) for formatting
- Use `cargo clippy` for linting
- Keep functions focused and small
- Add doc comments for public APIs
- Error messages should be actionable ("failed to connect to Ollama at localhost:11434" not "connection error")

## Commit Messages

Use conventional commit style:

```
feat(agent): add checkpoint resume on crash
fix(security): prevent tier escalation bypass in sub-agents
docs: update quick start for Ollama setup
test(tools): add edge cases for glob pattern matching
```

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
