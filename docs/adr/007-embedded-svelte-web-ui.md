# ADR-007: Embedded Svelte UI via rust_embed

## Status

Accepted

## Context

Ryvos needs a web dashboard for monitoring agent activity, browsing audit logs,
managing Viking memory, viewing costs, and handling approval requests. The
question is how to deliver that UI.

We considered three approaches:

1. **Separate SPA deployment.** Build the frontend as a standalone app and
   deploy it to a CDN or static host. The frontend talks to the Ryvos HTTP
   API. This is the standard approach for web apps, but it means two things
   to deploy and keep in sync. For users running Ryvos on a home server or
   edge device, asking them to also deploy a frontend is too much friction.

2. **Server-rendered HTML.** Generate HTML on the Rust side using a template
   engine like askama or tera. Simple, but the result is a traditional
   page-reload experience. For a real-time dashboard that needs WebSocket
   updates and smooth interactions, server rendering feels clunky.

3. **Embedded SPA.** Build the frontend to static assets (HTML, JS, CSS) and
   embed them into the Rust binary at compile time. The binary serves its own
   UI. One file to deploy, everything works.

## Decision

We chose option 3. The web UI is a Svelte 5 single-page application that gets
built to static assets and embedded into the Rust binary using the rust_embed
crate.

The build pipeline works like this:

1. The Svelte app lives in a `web/` directory within the repo.
2. Running `npm run build` produces a `dist/` folder with the compiled HTML,
   JS, and CSS.
3. The Rust build includes those files via `#[derive(RustEmbed)]` on a struct
   that points to the `dist/` directory.
4. The gateway HTTP server (built on hyper) serves these embedded assets at
   the root path. Any request that does not match an API route falls through
   to the SPA's index.html for client-side routing.

The UI connects to the same server via WebSocket for real-time event streaming.
It receives the same events that flow through the internal EventBus, formatted
as JSON and pushed to all connected WebSocket clients.

The Svelte app includes views for:
- Live agent activity and event timeline
- Audit log browser with search and filtering
- Viking memory explorer (browse, search, read, write)
- Cost tracking dashboard
- Session history
- Approval request handling

## Consequences

**What went well:**

- Single binary includes everything. Users download one file, run it, and
  open their browser to `http://localhost:3117`. No nginx, no CDN, no
  separate frontend deploy.
- The UI is always version-matched with the backend. There is no possibility
  of running a frontend from v0.5 against a backend from v0.6. They ship
  together.
- Svelte 5 produces small bundles. The entire compiled UI adds roughly 376KB
  to the binary size. That is negligible compared to the 45MB Rust binary.
- Real-time updates through WebSocket work beautifully. The dashboard shows
  tool executions, token counts, and agent state changes as they happen.

**What is harder:**

- Any UI change requires a Rust rebuild. Even if you only changed a CSS color,
  you need to run the Svelte build and then recompile the Rust binary so
  rust_embed picks up the new assets. During development, we mitigate this
  by running the Svelte dev server separately and proxying API requests to
  the Rust backend.
- The embedded approach makes it harder for users to customize the UI. They
  cannot just edit an HTML file on disk. If we ever want user-customizable
  themes or layouts, we would need to add a mechanism for overriding embedded
  assets with local files.
- Svelte is less mainstream than React for open source contributions. Fewer
  developers have Svelte experience. We chose it because of its smaller bundle
  size and simpler mental model, but it does narrow the contributor pool.
- The binary size increase is small now, but could grow if we add more assets
  (images, fonts, icons). We keep an eye on this and optimize where possible.

The embedded approach is a natural fit for a single-binary tool. It keeps the
deployment story simple and ensures the UI always works out of the box.
