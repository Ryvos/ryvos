# ADR-002: Passthrough Security Instead of Blocking

## Status

Accepted

## Context

The traditional approach to agent security is tier-based blocking. You classify
tools and commands into risk tiers (T0 through T4, say) and gate execution behind
approval workflows. High-risk actions get blocked until a human approves them.
This is the pattern most agent frameworks use.

We tried this. It was terrible.

In practice, blocking creates three problems. First, it kills agent autonomy.
The whole point of an agent is that it works while you are away. If it stops and
waits for approval every time it wants to run a shell command or modify a file,
you have just built a very expensive clipboard. Second, users learn to
rubber-stamp approvals. After the fifth "Are you sure?" prompt, people start
clicking Yes without reading. The security theater provides zero actual safety.
Third, it is impossible to classify risk accurately in advance. A command like
`rm -rf` is dangerous or harmless depending entirely on its arguments and context.
Static tier classification cannot capture that nuance.

We needed a model that preserves agent usefulness while still providing meaningful
safety guarantees.

## Decision

Ryvos never blocks tool execution by default. Instead, we use a passthrough
security model built on five pillars:

1. **Audit everything.** Every tool call, its arguments, its result, and its
   timing are recorded in the audit database. Nothing is invisible.

2. **Detect patterns.** The Guardian subsystem watches tool executions in real
   time and flags anomalies. Repeated failures, unusual command patterns, or
   access to sensitive paths trigger warnings, not blocks.

3. **Record lessons.** When something goes wrong (and the user or the agent
   identifies it), the safety subsystem records that lesson. Future executions
   in similar contexts will have that lesson injected as context.

4. **Constitutional AI principles.** Every system prompt includes constitutional
   principles: do not destroy data without confirmation, prefer reversible
   actions, explain before acting on sensitive resources. The LLM's own
   judgment is the first line of defense.

5. **Optional approval checkpoints.** For users who want manual gates, the
   approval UI is available through any connected channel (Telegram, web UI).
   But it is opt-in, not mandatory.

The key insight is that safety is a learning problem, not a classification
problem. The agent gets safer over time as it accumulates lessons, not because
we built a bigger blocklist.

## Consequences

**What went well:**

- The agent always completes its work. Users do not experience random stops
  or approval fatigue. This is a major usability win.
- Safety actually improves over time. Each incident becomes a lesson that
  prevents recurrence. Blocking-based systems are static by comparison.
- The full audit trail means you can always reconstruct what happened and why.
  This is more useful for post-incident analysis than a list of blocked
  attempts.
- The constitutional approach aligns with how modern LLMs actually work. They
  are already trained to be cautious. Injecting safety context reinforces that
  training rather than fighting it.

**What is riskier:**

- A truly dangerous command can execute. If the LLM decides to run something
  destructive and the constitutional principles do not catch it, there is no
  hard gate. This is a real risk.
- Mitigation: the Docker sandbox (when enabled) provides containment. The agent
  can run in an isolated environment where destructive actions cannot escape.
- Mitigation: the approval UI gives users a manual checkpoint for specific tool
  categories if they want one.
- This approach requires trust in the LLM's judgment. As models improve, this
  gets safer. But it does mean that a weaker model might make worse decisions.

We believe this is the right tradeoff for a personal agent that is meant to be
genuinely useful. The blocking approach optimizes for preventing the worst case
at the cost of making the common case painful. We optimize for the common case
and mitigate the worst case through other means.
