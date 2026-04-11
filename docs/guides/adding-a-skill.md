# Adding a skill

## When to use this guide

A **[skill](../glossary.md#skill)** is a drop-in tool packaged as a TOML
manifest plus an executable command. The command can be a Python script, a
shell pipeline, a compiled Rust binary, or anything else that reads JSON
from stdin and writes text to stdout. Skills load from `~/.ryvos/skills/`
at daemon startup and appear in the **[tool registry](../glossary.md#tool-registry)**
indistinguishably from a built-in tool: the LLM sees the same tool schema,
the **[security gate](../glossary.md#security-gate)** audits the call the
same way, and the skill runs inside the daemon's `tokio::time::timeout`
budget like any other tool.

Choose a skill when the logic is in a language other than Rust, when the
implementation is a thin wrapper over a CLI you already have installed,
when you want to distribute the tool separately from the binary, or when
you want operators to be able to replace the tool without rebuilding
Ryvos. Choose a built-in tool instead (see [adding-a-tool.md](adding-a-tool.md))
when the code needs direct access to Ryvos types like `ToolContext` or
the event bus; choose an [MCP server](wiring-an-mcp-server.md) when the
integration is an external service with its own process lifecycle.

## Directory layout

Skills live under `~/.ryvos/skills/`, one subdirectory per skill:

```text
~/.ryvos/skills/
  weather_lookup/
    skill.toml
    weather.py
  csv_summarizer/
    skill.toml
    summarize.sh
```

Each subdirectory is scanned at startup. A subdirectory without a
`skill.toml` is silently ignored, which means a half-unpacked archive
cannot break daemon startup. A malformed manifest logs a warning and is
skipped; the rest of the skills still load.

## Manifest fields

The TOML manifest parses into a `SkillManifest` struct in
`crates/ryvos-skills/src/manifest.rs`. Every field is required unless
marked optional.

- `name` — unique tool name. The LLM calls the skill by this name.
- `description` — one sentence describing what the skill does. Injected
  into the LLM's tool schema.
- `command` — shell command to execute. The literal string `$SKILL_DIR`
  is replaced with the absolute path to the skill's directory at runtime,
  so bundled scripts have stable paths.
- `timeout_secs` — optional, default 30. Hard upper bound per invocation.
- `requires_sandbox` — optional, default `false`. Advisory flag; the
  skill runtime itself does not enforce sandboxing today.
- `input_schema_json` — optional. A JSON string containing an
  OpenAI-compatible function parameter schema. Defaults to an empty
  `object`. Malformed JSON fails skill creation.
- `tier` — optional, default `"t2"`. Informational
  **[T0–T4](../glossary.md#t0t4)** metadata only.
- `prerequisites` — optional table with `required_binaries`,
  `required_env`, and `required_os` fields. A missing prerequisite
  silently skips the skill at load time with a warning.

An illustrative manifest:

```toml
name = "weather_lookup"
description = "Fetch current weather for a city using the OpenWeatherMap API"
command = "python3 $SKILL_DIR/weather.py"
timeout_secs = 15
tier = "t0"
input_schema_json = '''
{
  "type": "object",
  "properties": {
    "city": { "type": "string", "description": "City name, e.g. Barcelona" },
    "units": { "type": "string", "enum": ["metric", "imperial"], "default": "metric" }
  },
  "required": ["city"]
}
'''

[prerequisites]
required_binaries = ["python3"]
required_env = ["OPENWEATHER_API_KEY"]
required_os = "linux"
```

## Writing the script

The skill runtime pipes a serialized JSON object to the child's stdin,
closes stdin, and waits for exit. The script writes its response to
stdout and any diagnostics to stderr. A non-zero exit is recorded as a
tool error.

The contract is strict and simple:

- **Stdin** — a single JSON object matching `input_schema_json`.
- **Stdout** — plain text that the LLM will see as the tool result. Not
  JSON unless the skill explicitly wants to return JSON; the runtime does
  not parse it.
- **Stderr** — diagnostic logs for the operator. Discarded on success,
  included in the error payload on non-zero exit.
- **Exit code** — `0` for success; any other value is a tool error and
  stderr plus stdout are surfaced to the model.

A Python skill reads stdin with `json.load(sys.stdin)`. A Bash skill uses
`jq`. A Rust binary reads with `serde_json::from_reader(io::stdin().lock())`.
Emit human-readable output: the LLM sees the stdout verbatim and reasons
about it.

```python
#!/usr/bin/env python3
import json, os, sys, urllib.request

args = json.load(sys.stdin)
city = args["city"]
units = args.get("units", "metric")
key = os.environ["OPENWEATHER_API_KEY"]

url = f"https://api.openweathermap.org/data/2.5/weather?q={city}&units={units}&appid={key}"
with urllib.request.urlopen(url) as resp:
    data = json.load(resp)

temp = data["main"]["temp"]
desc = data["weather"][0]["description"]
print(f"{city}: {temp}° ({desc})")
```

## Testing locally

Install the skill by dropping it into `~/.ryvos/skills/your_skill/` and
restarting the daemon (or running `ryvos run ...` fresh). Watch the
startup logs — the loader prints a debug line for each skill it tries to
load, a warn line for each skipped one with the reason, and an info line
for successful loads with the count. Common skip reasons:

- `missing skill.toml` — the directory exists but the manifest is not
  there.
- `failed to parse skill.toml` — a syntax error in TOML; the warning
  includes the parse error.
- `missing binary python3` — a prerequisite failed the `which` lookup.
- `missing env OPENWEATHER_API_KEY` — the env var is not set in the
  daemon's environment.
- `wrong OS linux, got macos` — the manifest pins a platform that does
  not match `std::env::consts::OS`.

Once the skill loads, confirm it with `ryvos skill list` (which reads
`~/.ryvos/skills/` and reports installed skills) and ask the agent to use
it: `ryvos run "look up weather for Barcelona"`. The LLM picks the skill
out of the tool catalog by name and description. The audit trail records
the invocation the same way it records a built-in tool.

For faster iteration, run the skill's command directly from a shell with
a test input:

```bash
echo '{"city":"Barcelona"}' | OPENWEATHER_API_KEY=... python3 weather.py
```

The daemon runs exactly this form under the hood (with `$SKILL_DIR`
substituted), so anything that works here will work from the agent.

## Publishing to the remote registry

The remote registry is a JSON index plus SHA-256-verified tarballs. The
index document has the shape:

```json
{
  "version": 1,
  "skills": [
    {
      "name": "weather_lookup",
      "description": "Fetch current weather",
      "version": "1.0.0",
      "author": "you@example.com",
      "tarball_url": "https://example.com/weather-1.0.0.tar.gz",
      "sha256": "6c7d...",
      "tier": "t0",
      "tags": ["weather", "api"]
    }
  ]
}
```

To publish, pack the skill directory into a `.tar.gz` whose top-level
directory contains the `skill.toml` (the installer uses
`tar --strip-components=1` so the outer directory name is discarded),
compute its SHA-256, upload the tarball to any HTTPS-reachable location,
and add an entry to the index JSON. Operators install the skill with
`ryvos skill install <name>`, which fetches the index, downloads the
tarball, verifies the checksum byte-for-byte, and extracts it under
`~/.ryvos/skills/<name>/`. Mismatched checksums abort the install. There
is no signature layer; operators who need stronger trust should run their
own registry behind mutual TLS or point the registry URL at a local file.

## Verification

1. `ryvos skill list` shows the skill in the installed set.
2. Daemon startup logs the skill as `loaded`, not `skipped`.
3. `ryvos run "use my skill"` — the agent picks the skill, the audit
   trail records the call, and the skill's stdout appears in the final
   response.
4. `ryvos audit query --tool weather_lookup` shows the invocation with
   the input summary and the outcome. The
   **[tool registry](../glossary.md#tool-registry)** made no distinction
   between this and a built-in tool; the only difference is the
   `SkillTool` impl that called into your command instead of native Rust.

For the deeper skill runtime story — how the loader resolves
prerequisites, how stdin framing works, how timeouts abort in-flight
skills — read [../crates/ryvos-skills.md](../crates/ryvos-skills.md). For
how the registry dispatches the call alongside built-ins and MCP tools,
read [../internals/tool-registry.md](../internals/tool-registry.md). To
add a native built-in tool instead, follow
[adding-a-tool.md](adding-a-tool.md).
