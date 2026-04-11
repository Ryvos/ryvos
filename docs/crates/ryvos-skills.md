# ryvos-skills

`ryvos-skills` is the drop-in plugin system for Ryvos. A **[skill](../glossary.md#skill)**
is any executable command — a Python script, a compiled binary, a shell
pipeline, a Node.js CLI — wrapped in a TOML manifest that declares its name,
description, input schema, and runtime prerequisites. Validated skills are
registered into the **[tool registry](../glossary.md#tool-registry)** on
startup and become indistinguishable from built-in tools from the agent's
point of view: the LLM sees a uniform list of tools with uniform schemas,
and the **[security gate](../glossary.md#security-gate)** audits skill calls
exactly like it audits `bash` or `fs_read`.

The crate has two surfaces: a local loader that walks `~/.ryvos/skills/` and
registers every valid manifest, and a remote registry client that downloads
verified skill tarballs from a JSON index URL. The local loader is always
on; the remote client is opt-in via the `ryvos skill` CLI subcommand.

## Position in the stack

`ryvos-skills` sits in the integration layer. It depends on `ryvos-core` for
the `Tool` trait, `ToolResult`, `ToolContext`, and `RyvosError`, and on
`ryvos-tools` for `ToolRegistry`. External dependencies are small:
`sha2` for SHA-256 verification of downloaded tarballs, `reqwest` for the
HTTPS fetch, `toml` for manifest parsing, and `tokio` for the subprocess
runtime. See `crates/ryvos-skills/Cargo.toml`.

## Directory layout

Skills live under `~/.ryvos/skills/`, one subdirectory per skill:

```text
~/.ryvos/skills/
  weather_lookup/
    skill.toml
    weather.py
  docker_manager/
    skill.toml
    bin/docker-wrap
  ...
```

The loader scans the skills directory, treats each subdirectory as a
candidate skill, and looks for a `skill.toml` file inside. Any subdirectory
without a manifest is silently skipped; this keeps a partially-unpacked
archive or a scratch directory from breaking startup. A malformed manifest
logs a warning and is skipped — the rest of the skills still load.

## Manifest

`SkillManifest` in `crates/ryvos-skills/src/manifest.rs:21` is the TOML
shape every manifest must parse into. Fields:

- `name` — unique tool name. The LLM sees this in the tool list and calls
  it by name, so it must be a valid identifier.
- `description` — human-readable sentence that describes what the skill
  does. This is injected into the LLM's tool schema.
- `command` — shell command to execute. The literal string `$SKILL_DIR` is
  substituted with the absolute path of the skill's directory at runtime,
  so scripts can reference bundled files with stable paths.
- `timeout_secs` — optional, defaults to 30. Hard upper bound on a single
  execution of the skill.
- `requires_sandbox` — optional, defaults to `false`. Advisory flag for
  callers that want to run the command in a tighter sandbox; the skill
  runtime itself does not enforce sandboxing.
- `input_schema_json` — optional, defaults to `{"type":"object","properties":{}}`.
  A JSON string containing an OpenAI-compatible function parameter schema.
  The loader parses this into a `serde_json::Value` at load time; a
  malformed schema fails the skill's creation with a `Config` error.
- `tier` — optional, defaults to `"t2"`. Informational security tier
  ([T0 through T4](../glossary.md#t0t4)); parsed into `SecurityTier` via
  its `FromStr` impl, falling back to `T2` if the string is unrecognized.
  Tiers are retained as metadata only — under
  [passthrough security](../glossary.md#passthrough-security), the gate
  never blocks a skill based on its tier.
- `prerequisites` — optional table described in the next section.

An illustrative minimal manifest looks like this:

```toml
name = "weather_lookup"
description = "Look up current weather for a city"
command = "python3 $SKILL_DIR/weather.py"
timeout_secs = 15
input_schema_json = '{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}'
```

## Prerequisites

`Prerequisites` in the same file has three fields, all optional:

- `required_binaries` — a list of executable names that must be found on
  `PATH`. Before creating the `SkillTool`, the loader runs each name
  through a small `which` helper that splits `$PATH` on `:` and checks
  whether the name is an executable file in any entry. A missing binary
  skips the skill with a warning and does not register it.
- `required_env` — a list of environment variable names that must be set.
  A missing variable skips the skill.
- `required_os` — optional platform gate. Accepts `"linux"`, `"macos"` or
  `"darwin"`, or `"windows"`. The loader compares against
  `std::env::consts::OS` and skips the skill on a mismatch with a
  descriptive log line.

The `check_prerequisites` function in
`crates/ryvos-skills/src/lib.rs:107` returns `Err(reason)` with the first
failing check, and the loader logs the reason at warn level so that users
can diagnose why a skill did not appear in the tool list without restarting
in a different environment.

Prerequisites fail *gracefully*: a skill declared on a Linux-only host
that happens to be installed on macOS simply does not load, rather than
crashing startup. This is important when a shared `~/.ryvos/skills`
directory is synced across machines.

## SkillTool

`SkillTool` in `crates/ryvos-skills/src/skill_tool.rs:13` is the `Tool`
trait implementation that every loaded skill reduces to. It holds the
parsed manifest, the absolute `skill_dir`, and a pre-parsed
`serde_json::Value` for the input schema so that parsing is paid exactly
once per skill rather than once per tool call.

The five `Tool` trait methods map to manifest fields directly:
`name` returns `manifest.name`, `description` returns `manifest.description`,
`input_schema` clones the cached schema, `tier` parses `manifest.tier` into
a `SecurityTier`, and `timeout_secs` and `requires_sandbox` return their
manifest values.

`execute` is where the work happens. The method:

1. Substitutes `$SKILL_DIR` in the command string with the real directory
   path.
2. Serializes the input `serde_json::Value` into a byte vector.
3. Wraps the whole execution in `tokio::time::timeout` with
   `Duration::from_secs(manifest.timeout_secs)`.
4. On non-Windows, spawns `bash -c <command>`; on Windows, spawns
   `cmd /C <command>`. This lets the manifest use arbitrary shell
   features (pipes, redirections, variable expansion) without the runtime
   having to parse them.
5. Sets the child's working directory to `ctx.working_dir`, and pipes
   `stdin`, `stdout`, and `stderr`.
6. Writes the serialized input to the child's stdin and drops the handle
   so the child sees EOF.
7. Calls `wait_with_output` to collect the exit status and both output
   streams.

The result is mapped back to a `ToolResult`:

- Exit status zero with non-empty stdout → `ToolResult::success(stdout)`.
- Exit status zero with empty stdout → `ToolResult::success("(no output)")`.
- Non-zero exit → `ToolResult::error("Exit code N\n<stderr or stdout>")`.
- IO error during spawn or wait → `Err(RyvosError::ToolExecution)`.
- Timeout elapsed → `Err(RyvosError::ToolTimeout)`.

Stdout and stderr are both decoded with `String::from_utf8_lossy`, so a
skill that emits invalid UTF-8 will see replacement characters in its
result rather than crashing the agent loop.

The convention that JSON input arrives on stdin is deliberate: it mirrors
the way MCP stdio transports work and avoids the quoting problems that
arise when passing structured data as command-line arguments. A Python
skill reads `sys.stdin.read()` and `json.loads` it; a Bash skill uses `jq`
or `cat`; a Rust binary reads `io::stdin().lock()` and feeds it to
`serde_json::from_reader`.

## Loading

`load_and_register_skills(dir, registry)` in
`crates/ryvos-skills/src/lib.rs:34` is the single entry point the daemon
calls at startup. It calls `load_skills(dir)` to build a `Vec<SkillTool>`,
then registers each tool into the `ToolRegistry` and returns the count.
`load_skills` is the function that actually walks the directory, parses
manifests, checks prerequisites, and constructs `SkillTool`s.

Every step failure is non-fatal: an unreadable directory returns an empty
vector with a debug log; a missing manifest is silently skipped with a
debug log; a malformed manifest logs at warn level; a failing prerequisite
logs at warn level with the reason; a `SkillTool::new` failure (bad input
schema JSON) logs at warn level. The only way a single bad skill can
break startup is if it panics during parsing, which is why the loader
uses `toml::from_str` and `serde_json::from_str` rather than any code that
could unwrap.

## Remote registry

`crates/ryvos-skills/src/registry.rs` contains the client for a remote
skill registry. The index is a JSON document with the following shape:

- `version` — integer index format version, defaults to 1.
- `skills` — array of `RegistryEntry` objects.

Each `RegistryEntry` has `name`, `description`, `version`, optional
`author`, `tarball_url`, `sha256`, `tier` (default `"t1"`), and `tags`.
The tarball URL must point at a `.tar.gz` whose top-level directory
contains a `skill.toml`.

Four operations are exposed:

- `fetch_index(url)` performs an HTTPS GET with a `User-Agent` header of
  `ryvos-skill-registry/<version>` and deserializes the response into a
  `RegistryIndex`.
- `search_skills(&index, query)` does a case-insensitive substring match
  against each entry's name, description, and tag list.
- `install_skill(&entry, skills_dir)` downloads the tarball, verifies the
  SHA-256 checksum byte-for-byte against `entry.sha256` (an empty string
  is rejected), removes any existing installation of the same name,
  writes the archive to `skills_dir/<name>.tar.gz`, and extracts it with
  `tar xzf --strip-components=1 -C <skill_dir>`. On extraction failure,
  the half-unpacked directory is removed; on a missing `skill.toml` after
  extraction, the directory is removed and an error is returned. The
  downloaded tarball is always deleted whether extraction succeeds or
  fails.
- `remove_skill(name, skills_dir)` deletes `skills_dir/<name>/`, returning
  an error if the directory does not exist.
- `list_installed(skills_dir)` returns a sorted list of directory names
  that contain a `skill.toml`.

SHA-256 verification is the only integrity mechanism. There is no
signature layer; operators who need stronger trust should run their own
registry behind a mutual-TLS proxy or point the registry URL at a file
inside a Git-tracked directory. The `sha256_hex` helper in the same file
uses `sha2::Sha256` and formats the digest as lowercase hex.

## CLI

The `ryvos skill` subcommand in the main binary is the only normal way to
touch the remote registry. The four operations map directly to the
functions above:

- `ryvos skill list` reads `list_installed` and prints the result.
- `ryvos skill search <query>` fetches the index and runs `search_skills`.
- `ryvos skill install <name>` fetches the index, looks up the entry by
  name, and calls `install_skill`.
- `ryvos skill remove <name>` calls `remove_skill`.

The CLI commands live in `crates/ryvos/src/commands/skill.rs`; this crate
exposes only the primitives they use.

## Where to go next

To package a new skill and distribute it through a registry, read
[../guides/adding-a-skill.md](../guides/adding-a-skill.md). The guide walks
through manifest authoring, SHA-256 generation, and the tar layout the
installer expects. For the `Tool` trait itself and how the tool registry
dispatches calls, read [../internals/tool-registry.md](../internals/tool-registry.md).
