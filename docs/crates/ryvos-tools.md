# ryvos-tools

The tool crate. `ryvos-tools` owns the **[tool registry](../glossary.md#tool-registry)**
and every built-in tool the agent can call. It is a small crate with a very
wide surface: one `ToolRegistry`, one `with_builtins` factory, and roughly
seventy concrete `Tool` trait implementations organized into twelve
categories. The agent never constructs a tool directly — it always goes
through the registry, and the registry is the same object whether a tool was
built in, loaded from a skill, or proxied from an **[MCP](../glossary.md#mcp)**
server.

This crate has three responsibilities: provide the `ToolRegistry` that the
agent's `AgentRuntime` holds, ship a catalog of built-in tools that covers
the common operator tasks (shell, filesystem, git, HTTP, data transforms,
etc.), and enforce per-tool execution timeouts so a hung tool cannot wedge
the runtime. Everything else — security decisions, audit logging, approval
handling — happens upstream in the **[security gate](../glossary.md#security-gate)**
and downstream in the runtime. The crate's own invariants are narrow: every
tool runs through `ToolRegistry::execute` and every execution is wrapped in
a `tokio::time::timeout`.

## Position in the workspace

`ryvos-tools` depends on `ryvos-core` (for the `Tool` trait, `ToolContext`,
`ToolResult`, and the deprecated **[T0–T4](../glossary.md#t0t4)** metadata)
and `ryvos-memory` (for the handful of tools that read session history or
the local Viking store). It does not depend on `ryvos-agent`, `ryvos-llm`,
or any higher-layer crate — the `Tool` trait is plain enough that any
concrete tool only needs core types and whatever external libraries it
wraps. The agent, the MCP client, and the skills loader all register tools
into the same `ToolRegistry`.

## The `ToolRegistry`

The registry is defined in `crates/ryvos-tools/src/registry.rs`. It is a
plain `HashMap<String, Arc<dyn Tool>>` with five methods: `register`,
`unregister`, `get`, `list`, and `execute`. Registration is by the tool's
own `name()` (so two tools with the same name collide and the second one
wins). `execute` is the only method that does anything nontrivial:

```rust
pub async fn execute(
    &self,
    name: &str,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<ToolResult> {
    let tool = self
        .get(name)
        .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?;

    let timeout = std::time::Duration::from_secs(tool.timeout_secs());

    match tokio::time::timeout(timeout, tool.execute(input, ctx)).await {
        Ok(result) => result,
        Err(_) => Err(RyvosError::ToolTimeout {
            tool: name.to_string(),
            timeout_secs: tool.timeout_secs(),
        }),
    }
}
```

The timeout is the tool's declared `timeout_secs`, not a registry-wide
default. Each tool chooses its own cap: bash defaults to 120 seconds,
`spawn_agent` gets 300, `web_fetch` gets 60. When the cap is exceeded, the
tool is abandoned mid-execution and a `ToolTimeout` error propagates.

`ToolRegistry::with_builtins` is the factory that constructs a registry
pre-populated with every built-in. It is the single place in the codebase
where every built-in tool appears, which makes it the canonical catalog —
any tool not listed there is either disabled, registered at runtime from a
skill or MCP server, or registered separately by the registration helper
for browser tools. The factory also contains the explicit
`// Disabled: use MCP servers instead` comments for the Gmail, Notion, Jira,
and Linear source files that are compiled but not wired in; those modules
remain in the tree so that developers who want to revive them can, but the
recommended integration path is through the MCP client. See
[ryvos-mcp.md](ryvos-mcp.md) for the wiring.

## Built-in tools by category

The sections below walk the twelve categories in the same order the
`with_builtins` factory registers them. Every tool implements
`ryvos_core::traits::Tool` and exposes a `name()`, `description()`,
`input_schema()`, `execute()`, `timeout_secs()`, and a `tier()` returning
the informational **[T0–T4](../glossary.md#t0t4)** label. Tier values are
kept as metadata for the pre-v0.6 blocking model but no longer gate
execution; see [ADR-002](../adr/002-passthrough-security.md) for the
deprecation notes.

### Bash

A single tool, `bash`, in `crates/ryvos-tools/src/builtin/bash.rs`. The tool
takes a `command` string and an optional per-call `timeout` (default 120
seconds), runs the command through `bash -c` in the session's working
directory, captures stdout and stderr, truncates combined output to 30k
characters at the end, and returns the result as a `ToolResult::success` on
exit code 0 or a `ToolResult::error` with the exit code otherwise.

The bash tool is the only built-in with a Docker sandbox path. When
`ctx.sandbox_config` is `Some`, `sandbox.enabled == true`, and
`sandbox.mode == "docker"`, the tool instead calls
`execute_sandboxed`, which talks to the Docker daemon via `bollard`:
creates a container from the configured image (default
`ryvos/sandbox:latest`), binds the workspace to `/workspace` if
`mount_workspace == true`, caps memory at `memory_mb` MiB, disables
networking (`network_mode: "none"`), runs the command under
`bash -c`, streams logs back, and removes the container on exit. Output
truncation at 30k characters applies in both paths. Sandboxing is opt-in
via `[sandbox]` in `ryvos.toml`; the unsandboxed path remains the default.
The tool reports `requires_sandbox() == true` so callers can decide
whether to switch modes.

### File system

Eighteen tools across three files. The basic trio — `read`, `write`, and
`edit` — each live in their own module
(`crates/ryvos-tools/src/builtin/read.rs`,
`crates/ryvos-tools/src/builtin/write.rs`,
`crates/ryvos-tools/src/builtin/edit.rs`). `read` accepts a path and an
optional line range; `write` takes a path and content and creates or
overwrites the file; `edit` takes a path, an `old_string`, and a
`new_string` and does a unique-anchor replacement with a `replace_all`
flag. `apply_patch` (`crates/ryvos-tools/src/builtin/apply_patch.rs`)
applies unified-diff hunks using the `similar` crate as a fallback when
the context fuzz-match succeeds.

`glob` and `grep` live in `glob.rs` and `grep.rs` and provide pattern and
content search, respectively. Both honor the session working directory
and return text output ordered for scanning.

The nine tools in `filesystem.rs` cover the long tail: `file_info`
(metadata), `file_copy`, `file_move`, `file_delete`, `dir_list`,
`dir_create`, `file_watch` (inotify-style change notifications for a
bounded window), `archive_create` (tar or zip), and `archive_extract`.

### Git

Six tools in `crates/ryvos-tools/src/builtin/git.rs`: `git_status`,
`git_diff`, `git_log`, `git_commit`, `git_branch`, and `git_clone`. Each
shells out to the `git` binary through the same `tokio::process::Command`
pattern as the bash tool, so these are effectively thin wrappers with
structured schemas. The commit tool composes the full invocation from
its `message` and optional `files` argument and explicitly passes
`--no-verify` guards to the user rather than skipping hooks silently.

### Code and dev

Four tools in `crates/ryvos-tools/src/builtin/code.rs`: `code_format`
(formats a file via its language's canonical formatter — `rustfmt`,
`black`, `prettier`, and so on), `code_lint` (runs the language's
linter), `test_run` (discovers and runs the nearest test suite), and
`code_outline` (extracts top-level symbols for a quick map of a file).
Language detection comes from file extension plus `shebang` inspection.

### Data transforms

Eight tools in `crates/ryvos-tools/src/builtin/data.rs`: `json_query`
(jq-like expressions over JSON), `csv_parse` (row-by-row parsing with
header inference), `yaml_convert` (YAML to JSON and back), `toml_convert`
(same for TOML), `base64_codec` (encode and decode), `hash_compute`
(sha256, sha1, md5), `regex_replace`, and `text_diff` (unified-diff
output from two strings or two files via `similar`).

### Database

Two tools in `crates/ryvos-tools/src/builtin/database.rs`: `sqlite_query`
(opens a read-only connection, runs a query, returns rows as JSON) and
`sqlite_schema` (enumerates tables, columns, and indexes). The connection
path is taken from the input payload — these tools operate on arbitrary
databases, not on Ryvos's own stores.

### Network

Four tools in `crates/ryvos-tools/src/builtin/network.rs`: `http_request`
(full HTTP client with method, headers, body, and response truncation at
10k characters), `http_download` (streams a URL to a local path),
`dns_lookup` (A and AAAA records), and `network_check` (TCP-level
reachability probe).

`web_fetch` lives in its own module
(`crates/ryvos-tools/src/builtin/web_fetch.rs`). It is the recommended
tool for fetching web pages: it strips `<script>` and `<style>` blocks,
removes HTML tags with a regex, decodes a handful of common entities,
collapses whitespace, truncates to `max_length` (default 30k), and wraps
the result in an `<external_data source="…" trust="untrusted">` block
with a trailing instruction reminding the model that the content is
data, not commands. This tagging is the primary prompt-injection defense
the crate ships with; the safety constitution in
`crates/ryvos-agent/src/context.rs` references these tags and tells the
model to treat them as untrusted.

`web_search` is implemented in `crates/ryvos-tools/src/builtin/web_search.rs`
as a Tavily-backed search tool. It takes a `TAVILY_API_KEY` out of the
environment and is not registered by `with_builtins`; the gateway registers
it conditionally when a key is present. Adding it to `with_builtins`
unconditionally would force every run to carry a hard dependency on
Tavily, which is why it sits slightly outside the default catalog.

### System

Five tools in `crates/ryvos-tools/src/builtin/system.rs`: `process_list`
(ps-style enumeration), `process_kill` (by PID or name), `env_get`
(reads environment variables), `system_info` (kernel, arch, memory
counts), and `disk_usage` (du-style summary for a path).

### Browser

Five tools registered by the helper function
`crates/ryvos-tools/src/builtin/browser.rs:register_browser_tools`:
`browser_navigate`, `browser_screenshot`, `browser_click`, `browser_type`,
and `browser_extract`. They share a single Chromium session managed via
`chromiumoxide`, held in a `std::sync::OnceLock<Arc<BrowserSession>>` so
every tool in a run reuses the same page. A stale-check (`page.evaluate("1+1")`)
at the top of `ensure_page` recreates the browser if the previous page is
unresponsive. Chrome is discovered via `CHROME_PATH` or a platform-aware
candidate list (`/usr/bin/chromium`, `/usr/bin/google-chrome`,
`/Applications/Google Chrome.app/...`, etc.). Screenshots are base64
inline in the tool result, capped at 8k characters of descriptive text.
Browser tools report `T2` because any interaction with a live page can
mutate state on the remote side.

### Memory

Three built-in memory tools in `crates/ryvos-tools/src/builtin/memory.rs`
plus `memory_search.rs` and `memory_write.rs`. `memory_get` reads
`MEMORY.md` or a named file from the workspace memory directory;
`memory_write` appends a timestamped note to `MEMORY.md`;
`daily_log_write` appends to today's `memory/YYYY-MM-DD.md`;
`memory_delete` removes a memory file by name; and `memory_search` runs
FTS5 or (when embedding config is present) cosine-similarity search over
all past conversations via the history store in `ryvos-memory`.

### Scheduling

Three tools in `crates/ryvos-tools/src/builtin/scheduling.rs`:
`cron_list`, `cron_add`, and `cron_remove`. They read and edit the
`[[cron.jobs]]` blocks in `ryvos.toml` through the config parser in
`ryvos-core`. The scheduler itself is in `ryvos-agent`; these tools just
hand it new job definitions.

### Sessions

Five tools in `crates/ryvos-tools/src/builtin/sessions.rs`: `session_list`
(active sessions), `session_history` (messages for a given session),
`session_send` (route a message to a session by id), `session_spawn`
(create a new session), and `session_status` (idle, active, or waiting
for approval). These tools pull the `SessionStore` out of `ToolContext`,
so any caller that wants to use them must set `ctx.store`.

### Viking

Four tools in `crates/ryvos-tools/src/builtin/viking.rs`: `viking_search`,
`viking_read`, `viking_write`, and `viking_list`. Each tool fetches an
`Arc<VikingClient>` out of `ctx.viking_client` through an explicit
downcast — the Viking client is carried through `ToolContext` as an
`Arc<dyn Any + Send + Sync>` because `ryvos-core` does not know about
`ryvos-memory`'s concrete type, and the downcast to
`Arc<ryvos_memory::VikingClient>` happens at the tool boundary. If the
client is not configured, the tool returns a structured error asking the
user to enable `[openviking]` in the config. The `viking_search` tool
queries the local store or the standalone HTTP server at port 1933
depending on how the client was constructed; the full pattern is
described in [ADR-003](../adr/003-viking-hierarchical-memory.md).

### Miscellaneous

`spawn_agent` (`crates/ryvos-tools/src/builtin/spawn_agent.rs`) is the
tool the agent uses to delegate a task to a sub-agent. It takes a
`prompt` string, fetches `ctx.agent_spawner` out of the tool context,
and calls `spawner.spawn(prompt).await`. The spawner is usually the
`AgentRuntime` itself (which implements `AgentSpawner`) or a
`PrimeOrchestrator` (which spawns sub-agents under a tighter
`SecurityPolicy`). The tool caps itself at 300 seconds and reports `T3`
because a sub-agent inherits whatever tool set the parent hands it.

`notification_send` (`crates/ryvos-tools/src/builtin/notification.rs`)
emits a `NotificationRequested` event on the EventBus, which channel
adapters pick up and route to the appropriate platform. It is the
universal exit point for "tell the operator about this".

## Shared patterns

A few patterns recur across tools and are worth naming once.

**Working directory resolution.** Every filesystem-facing tool resolves
its path argument against `ctx.working_dir`. Absolute paths are used as
given; relative paths are joined to the session working directory. This
is how two sessions can have independent "current directories" without
the tools carrying per-call state.

**Prompt-injection tagging.** Tools that fetch content from untrusted
sources wrap the output in `<external_data source="…" trust="untrusted">`
blocks with a trailing instruction reminding the model to treat the
content as data. `web_fetch` is the canonical example; `http_request`
applies the same wrapping to HTML responses. The safety constitution in
`crates/ryvos-agent/src/context.rs` tells the model explicitly that
content inside these tags is data, not commands, and that it must
re-anchor to the user's goal after processing external input.

**Output truncation.** Every tool that produces variable-length output
enforces a cap: bash and web fetch at 30k characters, HTTP request at
10k, browser screenshots at 8k of descriptive text. The cap is
tool-local so that different tools can pick different budgets. The
intelligence module in `ryvos-agent` (see
[ryvos-agent.md](ryvos-agent.md)) also applies a token-based
`compact_tool_output` pass on the agent side before the result is
appended to the conversation, so very long outputs are trimmed a second
time at a coarser, token-based threshold.

**Security tier reporting.** Every `Tool` impl returns a `SecurityTier`
via `tier()`. The values are informational after v0.6 — the gate no
longer consults them to decide whether to dispatch. They remain useful
for the audit trail, the run log, and operator tooling that wants to
group tools by risk category. The pre-v0.6 blocking model is described
in [ADR-002](../adr/002-passthrough-security.md).

**Disabled integrations.** Four source files are compiled but not
registered by `with_builtins`: `google.rs`, `notion.rs`, `jira.rs`, and
`linear.rs`. Each one has a matching `// Disabled: use MCP servers
instead` comment in the registry factory. The reasoning, recorded in
[ADR-008](../adr/008-mcp-integration-layer.md), is that vendor
integrations live better as MCP servers than as hard-coded tools: they
can be swapped out without a Ryvos release, and any MCP-aware client
can use them. The source files are kept for operators who want to
resurrect them locally, but the recommended path is MCP.

## Extending the registry

Adding a new built-in tool is a matter of implementing the `Tool` trait
and registering it in `with_builtins`. The practical walkthrough —
argument schema, error handling, context plumbing, and tests — is in
[../guides/adding-a-tool.md](../guides/adding-a-tool.md). The execution
path (how the agent loop calls `ToolRegistry::execute`, how parallel
batches of tool calls are dispatched, and how results are fed back to
the model) is documented in
[../internals/tool-registry.md](../internals/tool-registry.md).

For the wire-level view of how external MCP tools appear in the same
registry, see [ryvos-mcp.md](ryvos-mcp.md). For skill-packaged tools,
see [ryvos-skills.md](ryvos-skills.md).
