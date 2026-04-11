# Adding a tool

## When to use this guide

Three extension points let Ryvos call into new functionality: **built-in
tools**, **[skills](../glossary.md#skill)**, and **[MCP](../glossary.md#mcp)**
servers. This guide covers the first. Choose a built-in tool when the logic
is tightly coupled to Ryvos types (session id, `ToolContext`, the event bus),
when the code needs to live inside the daemon process for latency reasons,
or when you want to ship the tool with every Ryvos binary so operators do
not have to install anything extra. Choose a skill when the logic is a
script in another language that reads JSON on stdin and writes to stdout;
see [adding-a-skill.md](adding-a-skill.md). Choose an MCP server when the
integration already exists as an MCP server, when the implementation lives
in a separate process for isolation, or when you want the same tool
available to any MCP-aware client; see
[wiring-an-mcp-server.md](wiring-an-mcp-server.md).

Built-in tools live in [`ryvos-tools`](../crates/ryvos-tools.md). This guide
walks through the nine-step workflow for adding one: picking a home, writing
the `Tool` impl, choosing a timeout and tier, registering the tool with
`ToolRegistry::with_builtins`, wiring tests, and verifying the result from
the REPL.

## The `Tool` trait shape

The trait is defined in `crates/ryvos-core/src/traits.rs`. A minimal
implementation needs four methods; three more have sensible defaults.

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult>;

    fn timeout_secs(&self) -> u64 { 30 }
    fn requires_sandbox(&self) -> bool { false }
    fn tier(&self) -> SecurityTier { SecurityTier::T1 }
}
```

`name` is what the LLM sees in the tool list. `description` is a single
sentence the model uses to decide when to call the tool; be specific and
mention the inputs. `input_schema` returns a JSON-Schema-shaped
`serde_json::Value` that the LLM treats as the function-call signature.
`execute` receives an owned JSON value (already validated against the
schema at the provider level, but never trust it — validate again) plus a
**[tool context](../glossary.md#tool-registry)** carrying the session id,
working directory, and optional handles to the session store, agent
spawner, sandbox config, and Viking client.

## Step-by-step workflow

1. **Pick a category.** The 70+ built-ins are grouped into twelve files
   under `crates/ryvos-tools/src/builtin/` (bash, filesystem, git, code,
   data, database, network, system, browser, memory, scheduling,
   sessions). Add the new tool to the file whose category fits. If the
   category is genuinely new, create
   `crates/ryvos-tools/src/builtin/your_tool.rs` and add a `pub mod
   your_tool;` line to `crates/ryvos-tools/src/builtin/mod.rs`.

2. **Sketch the struct.** Most built-ins are zero-sized or carry a single
   config field. Keep state out of the tool: the runtime clones the tool
   list across concurrent batches, and any mutable state needs its own
   synchronization.

   ```rust
   pub struct MyTool;

   impl MyTool {
       pub fn new() -> Self { Self }
   }
   ```

3. **Write the input schema.** Use `serde_json::json!` to build a
   JSON-Schema object. Every field the tool reads from `input` must appear
   here with a type, and required fields must go in a `required` array.
   Describe each field — the description is what the LLM reads when it
   decides what to pass.

   ```rust
   fn input_schema(&self) -> serde_json::Value {
       serde_json::json!({
           "type": "object",
           "properties": {
               "path": { "type": "string", "description": "Absolute path to the target file" },
               "limit": { "type": "integer", "description": "Max bytes to read", "default": 4096 }
           },
           "required": ["path"]
       })
   }
   ```

4. **Implement `execute`.** Parse the input with `serde_json::from_value`
   into a typed struct, resolve any filesystem paths against
   `ctx.working_dir`, do the work, and return `ToolResult::success(text)`
   or `ToolResult::error(message)`. Never panic on bad input; convert
   parse failures into `ToolResult::error`.

   ```rust
   #[async_trait::async_trait]
   impl Tool for MyTool {
       fn name(&self) -> &str { "my_tool" }
       fn description(&self) -> &str { "One-sentence summary of the tool" }
       fn input_schema(&self) -> serde_json::Value { /* as above */ }

       async fn execute(&self, input: serde_json::Value, ctx: ToolContext)
           -> Result<ToolResult>
       {
           let args: MyArgs = serde_json::from_value(input)
               .map_err(|e| ToolResult::error(format!("bad input: {e}")))?;
           let path = ctx.working_dir.join(&args.path);
           // do the work
           Ok(ToolResult::success(format!("done: {}", path.display())))
       }
   }
   ```

5. **Pick a timeout.** Override `timeout_secs` if 30 seconds is wrong. The
   registry wraps every call in `tokio::time::timeout(timeout_secs, ...)`;
   exceeding the cap aborts the in-flight call with `RyvosError::ToolTimeout`.
   Shell tools use 120, the sub-agent spawner uses 300, HTTP fetches use
   60. Be generous — a tool that times out at its declared cap is a bug
   report.

6. **Pick a tier.** `tier()` returns a
   **[T0–T4](../glossary.md#t0t4)** label. The gate does not block based
   on tier under [passthrough security](../glossary.md#passthrough-security),
   but the audit trail and operator tooling group tools by the reported
   value. Use `T0` for read-only, `T1` for write-local, `T2` for mutating,
   `T3` for dangerous (anything a mistake could make unrecoverable), and
   `T4` for system-level. When in doubt, pick the tier that best matches
   a similar existing tool.

7. **Set the sandbox flag.** If the tool must run inside a container when
   sandboxing is enabled, override `requires_sandbox()` to return `true`.
   Only `bash` currently uses this flag; adding a new sandboxed tool
   requires a matching branch in the sandbox execution path.

8. **Register the tool.** Add one line to
   `ToolRegistry::with_builtins` in `crates/ryvos-tools/src/registry.rs`:

   ```rust
   registry.register(Arc::new(MyTool::new()));
   ```

   Place the call in the category block that matches the module. The
   factory is the canonical catalog — if a tool is not listed here and
   is not registered at runtime by a skill or MCP server, the agent
   cannot see it.

9. **Write a unit test.** Add a `#[cfg(test)]` block at the bottom of the
   module. Use `ryvos_test_utils::test_tool_context()` to get a stock
   `ToolContext` with a `/tmp/ryvos-test` working directory. For tools
   that write files, pair the context with a `tempfile::TempDir` via
   `test_tool_context_with_dir(dir)`.

   ```rust
   #[tokio::test]
   async fn my_tool_happy_path() {
       let tool = MyTool::new();
       let input = serde_json::json!({ "path": "example.txt" });
       let result = tool.execute(input, test_tool_context()).await.unwrap();
       assert!(!result.is_error);
       assert!(result.content.contains("done"));
   }
   ```

## Testing with `MockTool` and fixtures

If the new tool is consumed by higher-level code — the agent loop, the
Director, a custom orchestrator — script the surrounding behavior with
`MockLlmClient` and `MockTool` from
[`ryvos-test-utils`](../crates/ryvos-test-utils.md). The usual pattern is to
build a `test_config()`, wrap an `InMemorySessionStore` in an `Arc`, chain
`MockLlmClient::with_tool_call("my_tool", json)` to script the LLM into
calling the new tool, and then assert on `MockTool::invocation_count()`.
See [ryvos-test-utils.md](../crates/ryvos-test-utils.md) for the full
harness pattern.

## Verification

1. Run the crate's unit tests: `cargo test --package ryvos-tools`. The
   `with_builtins` test at the bottom of `registry.rs` asserts that the
   factory produces a non-empty registry — adding a broken tool will fail
   it.
2. Build the full binary: `cargo build --release`. A schema that fails to
   parse at startup is a `RyvosError::Config` and aborts `AgentRuntime::new`.
3. Smoke-test from the REPL: `ryvos run "use my_tool with path=example.txt"`.
   The run log under `~/.ryvos/logs/{session}/` records the invocation; the
   audit trail in `audit.db` records the tool name, input summary, and
   outcome.
4. Inspect the audit entry: `ryvos audit query --tool my_tool`. The entry
   shows the reasoning string the gate produced, the resolved safety
   outcome, and any **[SafetyMemory](../glossary.md#safetymemory)** lessons
   that were available at dispatch time.

Once the tool ships, the [tool registry internals](../internals/tool-registry.md)
document covers how the agent loop dispatches it alongside built-ins,
skills, and MCP bridged tools. If the tool is sophisticated enough to
warrant its own skill package, read [adding-a-skill.md](adding-a-skill.md)
next. If the tool should be exposed to other MCP clients as well, the
server-side path is in [../api/mcp-server.md](../api/mcp-server.md). For
provider-specific extensions (a new LLM backend that the tool targets),
follow [adding-an-llm-provider.md](adding-an-llm-provider.md).
