# ryvos-tui

`ryvos-tui` is the interactive terminal UI for Ryvos. It draws a full-screen
ratatui application on top of the daemon's agent runtime and
**[EventBus](../glossary.md#eventbus)**, streams assistant text as it
arrives, renders tool calls and their results inline, exposes the
**[approval broker](../glossary.md#approval-broker)** as a slash-command
surface, and displays a live status bar with session ID, token totals, and
an animated spinner while a run is in progress. The crate is a pure
EventBus subscriber: it never polls the runtime and never mutates any
state outside its own `App` struct.

The TUI is what `ryvos` launches when invoked without arguments as an
interactive session, and what `ryvos tui` launches explicitly. Both entry
points resolve to the same `run_tui` function exposed here.

## Position in the stack

`ryvos-tui` sits in the integration layer alongside `ryvos-gateway`,
`ryvos-channels`, and `ryvos-skills`. Its workspace dependencies are
`ryvos-core` (for `AgentEvent`, `SessionId`, `Verdict`, and the event bus
type) and `ryvos-agent` (for `AgentRuntime` and `ApprovalBroker`). External
dependencies are `ratatui` for the widget model, `crossterm` for raw-mode
terminal I/O, `tui-banner` 0.1.4 for the ASCII art banner, and `tokio` plus
`futures` for the async plumbing. See `crates/ryvos-tui/Cargo.toml`.

## Entry point

`run_tui` in `crates/ryvos-tui/src/lib.rs:26` is the crate's single public
function. It takes the shared `AgentRuntime`, the `EventBus`, the
**[session](../glossary.md#session)** ID the TUI will attach to, and an
optional `ApprovalBroker`. The function:

1. Enables raw mode on the terminal.
2. Switches to the alternate screen and enables mouse capture via
   `crossterm::execute!`.
3. Builds a `ratatui::backend::CrosstermBackend` over stdout and wraps it
   in a `Terminal`.
4. Calls `app::run_app`, which owns the main loop.
5. Restores the terminal — disables raw mode, leaves the alternate screen,
   disables mouse capture, shows the cursor — regardless of whether the
   main loop returned `Ok` or `Err`. The result from the main loop is
   returned unchanged.

This layering is deliberate: raw mode and the alternate screen are toggled
exactly once, and any panic or error during `run_app` still falls back
through the cleanup sequence before the binary exits. The rest of the
crate can assume the terminal is in raw, alternate-screen mode with mouse
capture enabled.

## App state

`App` in `crates/ryvos-tui/src/app.rs:34` holds every piece of state the
UI renders. The interesting fields:

- `messages: Vec<DisplayMessage>` — the committed message history. Each
  entry has a `role` of `User`, `Assistant`, `Tool`, `Error`, or
  `System` (the `MessageRole` enum in the same file), and the text that
  should appear alongside it.
- `streaming_text: String` — the in-flight assistant text for the current
  turn. Text deltas are appended here and flushed into `messages` as a
  single `Assistant` entry when the turn ends or a tool call begins.
- `input: InputHandler` — the input buffer and cursor (see below).
- `session_id: SessionId` — the session the TUI is driving.
- `is_running: bool` — whether a run is currently in progress. Used to
  gate input submission and to toggle the status bar's spinner.
- `active_tool: Option<String>` — the name of the tool currently executing,
  displayed in the status bar.
- `scroll_offset: usize` — how far above the bottom of the message list
  the user has scrolled. Reset to zero whenever a run finishes so that
  auto-scroll resumes.
- `total_input_tokens`, `total_output_tokens` — cumulative token counts
  across the lifetime of the session.
- `tick_count: usize` — monotonic counter incremented every 100 ms tick;
  used to drive the spinner frame index.

## handle_agent_event

`App::handle_agent_event` in the same file is an exhaustive match on every
`AgentEvent` variant produced by the runtime. The match is structured
around the five display roles:

- `RunStarted` flips `is_running` to true and clears any stale
  `streaming_text` from a previous run.
- `TextDelta(text)` appends to `streaming_text` with no side effects.
- `ToolStart { name, .. }` sets `active_tool`, flushes any pending
  `streaming_text` as an `Assistant` message, and pushes a `Tool` message
  of the form `Running: {name}`.
- `ToolEnd { name, result }` clears `active_tool` and pushes a `Tool`
  message of the form `[{name}: ok|ERROR] {content}`. If the tool's
  content is longer than 200 characters, the tail is replaced with an
  ellipsis so that one large output does not drown the backlog.
- `RunComplete { input_tokens, output_tokens, .. }` clears `is_running`,
  accumulates the token counts, flushes any residual `streaming_text`,
  and resets the scroll offset.
- `RunError { error }` clears `is_running`, drops any in-flight stream,
  and pushes an `Error` message with the error string.
- `ApprovalRequested { request }` pushes a `System` message that names the
  tool, its tier, the truncated input summary, and the slash commands the
  user should type: `/approve <short-id>` or `/deny <short-id>`. The
  short ID is the first eight characters of the request ID, which is
  enough for `ApprovalBroker::find_by_prefix` to resolve.
- `ToolBlocked { name, tier, reason }` pushes an `Error` message in the
  `[BLOCKED] {name} ({tier}): {reason}` form.
- `HeartbeatFired`, `HeartbeatOk`, and `HeartbeatAlert` all push `System`
  messages that carry the **[Heartbeat](../glossary.md#heartbeat)** status.
  Firing logs the current time; OK logs the response character count;
  alert logs the alert body.
- `GuardianStall`, `GuardianDoomLoop`, and `GuardianBudgetAlert` all push
  `System` messages that summarize the **[Guardian](../glossary.md#guardian)**'s
  finding — elapsed time and turn for a stall, tool name and consecutive
  count for a **[doom loop](../glossary.md#doom-loop)**, used vs budgeted
  tokens for a budget alert.
- `GraphGenerated`, `NodeComplete`, `EvolutionTriggered`, and
  `SemanticFailureCaptured` expose the **[Director](../glossary.md#director)**
  through `[DIRECTOR]` system messages, so that a goal-driven run is
  visually distinct from a bare ReAct run.
- `GoalEvaluated` shows the `[GOAL PASSED]` or `[GOAL FAILED]` marker
  with the overall score rounded to a percentage.
- `JudgeVerdict` shows the four **[Verdict](../glossary.md#verdict)**
  variants as `[JUDGE] Accepted`, `[JUDGE] Retry`, `[JUDGE] Escalated`, or
  `[JUDGE] Continue`.

A handful of events — `TurnComplete`, `ApprovalResolved`, `CronFired`,
`GuardianHint`, `UsageUpdate`, `DecisionMade`, `CronJobComplete`,
`BudgetWarning`, `BudgetExceeded` — are intentionally dropped because the
TUI has no place to show them without creating noise; they are visible in
the daemon's JSONL run log and in the Web UI.

## EventLoop

`EventLoop` in `crates/ryvos-tui/src/event.rs:20` merges three asynchronous
sources into one `TuiEvent` stream:

- Keyboard input through `crossterm::event::poll` and `read`, called from
  `tokio::task::spawn_blocking` so that the blocking crossterm API does
  not stall the tokio runtime. The poll timeout is 50 ms.
- Agent events through `broadcast::Receiver::recv`. If the receiver lags
  (because the TUI fell behind a burst of deltas), the loop emits a
  `Tick` rather than surfacing the `Lagged` error — the next draw will
  pick up whatever is in the `App` by then.
- A 100 ms tick timer for animations.

`EventLoop::next` is one `tokio::select!` across the three branches. The
merged `TuiEvent` enum has variants for `Key`, `Resize`, `Agent`, and
`Tick`. Every branch yields at most once per call, and the main loop in
`run_app` calls `next` again on every iteration, so the UI stays
responsive even when the EventBus is quiet.

## Input handling

`InputHandler` in `crates/ryvos-tui/src/input.rs:28` is a small buffer
with a cursor. `handle_key` returns an `InputAction` enum with variants
for `Submit`, `Newline`, `Quit`, `Clear`, `ScrollUp`, `ScrollDown`,
`Approve`, `Deny`, `Soul`, and `None`.

The key bindings are:

- `Enter` on a non-empty buffer that does not start with `/` submits the
  text as a new user message.
- `Shift+Enter` inserts a literal newline into the buffer, so multi-line
  prompts are possible.
- `Ctrl+C` returns `Quit`.
- `Backspace`, `Delete`, `Left`, `Right`, `Home`, `End` implement standard
  line editing against the cursor position.
- `PageUp` and `PageDown` scroll the message list by three lines per
  press.

Five slash commands are parsed when the buffer starts with `/` and
`Enter` is pressed:

- `/quit`, `/exit`, and `/q` all return `Quit`.
- `/clear` returns `Clear`, which in `run_app` wipes the `messages` vec
  and pushes a `System` message confirming the clear.
- `/soul` returns `Soul`, which pushes a hint telling the user to run
  `ryvos soul` in a separate terminal. The soul interview is not run
  inside the TUI because it uses line-buffered prompts that do not
  interoperate with raw mode.
- `/approve <prefix>` returns `Approve(prefix)`. The main loop then calls
  `ApprovalBroker::find_by_prefix` to resolve the full request ID and
  `ApprovalBroker::respond` with `ApprovalDecision::Approved`. A
  confirmation message is pushed on success; a missing broker or
  unmatched prefix produces an `Error` message.
- `/deny <prefix> [reason]` returns `Deny(prefix, reason)`. The reason is
  passed through to `ApprovalDecision::Denied`; if omitted, the default
  is `"denied by user"`.

## Rendering

`ui::draw` in `crates/ryvos-tui/src/ui.rs:43` is called on every loop
iteration. The layout is vertical with up to four regions:

1. **Optional banner** at the top. A `BannerCache` computed once from
   `tui_banner::Banner::new("RYVOS").style(Style::NeonCyber).render()`
   stores the rendered lines, their maximum width, and their height. The
   banner is drawn at full size when the terminal is both wide enough for
   the ASCII art and tall enough to leave at least eight rows for
   messages plus the status bar plus the input. On a shorter terminal,
   the banner collapses to a single compact line rendering "RYVOS" in
   bold cyan followed by a tagline in dark gray. On a terminal shorter
   than eleven rows total, the banner is omitted entirely to preserve
   message space.
2. **Message list**, constrained to `Min(1)`, rendered as a `Paragraph`
   with `Wrap { trim: false }` and a full-border block titled `Messages`.
   Each message is prefixed by a role marker and styled:
   `User` shows `>` in bold cyan; `Assistant` shows no prefix in white;
   `Tool` shows `[tool]` in yellow; `Error` shows `[error]` in bold red;
   `System` shows `[system]` in dark gray. In-flight `streaming_text` is
   appended beneath the committed messages as plain white lines so the
   user sees text arrive as it streams. Scroll position is computed from
   `scroll_offset` so that the default view sticks to the bottom and the
   user can `PageUp` to walk back through history.
3. **Status bar**, a one-row `Paragraph` with a dark-gray background. When
   `is_running` is false it shows
   `Session: <8-char> | Tokens: <in>in/<out>out | /quit to exit`. When
   `is_running` is true it replaces the content with a ten-frame braille
   spinner (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) indexed by `tick_count / 2`, the literal text
   `Thinking...`, and the active tool name in brackets if present.
4. **Input box**, a three-row `Paragraph` with a full-border block titled
   `Input`. After rendering, `set_cursor_position` places the terminal
   cursor at the current buffer cursor inside the box so the operating
   system's cursor blink tracks the user's editing position.

## Main loop

`run_app` in `crates/ryvos-tui/src/app.rs` is the main loop. Every
iteration draws the UI, waits for the next `TuiEvent` from the merged
event loop, and dispatches it:

- `Key` events go through `input.handle_key` and the resulting
  `InputAction` drives the main loop. `Submit` pushes the user message
  into `messages`, resets the scroll offset, and spawns a tokio task that
  calls `runtime.run(&session_id, &text)` and publishes `RunError` on
  the EventBus if the call fails. The run itself streams back through
  `TuiEvent::Agent` events, which the same loop will consume on
  subsequent iterations.
- `Agent` events are handed to `App::handle_agent_event`.
- `Tick` events increment `tick_count`.
- `Resize` events cause the next `draw` call to pick up the new frame
  area; no explicit handling is needed.

The loop exits when `Quit` is returned from the input handler or when
`next` returns `None` (the crossterm side of the event loop has closed).

## Where to go next

The TUI is launched from the `ryvos` binary's top-level command and from
the `ryvos tui` subcommand. For the full set of CLI commands and their
flags, read [../operations/cli-reference.md](../operations/cli-reference.md).
