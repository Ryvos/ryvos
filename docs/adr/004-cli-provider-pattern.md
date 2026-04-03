# ADR-004: CLI Provider for Claude Code and Copilot

## Status

Accepted

## Context

Ryvos needs to talk to large language models. The obvious approach is to call
the API directly: send HTTP requests to Anthropic's API or OpenAI's API, pay
per token, handle rate limits and retries yourself.

But there is an interesting alternative. Claude Code and GitHub Copilot both
have subscription billing. You pay a flat monthly fee and get a generous (or
unlimited) amount of usage. Their CLIs are installed locally and handle all the
authentication, billing, streaming, and tool execution internally.

The catch: their APIs are not publicly documented for programmatic use. There
is no official "spawn Claude Code as a subprocess" SDK. But the CLIs do accept
structured input, support tool passing via flags like `--allowedTools`, and
emit streaming JSON or JSONL output that can be parsed.

For users who already pay for Claude Code Pro or Copilot, this means Ryvos can
piggyback on their existing subscription. No separate API key needed, no per-token
charges, no billing surprises.

## Decision

We implemented a CLI provider pattern. Ryvos spawns the Claude Code (or Copilot)
binary as a child process using tokio's async process API. It passes the prompt
via stdin and reads streaming responses from stdout.

The implementation works like this:

1. **Discovery.** On startup, Ryvos checks if `claude` or `gh copilot` is
   available on the PATH. It records the available providers.
2. **Spawning.** When a task needs an LLM, Ryvos spawns the CLI with appropriate
   flags. For Claude Code, this includes `--print` for non-interactive mode,
   `--output-format json`, and `--allowedTools` to pass the available tool set.
3. **Streaming.** The CLI emits events as JSON lines. Ryvos parses each line,
   extracts assistant messages, tool calls, and tool results, and feeds them
   into the internal event bus.
4. **Session resume.** Claude Code supports `--resume` with a session ID. Ryvos
   tracks session IDs so it can resume conversations across agent restarts.
5. **Fallback.** If no CLI provider is available, Ryvos falls back to direct
   API calls using standard HTTP (reqwest + API key).

The provider interface is abstracted behind a trait, so adding new CLI providers
or switching between them is straightforward.

## Consequences

**What went well:**

- Users with Claude Code Pro get effectively unlimited agent usage at no
  additional cost. This is a huge value proposition compared to agents that
  burn through API credits.
- Session resume means the agent can pick up where it left off. Long-running
  goals do not lose their conversation context on restart.
- All of Claude Code's built-in tools (file editing, bash, web search) come
  for free. We do not need to reimplement them.
- The streaming JSON output is reasonably stable. We parse it with serde and
  handle unknown fields gracefully.

**What is harder:**

- This is fundamentally a black box integration. We cannot control what
  happens inside the CLI process. If Claude Code decides to use a tool in a
  way we did not expect, we can only observe it after the fact through the
  audit log.
- The JSON output format is not a stable API. It can change between CLI
  versions without notice. We have to maintain a parser that handles format
  drift.
- Error handling is tricky. If the CLI crashes, hangs, or emits malformed
  output, we need robust recovery. We use timeouts and process monitoring
  to detect and recover from these cases.
- Not everyone has Claude Code or Copilot. The direct API fallback ensures
  Ryvos still works, but the experience is different (per-token billing,
  no built-in tools).

The CLI provider pattern is unconventional, but it gives us access to
subscription billing and a rich tool ecosystem that would be extremely
expensive to replicate through direct API calls. The instability risk is
real, but manageable with defensive parsing and good fallback behavior.
