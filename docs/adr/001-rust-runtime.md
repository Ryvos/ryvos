# ADR-001: Rust for the Agent Runtime

## Status

Accepted

## Context

Most AI agent frameworks are built in TypeScript or Python. That makes sense for
quick prototyping, but Ryvos has a different deployment target. We need an agent
runtime that can run 24/7 on edge hardware like a Raspberry Pi, a NAS, or a small
home server. It needs to start instantly, use minimal memory, and survive long
uptimes without degradation.

Python has the GIL, garbage collection pauses, and high baseline memory usage.
Even a slim Python process with asyncio sits at 80MB+ RSS before you load a
single library. TypeScript on Node.js is better, but still carries V8's overhead
and tends to balloon in memory over days of continuous operation.

We also wanted a single static binary. No pip install, no npm install, no Docker
required. Download one file, run it. That rules out interpreted languages unless
you bundle the entire runtime, which defeats the purpose.

The team had existing Rust experience, which lowered the barrier. We were not
starting from zero.

## Decision

We chose Rust with the tokio async runtime as the foundation for the entire Ryvos
agent system. The binary is compiled as a single static executable with all assets
(including the web UI) embedded at build time.

Key technical choices that follow from this:

- **tokio** for async I/O, task spawning, broadcast channels, and timers.
- **reqwest** for HTTP client operations (API calls to LLM providers).
- **rusqlite** for all persistence (SQLite, no external database server).
- **rust_embed** for baking static frontend assets into the binary.
- **serde/serde_json** for all serialization, which Rust's ecosystem handles well.

We compile with `--release` and strip debug symbols. The result is a single file
you can scp to a server and run.

## Consequences

**What went well:**

- The release binary is 45MB. Compressed, it fits in a GitHub release artifact
  with room to spare.
- Runtime memory sits at roughly 57MB RSS during normal agent operation. That is
  less than a typical Electron app's idle footprint.
- Startup time is under 6 milliseconds. Cold boot to "ready to accept commands"
  is nearly instant. This matters for CLI usage where you invoke ryvos and expect
  an immediate response.
- No garbage collector means no GC pauses. The agent can run for weeks without
  memory-related slowdowns.
- Rust's type system catches entire categories of bugs at compile time. Refactors
  across 28k+ lines of code are surprisingly safe.

**What is harder:**

- The learning curve is real. Rust's borrow checker, lifetimes, and async model
  take time to internalize. This makes it harder to attract open source
  contributors compared to a TypeScript project.
- Compile times are noticeable. A clean build takes about 90 seconds. Incremental
  builds are faster, but still slower than a hot-reload JS workflow.
- The AI/ML ecosystem in Rust is immature compared to Python. We cannot easily
  call into libraries like transformers or langchain. For now, that is fine
  because we delegate all inference to external LLM APIs. If we ever need local
  model inference, we will need to evaluate options like candle or ort.
- Error handling with Result types is verbose. It is safer than exceptions, but
  new contributors sometimes find the ? operator and error type conversions
  confusing.

Despite the tradeoffs, Rust's performance characteristics align perfectly with
our deployment model. An agent that runs on a $35 board without breaking a sweat
is a meaningful differentiator.
