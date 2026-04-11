# Adding an LLM provider

## When to use this guide

Ryvos ships with eight native LLM provider implementations (Anthropic,
OpenAI, Azure, Bedrock stub, Gemini, Cohere, `claude-code`, `copilot`)
and ten OpenAI-compatible presets (Ollama, Groq, OpenRouter, Together,
Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek). The decision
tree for adding an eleventh is short:

- **If the vendor speaks the OpenAI chat completions format,** add a
  preset in `crates/ryvos-llm/src/providers/presets.rs`. Four lines of
  code: a base URL, an optional `needs_key` flag, an `extra_headers`
  map, and an entry in `all_preset_names()`. The wildcard arm in the
  factory already hands presets to `OpenAiClient`, which speaks every
  OpenAI-compatible dialect that the rest of the industry has converged
  on. No new file is needed.

- **If the vendor has its own wire format** — Anthropic's typed events,
  Gemini's `contents`/`parts` shape, Cohere's deeply nested deltas, a
  subprocess-based CLI — add a native implementation. That means a new
  file under `crates/ryvos-llm/src/providers/`, an `LlmClient` impl, and
  a factory arm in `lib.rs`.

This guide covers the native path. For the preset path, jump to the
"Preset alternative" section at the end.

The crate-level reference is [`ryvos-llm`](../crates/ryvos-llm.md). The
streaming contract is described there in depth; this guide focuses on
the workflow a contributor follows to add a provider from a blank file
to a passing test.

## The `LlmClient` trait

The trait is defined in `crates/ryvos-core/src/traits.rs` and has exactly
one method:

```rust
fn chat_stream(
    &self,
    config: &ModelConfig,
    messages: Vec<ChatMessage>,
    tools: &[ToolDefinition],
) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>>;
```

The double-`Result` is intentional. The outer `Result` reports a failure
to even start the stream (HTTP connect error, missing API key, CLI spawn
failure). The inner `Result` reports a failure mid-stream (SSE parse
error, broken connection, structured error event from the vendor). The
agent loop distinguishes the two for retry classification.

Every provider in `ryvos-llm` implements this one method and nothing
else. Everything a provider does internally — message conversion, tool
schema translation, authentication, SSE parsing, stop-reason mapping —
is private to its file.

## Step-by-step workflow

1. **Create the provider file.** Add
   `crates/ryvos-llm/src/providers/your_provider.rs` and a
   `pub mod your_provider;` line to
   `crates/ryvos-llm/src/providers/mod.rs`. The struct is usually a thin
   wrapper over a shared `reqwest::Client`:

   ```rust
   pub struct YourClient {
       http: reqwest::Client,
   }

   impl YourClient {
       pub fn new() -> Self {
           Self { http: reqwest::Client::new() }
       }
   }
   ```

2. **Write `convert_messages`.** Walk the `Vec<ChatMessage>` and emit
   the vendor's native message array. Handle three cases the rest of
   the system cannot do for you:

   - **System prompt placement.** Some vendors (Anthropic, Gemini) want
     the system prompt as a top-level field; others (OpenAI) want it as
     a message with `role: "system"`. Extract the first system message
     from the slice and route it accordingly.
   - **Tool results.** A `ChatMessage` with role `Tool` carries a
     `ContentBlock::ToolResult`. OpenAI expects it as a standalone
     message with `role: "tool"` and a `tool_call_id`; Anthropic expects
     it as a `user` message with a `tool_result` content block; Gemini
     expects a `functionResponse` part. Get this wrong and the model
     will refuse to continue the conversation.
   - **Assistant tool calls.** Assistant messages with `ContentBlock::ToolUse`
     blocks need to match whatever shape the vendor's tool-use response
     format takes.

3. **Write `convert_tools`.** Map `ToolDefinition` into the vendor's
   function-calling schema. Most providers use a variant of JSON Schema
   wrapped in a `function` object; the shape differs in whether
   `parameters` is flat or nested, whether `description` is optional,
   and whether there's an enclosing `type: "function"` discriminator.

4. **Build the request.** Serialize a dedicated request struct with
   `serde_json` and set `stream: true` if the vendor uses SSE. For
   CLI providers (subprocess-based), build the argv array instead of a
   JSON body — see the Claude Code and Copilot notes in
   [`ryvos-llm`](../crates/ryvos-llm.md).

5. **Send the request.** Apply the `x-api-key`, `Authorization: Bearer`,
   `api-key`, or other auth header the vendor wants. Merge any
   `extra_headers` from `ModelConfig` so operators can add custom
   headers from the TOML config. `reqwest::Client::post(base_url)` is
   the idiomatic call for HTTP providers.

6. **Parse the stream.** Wrap the response's `bytes_stream()` in
   `crates/ryvos-llm/src/streaming.rs`'s `SseStream` for SSE-based
   providers; for JSONL-based CLI providers, use
   `tokio_stream::wrappers::LinesStream` on the child's stdout. Each
   event passes through a `parse_*_to_delta` function that returns one
   or more `StreamDelta` variants.

7. **Emit the right `StreamDelta` variants.** The rest of Ryvos sees
   only these variants; mapping them correctly is the whole point of
   the provider. The common cases:

   | Provider event | `StreamDelta` variant |
   |---|---|
   | Streamed assistant text chunk | `TextDelta(String)` |
   | Streamed reasoning/thinking chunk | `ThinkingDelta(String)` |
   | Start of a tool call (id + name) | `ToolUseStart { id, name }` |
   | Tool-call argument fragment | `ToolInputDelta(String)` |
   | Per-turn token counts | `Usage { input, output }` |
   | Provider request id | `MessageId(String)` |
   | End of turn (no tool calls) | `Stop(StopReason::EndTurn)` |
   | End of turn (tool calls pending) | `Stop(StopReason::ToolUse)` |
   | Context window exhausted | `Stop(StopReason::MaxTokens)` |

   A single wire chunk can produce multiple deltas. OpenAI-compatible
   providers frequently send both a tool name and its first argument
   fragment in the same SSE event; the parser returns a `Vec<Result<...>>`
   and the outer `chat_stream` flattens it. Model it the same way if
   the vendor has a similar quirk.

8. **Register the client in the factory.** Add an arm to
   `create_client` in `crates/ryvos-llm/src/lib.rs`:

   ```rust
   match config.provider.as_str() {
       "anthropic" | "claude" => Box::new(AnthropicClient::new()),
       // ... existing arms ...
       "yourvendor" | "yv" => Box::new(YourClient::new()),
       _ => Box::new(OpenAiClient::new()),
   }
   ```

   Use two or three aliases if the vendor's short name and full name
   both feel natural — users will type one or the other in `ryvos.toml`.

## Handling provider quirks

Three quirks come up often enough to name:

- **Rate limits.** A 429 response should bubble up as
  `RyvosError::LlmRequest` with the message containing `429`. The
  retry classifier in `RetryingClient` checks for `429`, `500`, `502`,
  `503`, `timeout`, and `connection` substrings and treats all of them
  as retryable with exponential backoff and jitter. Stream errors are
  always retryable because stream failures are almost always transient.
  Return a clean error message and the wrapper handles the rest.

- **Reasoning tokens.** Models with separate thinking budgets (OpenAI's
  `o1`/`o3`/`o4`, Anthropic with `thinking.enabled`, DeepSeek with
  `reasoning_content`, Qwen with `reasoning`) emit reasoning deltas in
  vendor-specific fields. Map them all to `StreamDelta::ThinkingDelta`.
  When thinking is enabled, some vendors disallow `temperature` in the
  request — handle that conditionally at request-build time.

- **Tool-call format.** Anthropic emits `tool_use` content blocks
  nested in a single assistant message; OpenAI emits a `tool_calls`
  array alongside a possibly-null `content`; Gemini emits
  `functionCall` parts; Cohere has `delta.message.tool_calls.function.arguments`
  four levels deep. Every provider file has a `parse_chunk` function
  that handles exactly this shape — model the new one on the closest
  existing provider.

## Preset alternative

If the vendor is OpenAI-compatible, skip the whole file and add a
preset entry instead:

```rust
// in crates/ryvos-llm/src/providers/presets.rs
"yourvendor" => Some(PresetDefaults {
    base_url: "https://api.yourvendor.com/v1/chat/completions",
    needs_api_key: true,
    extra_headers: vec![],
}),
```

Then add `"yourvendor"` to `all_preset_names()` in the same file. The
factory's wildcard arm sends the request through `OpenAiClient`, which
handles every OpenAI-shaped variant already: o-series reasoning,
tool-call splitting, `reasoning_content` deltas, and `[DONE]`
termination. Adding a preset is a four-line change; adding a native
provider is a four-hundred-line change. Choose the preset path whenever
it works.

## Testing with `MockLlmClient`

For tests of the surrounding code (agent loop, cost tracking, channel
dispatch), use `MockLlmClient` from
[`ryvos-test-utils`](../crates/ryvos-test-utils.md). It records every
inbound call and lets you script turn-by-turn responses with
`with_text_response("...")` and `with_tool_call("tool", json)`. Tests
for the new provider itself should use `wiremock` or a recorded HTTP
fixture to drive the real client against a fake endpoint.

## Verification

1. Run the crate's unit tests: `cargo test --package ryvos-llm`. The
   `streaming.rs` tests at the bottom of the file cover the SSE parser
   edge cases; a new parser should add its own tests next to the
   existing Anthropic, OpenAI, and Gemini cases.
2. Point a minimal `ryvos.toml` at the new provider:

   ```toml
   [model]
   provider = "yourvendor"
   model_id = "latest-turbo"
   api_key = "${YOURVENDOR_API_KEY}"
   ```

3. Run `ryvos run "hello"`. Watch the run log under
   `~/.ryvos/logs/{session}/` — the JSONL stream should show `TextDelta`
   events arriving in order, a final `Stop(EndTurn)`, and a `Usage`
   delta that the cost tracker turns into a row in `cost.db`.
4. Call a tool: `ryvos run "use bash to print hello world"`. The LLM
   should emit a `ToolUseStart`, the accumulator should assemble the
   JSON input, and the agent loop should dispatch `bash` through the
   registry. A provider that cannot call tools is broken; there is no
   fallback path.
5. Optionally point a fallback chain at the new provider and break the
   primary to verify that `RetryingClient` routes through:

   ```toml
   [[fallback_models]]
   provider = "yourvendor"
   model_id = "latest-turbo"
   ```

For the broader streaming contract, read
[../crates/ryvos-llm.md](../crates/ryvos-llm.md). For the agent loop's
consumption of `StreamDelta` values turn by turn, read
[../internals/agent-loop.md](../internals/agent-loop.md). For a future
dedicated wire-format reference, see
[../architecture/streaming-protocol.md](../architecture/streaming-protocol.md).
For the CLI-provider subprocess pattern that `claude-code` and
`copilot` use, read ADR-004 linked from the crate reference.
