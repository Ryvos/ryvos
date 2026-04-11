# Deployment

Ryvos ships as a single statically-linked binary. The same file is the REPL, the
terminal UI, the HTTP gateway, the channel daemon, the MCP stdio server, and the
standalone **[Viking](../glossary.md#viking)** memory server. There are no
runtime dependencies, no interpreter, and no companion services. Deployment is
therefore the problem of getting one file onto a host, giving it a config, and
keeping it running.

This document covers the five supported paths: direct binary install, Cargo
install from source, Docker, platform service managers (systemd and launchd),
and Fly.io. Each path ends at the same runtime shape — a long-lived `ryvos
daemon --gateway` process listening on TCP `18789` by default — and differs
only in how the binary gets onto disk and how the process is supervised. For
the full set of configuration knobs the running daemon reads, see
[configuration.md](configuration.md).

## Single-binary install

The canonical install path is `install.sh`, which downloads the correct
release asset from GitHub and places it on `PATH`. The release workflow in
`.github/workflows/release.yml` publishes five artifacts per tag, and the
installer picks one based on `uname -s` and `uname -m`:

| Target triple | Artifact name |
|---|---|
| `x86_64-unknown-linux-musl` | `ryvos-linux-x86_64` |
| `aarch64-unknown-linux-musl` | `ryvos-linux-aarch64` |
| `x86_64-apple-darwin` | `ryvos-macos-x86_64` |
| `aarch64-apple-darwin` | `ryvos-macos-aarch64` |
| `x86_64-pc-windows-msvc` | `ryvos-windows-x86_64.exe` |

The Linux builds are statically linked against musl, so they run on any glibc
or musl distribution without library surgery. The macOS builds are codesigned
ad hoc and work on both Intel and Apple Silicon. Windows is experimental — it
compiles and runs, but channel adapters and the systemd/launchd installer
paths are a no-op on Windows.

```bash
curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
# or manually:
curl -L https://github.com/Ryvos/ryvos/releases/latest/download/ryvos-linux-x86_64 \
  -o /usr/local/bin/ryvos
chmod +x /usr/local/bin/ryvos
ryvos --version
```

To pin a version, export `RYVOS_VERSION=v0.8.3` before piping `install.sh`. To
change the install directory, export `RYVOS_INSTALL_DIR=$HOME/.local/bin`.

## Cargo install from source

Rust 1.75 or newer is the minimum toolchain. From a fresh clone of the repo:

```bash
cargo install --path . --locked
```

This produces the same binary as the release workflow but compiled against
the local libc instead of musl. `--locked` is recommended so that the Cargo
lockfile pins every transitive dependency to the version tested in CI. Debug
builds are not supported for daemons — the **[Guardian](../glossary.md#guardian)**
and **[Director](../glossary.md#director)** both rely on release-profile
optimizations for their latency budgets.

## Docker

The repo ships a multi-stage `Dockerfile` at the root. Stage one builds a
statically linked musl binary with `strip` and thin LTO; stage two is Alpine
3.21 (about 5 MB base) with `ca-certificates`, `curl`, and `git`. The entry
point is `ryvos daemon --gateway`, the workspace is mounted at `/data`, and a
`HEALTHCHECK` pings `/api/health` every 30 seconds.

```bash
docker build -t ryvos:0.8.3 .
docker run -d --name ryvos \
  -p 18789:18789 \
  -v ryvos-data:/data \
  -e ANTHROPIC_API_KEY=sk-ant-... \
  -e RYVOS_GATEWAY_TOKEN=change-me \
  --restart unless-stopped \
  ryvos:0.8.3
```

The image runs as the non-root `ryvos` user and expects the workspace to be a
volume — the seven SQLite databases described in
[../architecture/persistence.md](../architecture/persistence.md) live there.
A compose file wraps the same intent and is the shape most users will copy:

```yaml
services:
  ryvos:
    image: ghcr.io/ryvos/ryvos:v0.8.3
    container_name: ryvos
    restart: unless-stopped
    ports:
      - "18789:18789"
    volumes:
      - ./workspace:/data
      - ./config.toml:/data/config.toml:ro
    environment:
      ANTHROPIC_API_KEY: ${ANTHROPIC_API_KEY}
      RYVOS_GATEWAY_TOKEN: ${RYVOS_GATEWAY_TOKEN}
      RUST_LOG: ryvos=info,warn
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:18789/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3
```

The config file is mounted read-only so that accidental writes from the
container cannot corrupt the host copy. Every environment variable referenced
by the config with `${VAR}` expansion (see
[environment-variables.md](environment-variables.md)) is resolved at daemon
startup against the container's environment, not the host's.

## systemd (Linux)

`ryvos init --yes` on Linux writes a systemd user unit at
`~/.config/systemd/user/ryvos.service` and runs `systemctl --user enable --now
ryvos.service`. The unit is generated from the current binary path and config
path, and always looks like this:

```text
[Unit]
Description=Ryvos AI Agent Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/home/user/.local/bin/ryvos --config /home/user/.ryvos/config.toml daemon
Restart=always
RestartSec=10
Environment=RUST_LOG=ryvos=info

[Install]
WantedBy=default.target
```

`Type=simple` is correct because the daemon does not fork — Tokio runs every
subsystem on the main thread group. `Restart=always` plus `RestartSec=10`
gives a ten-second backoff on crashes; checkpoint resume (see
[../internals/checkpoint-resume.md](../internals/checkpoint-resume.md)) makes
the restart non-disruptive for in-flight runs. User units stop when the user
logs out unless lingering is enabled — the onboarding wizard offers to run
`loginctl enable-linger $USER` on first run, and the same command is safe to
run manually. After install:

```bash
systemctl --user enable --now ryvos.service
systemctl --user status ryvos.service
journalctl --user -u ryvos -f
```

Upgrades follow the pattern in [upgrading.md](upgrading.md): stop the unit,
replace the binary, start the unit. There is no config migration step because
the config is forward-compatible.

## launchd (macOS)

`ryvos init --yes` on macOS writes
`~/Library/LaunchAgents/com.ryvos.agent.plist` and loads it with `launchctl
load`. The plist sets `RunAtLoad` and `KeepAlive` so that the agent starts at
login and auto-restarts on crash, and redirects stdout and stderr to
`~/.ryvos/daemon.log` and `~/.ryvos/daemon.err`. `RUST_LOG=ryvos=info` is set
in the plist's `EnvironmentVariables` dict.

```bash
launchctl unload ~/Library/LaunchAgents/com.ryvos.agent.plist
launchctl load   ~/Library/LaunchAgents/com.ryvos.agent.plist
launchctl list | grep ryvos
```

The LaunchAgent runs as the current user, not as a system daemon. For a
multi-user install, drop the plist into `/Library/LaunchDaemons/` and adjust
`Label` and paths — that path is not automated by `ryvos init`.

## Windows

Windows is experimental. The binary builds and the REPL, TUI, and gateway
run, but there is no service installer and the systemd/launchd path in
`src/onboard/service.rs` is a no-op on that target. Two viable shapes:

- Register `ryvos daemon --gateway` as a Windows **Task Scheduler** task with
  trigger "At log on" and restart settings matching the systemd unit.
- Run Ryvos inside WSL2 and follow the systemd path above. Networking between
  the WSL2 instance and the Windows host is available at `localhost`.

## Fly.io

Fly.io is the reference single-host cloud target. Build the container once
and push it to the registry, then declare a persistent volume for the
workspace:

```bash
fly launch --image ghcr.io/ryvos/ryvos:v0.8.3 --no-deploy
fly volumes create ryvos_data --size 1 --region fra
fly deploy
```

A working `fly.toml` for a single-machine deployment:

```toml
app = "ryvos-example"
primary_region = "fra"

[build]
  image = "ghcr.io/ryvos/ryvos:v0.8.3"

[[mounts]]
  source = "ryvos_data"
  destination = "/data"

[http_service]
  internal_port = 18789
  force_https = true
  auto_stop_machines = false
  auto_start_machines = true
  min_machines_running = 1

[env]
  RUST_LOG = "ryvos=info,warn"
```

Environment secrets (`ANTHROPIC_API_KEY`, `RYVOS_GATEWAY_TOKEN`, channel bot
tokens) are set with `fly secrets set`. Fly's built-in TLS termination
handles the HTTPS upgrade; the container itself speaks plain HTTP to the
runtime on port 18789.

## Reverse proxy

For a self-hosted install on a domain, terminate TLS at a reverse proxy and
forward both HTTP and WebSocket upgrades. A minimal Caddy config:

```text
agent.example.com {
    reverse_proxy 127.0.0.1:18789
}
```

Caddy forwards WebSocket upgrades automatically. For nginx, the equivalent is:

```text
server {
    listen 443 ssl http2;
    server_name agent.example.com;

    location / {
        proxy_pass http://127.0.0.1:18789;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header Authorization $http_authorization;
        proxy_read_timeout 3600s;
    }
}
```

The `Authorization` header must be forwarded verbatim so that the gateway's
role-based auth (see [../api/auth-and-rbac.md](../api/auth-and-rbac.md)) can
validate bearer tokens. The long `proxy_read_timeout` is for the `/ws`
lane — long-lived agent runs stream events for many minutes.

## Firewall and binding

The gateway defaults to `127.0.0.1:18789`, which is unreachable from other
hosts. To expose the gateway on a LAN, set `[gateway] bind = "0.0.0.0:18789"`
and rely on an upstream firewall or the built-in RBAC for access control. Do
not bind directly to a public interface without at least one
`[[gateway.api_keys]]` entry; anonymous access defaults to Admin for
self-hosted convenience, which is the wrong default for a public IP.

## Auto-update

`ryvos update` self-updates from GitHub releases. The command fetches
`https://api.github.com/repos/Ryvos/ryvos/releases/latest`, picks the correct
artifact for `(os, arch)` using `detect_artifact_name` in
`src/main.rs:2259`, downloads the binary, renames the current executable to
`.bak`, atomically renames the new file into place, and cleans up the
backup. On any failure mid-rename the backup is restored. Running daemons
keep using the in-memory binary until restarted, so an update is not live
until `systemctl --user restart ryvos.service` (or the platform equivalent)
runs.

Cross-references:
[configuration.md](configuration.md),
[environment-variables.md](environment-variables.md),
[upgrading.md](upgrading.md),
[../crates/ryvos-gateway.md](../crates/ryvos-gateway.md).
