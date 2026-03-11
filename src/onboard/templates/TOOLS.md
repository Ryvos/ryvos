# Tool Usage Conventions

## The Cardinal Rule
**DO the thing. Don't EXPLAIN how to do the thing.**

When asked to perform a task:
1. Use tools to actually do it
2. Report what you did and the result
3. Only explain the process if explicitly asked "how" or "why"

Bad: "To check if the service is running, you can use: systemctl status myservice"
Good: *runs* `systemctl status myservice` → "It's running, active since 2 hours ago."

Bad: "To find the bug, you could look at the logs..."
Good: *runs* `journalctl -u myservice --since '1h ago' | tail -50` → "Found the error: [details]"

## Memory is Your Lifeline
Your context window is finite (~80k tokens). If you don't write it down, you WILL forget it.

### What to write where:
| Information type | Write to | Method |
|------------------|----------|--------|
| Hard facts (IPs, paths, credentials) | `~/.ryvos/memory/facts.md` | Edit or Bash append |
| Project decisions & status | `~/.ryvos/memory/projects.md` | Edit or Bash append |
| User preferences & style | `~/.ryvos/memory/preferences.md` | Edit or Bash append |
| Task completion / activity | `~/.ryvos/memory/YYYY-MM-DD.md` | Bash append |
| Quick unsorted memories | `~/.ryvos/MEMORY.md` (Recent section) | Bash append |
| Corrections to wrong memories | The file containing the error | Edit tool (fix in place) |

### When to write:
- **Immediately** when you learn a new fact, preference, or decision — don't wait
- **At end of every conversation** — daily log entry + MEMORY.md summary
- **When corrected** — fix the wrong entry RIGHT NOW, don't just acknowledge

### Memory maintenance:
Every few conversations, re-read MEMORY.md and move items from Recent into topic files. Keep MEMORY.md under 200 lines. Use Edit to update stale entries.

## Tool Selection
- Quick system check → Bash one-liner
- Read a file → Read tool (not `cat`)
- Search file contents → Grep
- Find files → Glob
- Edit existing file → Edit (precise) or Write (full rewrite)
- Web lookup → WebSearch, then WebFetch for details
- API call → Bash with curl
- Memory write → Bash `echo >> file` for appends, Edit for corrections
