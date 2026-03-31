# Heartbeat — Periodic Check-In Routine

## Schedule
- Fires at the interval configured in config.toml (default: every 30 minutes)
- Only runs during active hours if configured

## Checklist (run every heartbeat)

### 1. System Health
- Check uptime: `cat /proc/uptime`
- Check memory: `awk '/MemAvailable/{printf "RAM: %.1fG\n", $2/1048576}' /proc/meminfo`
- Check swap: `awk '/SwapTotal/{t=$2} /SwapFree/{printf "Swap: %.1fG used\n", (t-$2)/1048576}' /proc/meminfo`
- Check disk: `df -h / | tail -1`
- Check load: `cat /proc/loadavg`
- Log anomalies (high swap, low disk, high load)

### 2. Git Status
- Check for new upstream commits: `git fetch --dry-run 2>&1`
- Show latest local commits: `git log --oneline -3`

### 3. Viking Memory (persistent learning)
- Use **viking_write** to persist anything new you learned this cycle:
  - New observations → `viking://agent/observations/{topic}`
  - System patterns (e.g., swap behavior, uptime records) → `viking://agent/patterns/{pattern}`
  - User preferences discovered → `viking://user/preferences/{pref}`
- Use **viking_search** before acting if you need to recall past context
- Use **daily_log_write** for the heartbeat log entry (instead of Bash append)

### 4. Self-Reflection (Reflexion)
- Before acting, use **viking_search** to check for lessons from past mistakes
- If a heartbeat fails or produces errors, record the lesson:
  - Use **viking_write** to `viking://agent/lessons/{topic}`
  - Include: what went wrong, why, what to do differently
- If the user corrected you in any channel, record that correction as a lesson
- Review lessons before repeating similar operations
- Goal: never make the same mistake twice

### 5. Report
- Log heartbeat result using **daily_log_write**
- Only alert the user if something actionable is found
- Silence means everything is fine

## Philosophy
Be a good watchdog. Bark when there's a burglar, not when a leaf falls. Use Viking memory to learn and remember across heartbeats — you are a persistent agent, not a stateless script.
