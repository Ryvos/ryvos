# ryvos-llm

`ryvos-llm` is the LLM client layer. It sits in the platform tier of the
workspace, directly above `ryvos-core`, and provides exactly one runtime
abstraction: a boxed `LlmClient` that the agent loop calls with a vector of
`ChatMessage` values and a list of `ToolDefinition` schemas, and that returns
a boxed stream of `StreamDelta` events. Every higher-layer crate —
`ryvos-agent`, `ryvos-channels`, `ryvos-gateway`, `ryvos-tui` — talks to LLMs
exclusively through that stream. The agent loop never sees Anthropic's
`content_block_delta`, OpenAI's `tool_calls` array, Gemini's `functionCall`
part, or the `stream-json` lines that the `claude` CLI prints to stdout. All
of those wire formats are normalized inside this crate.

The crate ships eight dedicated provider implementations — Anthropic, OpenAI,
Azure OpenAI, AWS Bedrock (stub), Google Gemini, Cohere v2, Claude Code CLI,
and GitHub Copilot CLI — plus ten **[OpenAI-compatible](../glossary.md#api-billing)**
presets that reuse the OpenAI client with different base URLs and headers.
Two of the providers are **[CLI providers](../glossary.md#cli-provider)**:
they spawn a local binary as a child process rather than hitting an HTTP
endpoint. The rationale for that pattern is recorded in
[ADR-004](../adr/004-cli-provider-pattern.md). The streaming wire format
itself is described in [internals/cost-tracking.md](../internals/cost-tracking.md)
for cost accumulation and in [internals/agent-loop.md](../internals/agent-loop.md)
for how the loop consumes deltas. Error handling — retry classification,
fallback chains, and user-facing surface — is discussed under
[operations/troubleshooting.md](../operations/troubleshooting.md).

## Purpose and position

The crate's job is narrower than it looks: translate between Ryvos's
provider-agnostic message types and whatever each vendor expects on the wire,
then reverse the translation on the streamed response. It does not buffer
full responses, it does not own any conversation history, and it does not
decide whether a tool call is safe to execute. Those responsibilities belong
to `ryvos-agent`. This separation of concerns is what lets Ryvos support
eighteen-plus providers without duplicating logic: the agent loop,
Guardian, Judge, and cost tracker all see the same `StreamDelta` values no
matter which wire format produced them, and every provider implementation
is reduced to a thin translation layer.

The crate is a runtime dependency of `ryvos-agent`, and through that, of
every integration-layer crate that ultimately runs the loop. `ryvos-llm`
depends only on `ryvos-core` from the workspace; the factory functions and
the provider structs are the only types exported. The factory itself is a
pure match on a provider string:

See `crates/ryvos-llm/src/lib.rs:48`:

```rust
pub fn create_client(config: &ModelConfig) -> Box<dyn LlmClient> {
    match config.provider.as_str() {
        "anthropic" | "claude" => Box::new(AnthropicClient::new()),
        "gemini" | "google" => Box::new(GeminiClient::new()),
        "azure" | "azure-openai" => Box::new(AzureClient::new()),
        "bedrock" | "aws-bedrock" | "aws" => Box::new(BedrockClient::new()),
        "cohere" => Box::new(CohereClient::new()),
        "claude-code" | "claude-cli" | "claude-sub" => Box::new(ClaudeCodeClient::new()),
        "copilot" | "github-copilot" | "copilot-cli" => Box::new(CopilotClient::new()),
        _ => Box::new(OpenAiClient::new()),
    }
}
```

The wildcard arm is load-bearing. Every OpenAI-compatible provider — Ollama,
Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity,
DeepSeek, and any user-defined endpoint that speaks the OpenAI chat
completions format — falls through to the same `OpenAiClient` struct. What
distinguishes them is not the client type but the `base_url` and
`extra_headers` fields on `ModelConfig`, which `apply_preset_defaults` fills
in before the factory runs. The result is that adding an eleventh preset is
a four-line change to `providers/presets.rs`; adding a new native provider is
a new file and a new factory arm.

A second factory, `create_client_with_security`, exists for the two CLI
providers. It attaches a compiled `DangerousPatternMatcher` that inspects
tool invocations emitted by the child process, logs matches, but does not
block them. This mirrors the rest of Ryvos's **[passthrough
security](../glossary.md#passthrough-security)** stance: observation yes,
gating no.

## The HTTP provider pattern

Every non-CLI provider follows the same five-step pattern, and recognising
the pattern makes the provider-specific files much shorter to read:

1. **Convert messages.** A `convert_messages` function walks
   `Vec<ChatMessage>` and produces the vendor-specific message array, pulling
   system messages out into their own field if the vendor requires it.
2. **Convert tools.** A `convert_tools` (or inline equivalent) function maps
   `ToolDefinition` into the vendor's function-calling schema.
3. **Build the request.** A dedicated request struct is serialized to JSON
   with `stream: true` set.
4. **POST.** The crate uses a shared `reqwest::Client` per provider struct,
   with auth headers and any `extra_headers` from config applied.
5. **Parse SSE.** The response's `bytes_stream()` is wrapped in an `SseStream`,
   each event is passed to a `parse_*_to_delta` function, and the result is a
   `BoxStream<Result<StreamDelta>>` that the agent loop polls.

Step five is where the uniformity ends. Each provider has its own
`parse_chunk` function because every vendor has chosen a different shape for
streamed deltas. The common denominator — what the rest of the system sees —
is the `StreamDelta` enum from `ryvos-core`, which carries text deltas,
thinking deltas, tool-use starts, tool-input deltas, usage counts, message
IDs, CLI tool events, and stop reasons. Provider code's sole job is to emit
those variants in the correct order.

## Provider-by-provider quirks

The dedicated providers diverge in how they represent system messages, tool
results, thinking tokens, and authentication. The quirks below are the ones
that matter when debugging a streaming session or writing a new provider.

### Anthropic

Anthropic's Messages API treats the system prompt as a top-level `system`
field rather than a message with `role: "system"`. `convert_messages` in
`crates/ryvos-llm/src/providers/anthropic.rs:144` extracts the first system
message and returns it separately from the `user`/`assistant` array. Tool
results are sent as `user` messages containing `tool_result` content blocks.
Authentication uses an `x-api-key` header plus a pinned `anthropic-version`
(`2023-06-01`).

Extended thinking has one subtle constraint: when `thinking` is enabled in
config, the request must omit `temperature` entirely. The provider honours
this at `crates/ryvos-llm/src/providers/anthropic.rs:335` by passing `None`
for temperature whenever `thinking.is_some()`. The thinking budget itself
maps from the `ThinkingLevel` enum in `ryvos-core` to Anthropic's
`budget_tokens` integer via `ThinkingConfig`.

The streaming response uses typed events (`message_start`,
`content_block_start`, `content_block_delta`, `message_delta`,
`message_stop`, `ping`, `error`). The parser turns `text_delta` into
`StreamDelta::TextDelta`, `thinking_delta` into `StreamDelta::ThinkingDelta`,
`input_json_delta` into `StreamDelta::ToolInputDelta`, and `stop_reason`
strings into the typed `StopReason` enum.

### OpenAI

OpenAI's chat completions API is simpler on paper but carries more edge
cases in practice. `convert_messages` in
`crates/ryvos-llm/src/providers/openai.rs:144` emits tool results as
standalone `role: "tool"` messages with a `tool_call_id`, not as content
blocks inside a user message. Assistant messages with tool calls use the
`tool_calls` array alongside a possibly-null `content`.

The client detects OpenAI's o-series reasoning models by a simple prefix
check (`o1`, `o3`, `o4`) at
`crates/ryvos-llm/src/providers/openai.rs:345`. For those models it
suppresses `temperature` entirely and sends `reasoning_effort` instead,
mapping the `ThinkingLevel` enum to one of `low`, `medium`, `high`.

The most consequential quirk is in `parse_chunk` at
`crates/ryvos-llm/src/providers/openai.rs:236`. Unlike the Anthropic parser,
which returns `Option<Result<StreamDelta>>`, the OpenAI parser returns
`Vec<Result<StreamDelta>>`. Many OpenAI-compatible providers — notably Groq,
Together, and Fireworks — send a tool call's name and initial arguments in
the same SSE chunk. A single wire chunk therefore has to produce both
`ToolUseStart` and `ToolInputDelta` in the returned vector. The outer
`chat_stream` flattens those vectors into a flat delta stream with
`futures::stream::iter` and `.flatten()`. DeepSeek and Qwen extensions for
reasoning (`reasoning` / `reasoning_content` fields on the delta) are also
handled here and emitted as `ThinkingDelta`.

### Azure OpenAI

The Azure provider at `crates/ryvos-llm/src/providers/azure.rs` is a thin
wrapper: it reuses `super::openai::convert_messages`,
`super::openai::convert_tools`, and `super::openai::parse_chunk` verbatim.
The only differences are URL construction and authentication. Azure's
endpoint includes the resource name, deployment name, and API version in the
path and query, so the client builds a URL of the form
`https://{resource}.openai.azure.com/openai/deployments/{deployment}/chat/completions?api-version={api_version}`.
Authentication uses an `api-key` header rather than `Authorization: Bearer`.
Because the conversion layer is shared, every behaviour described for OpenAI
— o-series handling, tool-call splitting, reasoning deltas — applies to
Azure automatically.

### AWS Bedrock (stub)

`BedrockClient` at `crates/ryvos-llm/src/providers/bedrock.rs` is a
placeholder. Bedrock requires AWS SigV4 request signing, which would pull in
`aws-sigv4` and `aws-credential-types`. Rather than ship a half-working
implementation, the client returns a clear error directing users to either
use the `anthropic` provider with a direct API key or route through an
OpenAI-compatible gateway. Full Bedrock support is tracked for a later
release. Operators who need Bedrock today run Anthropic directly.

### Google Gemini

Gemini's native API uses `contents` where others use `messages`, `model` as
the assistant role name, `parts` instead of content blocks, and a
`system_instruction` top-level field that is structurally identical to a
content block but lives outside `contents`. Tool calls are represented as a
`functionCall` part; tool results are sent back as a `functionResponse` part
keyed by the original function name. API key authentication is in the query
string (`?key=...`) rather than a header, which is unusual but consistent
with how Google's SDKs work.

The parser at `crates/ryvos-llm/src/providers/gemini.rs:227` walks
`candidates[0].content.parts`, emits the first text or function call it
finds as a `StreamDelta`, and maps Gemini's camelCase stop reasons (`STOP`,
`MAX_TOKENS`) to the shared `StopReason` enum. `usageMetadata` appears in a
separate chunk and is emitted as `StreamDelta::Usage`.

### Cohere

The Cohere v2 Chat API lives at `crates/ryvos-llm/src/providers/cohere.rs`
and is the most deeply nested wire format Ryvos supports. Tool results are a
standalone `tool` role carrying a `tool_results` array of `{call, outputs}`
pairs, and streamed deltas arrive inside
`delta.message.content.text` or `delta.message.tool_calls.function.arguments`.
Every level is optional, which forces the response types into long chains of
`Option<...>` fields. The client's parse function walks those chains
defensively and only emits a `StreamDelta` when every required level is
present, falling back to `None` on malformed events rather than erroring the
stream.

### Claude Code CLI

`ClaudeCodeClient` in `crates/ryvos-llm/src/providers/claude_code.rs` is the
first of the two CLI providers. Instead of a `reqwest::Client`, it holds an
optional `DangerousPatternMatcher` and spawns the `claude` binary as a
child process whenever `chat_stream` is called. The argument construction
around `crates/ryvos-llm/src/providers/claude_code.rs:104` assembles:

- `--print --output-format stream-json --verbose` for machine-readable output.
- `--permission-mode bypassPermissions` by default (overridable), so the CLI
  runs headless without prompting.
- `--disallowedTools` for a hardcoded list of genuinely destructive bash
  patterns (`rm -rf:*`, `rm -r:*`, `mkfs:*`, `dd if=:*`). This is the only
  place in the crate where a tool is actively denied, and it is a last-resort
  safeguard against an obvious footgun rather than a security policy.
- `--model` if the caller supplied one other than `default`.
- `--resume` with the stored `cli_session_id` when the session is being
  continued (see `ryvos-memory`'s `SessionMetaStore`).
- `--system-prompt` concatenated from all system messages in the request.

The prompt is written to the child's stdin and stdin is closed, after which
the child's stdout is read line by line with `tokio_stream`'s `LinesStream`.
Each line is parsed as JSON and routed through `parse_stream_json`, which
maps:

- `{ "type": "system", "subtype": "init" }` to a
  `StreamDelta::MessageId(session_id)` so the caller can persist the
  resumable session ID.
- `{ "type": "assistant" }` with a `tool_use` content block to a
  `StreamDelta::CliToolExecuted { tool_name, input_summary }` event. Ryvos
  cannot block the tool — it has already executed inside the CLI by the
  time this event fires — but it logs the invocation to the
  **[audit trail](../glossary.md#audit-trail)** and runs it through the
  dangerous-pattern matcher so the lesson can feed **[SafetyMemory](../glossary.md#safetymemory)**.
- `{ "type": "assistant" }` with a `tool_result` block to
  `StreamDelta::CliToolResult { tool_name, output_summary, is_error }` for
  post-hoc safety evaluation.
- `{ "type": "result" }` to the final `TextDelta` plus
  `StreamDelta::Stop(EndTurn)`.

The client classifies billing by the presence of an API key. If `api_key` is
set, the run is `BillingType::Api`; otherwise the CLI is using the user's
Claude Max or Pro subscription and the run is
**[subscription-billed](../glossary.md#subscription-billing)**.

### GitHub Copilot CLI

`CopilotClient` in `crates/ryvos-llm/src/providers/copilot.rs` follows the
same subprocess pattern but with different CLI conventions. Copilot is
always `BillingType::Subscription` regardless of configuration — the
billing relationship is strictly between the user and GitHub. The argument
builder uses `--prompt`, `--output-format json`, `--silent`, `--no-color`,
`--no-ask-user` and either `--allow-all` or per-tool `--allow-tool` flags
based on `cli_permission_mode`.

Copilot's CLI has no `--system-prompt` flag, so the client prepends the
system context to the user prompt with an `\n\n---\n\n` separator. Session
resumption uses the format `--resume={session_id}` (with an equals sign,
unlike Claude's space-separated `--resume <id>`). Output is JSONL with event
types like `assistant.message_delta`, `assistant.reasoning_delta`,
`assistant.message`, and `result`; the parser also strips ANSI escape
sequences (CSI and OSC) from text content because Copilot may emit coloured
output even with `--no-color`.

The same audit-and-log pattern as Claude Code applies: tool requests are
inspected against dangerous patterns, warnings are logged, but execution is
never blocked. The `result` event carries a `sessionId` that is emitted as a
`MessageId` delta so the session meta store can save it for later
`--resume={sid}` calls.

## OpenAI-compatible presets

The ten presets live in `crates/ryvos-llm/src/providers/presets.rs` and
each provides three values: a default base URL, whether an API key is
needed, and a set of extra headers. The complete preset list:

| Preset | Default base URL | Needs key | Extra headers |
|---|---|---|---|
| `ollama` | `http://localhost:11434/v1/chat/completions` | no | — |
| `groq` | `https://api.groq.com/openai/v1/chat/completions` | yes | — |
| `openrouter` | `https://openrouter.ai/api/v1/chat/completions` | yes | `X-Title: Ryvos` |
| `together` | `https://api.together.xyz/v1/chat/completions` | yes | — |
| `fireworks` | `https://api.fireworks.ai/inference/v1/chat/completions` | yes | — |
| `cerebras` | `https://api.cerebras.ai/v1/chat/completions` | yes | — |
| `xai` | `https://api.x.ai/v1/chat/completions` | yes | — |
| `mistral` | `https://api.mistral.ai/v1/chat/completions` | yes | — |
| `perplexity` | `https://api.perplexity.ai/chat/completions` | yes | — |
| `deepseek` | `https://api.deepseek.com/v1/chat/completions` | yes | — |

`apply_preset_defaults` in `crates/ryvos-llm/src/lib.rs:84` fills `base_url`
and merges `extra_headers` into the user's `ModelConfig` at load time. User
overrides always take precedence, so setting `extra_headers = { "X-Title" =
"custom" }` in `ryvos.toml` replaces the preset's default. The factory then
dispatches the wildcard arm, returns an `OpenAiClient`, and the request goes
out to whatever URL the preset (or the user) resolved.

Because every preset is just data, adding a new one is a matter of dropping
another arm into `get_preset` and appending its name to `all_preset_names()`.
The guide [guides/adding-an-llm-provider.md](../guides/adding-an-llm-provider.md)
walks through the process.

## RetryingClient

`RetryingClient` in `crates/ryvos-llm/src/retry.rs` wraps any boxed
`LlmClient` with retry and fallback logic. It holds a primary client, a
`Vec<(ModelConfig, Box<dyn LlmClient>)>` of fallback providers, and a
`RetryConfig` with `max_retries`, `initial_backoff_ms`, and `max_backoff_ms`.

The retry decision is made by a small classifier, `is_retryable`, that
inspects the `RyvosError` variant. A request-level error is retryable if its
message contains any of `429`, `500`, `502`, `503`, `timeout`, or
`connection`. A stream-level error (`RyvosError::LlmStream`) is always
retryable because stream failures are almost always transient network or
encoding issues. Every other error — bad config, missing API key, explicit
4xx from the provider body — is terminal on the first attempt.

Backoff is exponential with jitter:
`initial_backoff_ms * 2^attempt`, clamped to `max_backoff_ms`, multiplied by
a random factor between 0.8 and 1.2. This prevents the thundering-herd
pattern where many concurrent retries line up on identical wall-clock
boundaries. After the primary exhausts its retry budget, the client walks
the fallback list in order, calling each `chat_stream` once with its own
`ModelConfig`. The first fallback that succeeds wins; all failures are
warned and ignored. If every fallback also fails, the last error from the
primary attempt is surfaced to the caller.

Fallbacks are a feature of `ryvos-core`'s config tree: users specify a
chain of `[[model.fallbacks]]` entries in `ryvos.toml` and the binary
builds the wrapped client at startup. The agent loop never knows whether
it is talking to a bare `AnthropicClient` or a `RetryingClient` wrapping an
Anthropic primary with an OpenRouter fallback — the trait signature is the
same.

## Streaming (SseParser and SseStream)

Every HTTP provider passes through the same SSE stack at
`crates/ryvos-llm/src/streaming.rs`. `SseParser` is a stateful buffer that
accumulates bytes, splits on the double-newline event boundary, and extracts
`event:` and `data:` fields into `SseEvent` values. It handles both `data: `
and `data:` (no space) forms, joins multiple data lines into a single
payload, and preserves the event type when present.

`SseStream<S>` adapts any `Stream<Item = Result<Bytes, reqwest::Error>>`
into a `Stream<Item = SseEvent>` by polling the inner byte stream,
feeding decoded UTF-8 chunks into the parser, and returning events as they
become complete. The one subtlety is that when the parser consumes bytes
without producing an event (partial chunk), the stream re-wakes itself with
`cx.waker().wake_by_ref()` rather than returning `Poll::Pending` without a
wake — the inner stream has no mechanism to re-wake the outer one if no new
bytes ever arrive. This is the kind of code that is hard to test
exhaustively; the unit tests at the bottom of `streaming.rs` cover the
chunked-event and multiple-event cases that motivated the design.

Provider parsers treat `[DONE]` in the `data` field as a stream terminator
and stop emitting deltas. The agent loop treats a completed
`BoxStream<Result<StreamDelta>>` — one that returns `None` from `poll_next`
— as the end of the turn, regardless of whether it saw an explicit
`Stop` delta first. Any SSE parse failure is logged with `tracing::warn!`
at the provider level and the event is dropped, on the grounds that a
malformed chunk should never abort an in-flight turn. Structured error
events (Anthropic's `{ "type": "error" }`, OpenAI error bodies) are the
exception: they are promoted to `Err(RyvosError::LlmStream(...))` and
bubble up to the agent loop, which terminates the turn and publishes a
`TaskFailed` event on the bus.

The `SseStream` adapter deliberately does not buffer an entire event
before yielding the previous one — events become available as soon as the
parser sees a double-newline boundary. Low-latency streaming matters for
the TUI and for the WebSocket broadcast channel, both of which render text
character by character as deltas arrive. Any provider that batches its
tokens server-side (for example, some Ollama builds with high
`num_batch`) degrades smoothness but never correctness, because the
agent loop does not require per-token granularity to function.

## Unified LlmClient trait

Every provider in this crate implements the same `LlmClient` trait from
`ryvos-core`. The trait has exactly one method:

```rust
fn chat_stream(
    &self,
    config: &ModelConfig,
    messages: Vec<ChatMessage>,
    tools: &[ToolDefinition],
) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>>;
```

The double `Result` is deliberate: the outer `Result` reports a failure to
even start the stream (HTTP connect error, missing API key, CLI spawn
failure), while the inner `Result` reports a failure mid-stream (SSE parse
error, child process crash, broken connection). The agent loop distinguishes
between these cases for retry policy and for event bus reporting.

Because the trait is object-safe, every higher-layer crate stores the LLM
client as `Box<dyn LlmClient>`. The factory functions `create_client` and
`create_client_with_security` are the only places in the workspace that
directly mention concrete provider types; everywhere else in
`ryvos-agent`, `ryvos-gateway`, and `ryvos-channels`, the client is an
opaque trait object. This is what makes retry wrapping, test-mock
substitution, and CLI patching uniform — the type system treats all 18+
providers as interchangeable.

The trait also returns `BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>>`
rather than plain `async fn`. The double-boxing is how the trait stays
object-safe in stable Rust: `async fn` in traits was stabilized only
recently, and replacing the manual boxing with language-level
`async fn in traits` is a future cleanup that will leave the rest of
Ryvos untouched. For now, the agent loop's call site is unambiguous:
construct a client once at startup, clone the `ModelConfig` and the
message vector into each invocation, await the outer future to get the
stream, then poll the stream until it ends.

## Where to go next

The retry chain and fallback machinery described here are consumed by the
agent loop; see [internals/agent-loop.md](../internals/agent-loop.md) for
the calling context and [internals/cost-tracking.md](../internals/cost-tracking.md)
for how `StreamDelta::Usage` events flow into `cost.db`. The CLI provider
pattern is justified at length in
[ADR-004](../adr/004-cli-provider-pattern.md). To add a new provider —
native or preset — follow
[guides/adding-an-llm-provider.md](../guides/adding-an-llm-provider.md).
