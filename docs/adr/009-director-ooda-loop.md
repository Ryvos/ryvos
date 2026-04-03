# ADR-009: Director OODA Loop for Goals

## Status

Accepted

## Context

Simple agent loops follow the ReAct pattern: observe the current state, think
about what to do, take an action, repeat. This works for straightforward tasks
like "edit this file" but falls apart for complex multi-step goals.

The problems with simple ReAct for complex goals: no planning (the agent chases
whatever seems most immediate), no parallelism (steps run strictly sequentially),
brittle failure handling (if step 5 of 10 fails, it retries blindly or gives up),
and no progress tracking (the user cannot see the plan or what remains).

We needed a higher-level execution model that handles planning, dependencies,
failure recovery, and visibility.

## Decision

The Director subsystem implements an OODA loop (Observe, Orient, Decide, Act)
for complex goal execution. When the agent receives a goal that requires multiple
steps, the Director takes over and manages the full lifecycle.

The four phases work like this:

**Observe:** The Director gathers context about the current state. What files
exist, what tools are available, what the user has said, what prior attempts
have produced. This context feeds into the planning phase.

**Orient:** The Director generates a DAG (directed acyclic graph) workflow. Each
node in the graph is a discrete task with a description, required inputs, expected
outputs, and dependencies on other nodes. The LLM generates this graph based on
the goal and the observed context. Nodes without dependencies can execute in
parallel.

**Decide:** The Director evaluates which nodes are ready to execute (all
dependencies satisfied), prioritizes them, and selects the next batch. After each
node completes, the Director evaluates the result. Did it succeed? Did it produce
the expected output? Should downstream nodes still execute as planned?

**Act:** The selected nodes execute. Each node runs as a focused agent task with
its own prompt and tool access. Results flow back to the Director for evaluation.

When a node fails, the Director enters a diagnostic sub-loop:

1. Analyze the failure (what went wrong and why).
2. Check if the graph needs restructuring (maybe the failing node should be
   split into smaller steps, or a prerequisite was missing).
3. Evolve the graph by adding, removing, or modifying nodes.
4. Retry the evolved plan.

This retry-with-evolution loop can run up to a configurable limit (default: 3
attempts per node) before the Director escalates to the user.

## Consequences

**What went well:**

- Complex goals get broken into visible, trackable steps. The user can see
  the plan, monitor progress, and understand what the agent is doing. This
  is a huge improvement over a black-box ReAct loop.
- The DAG structure enables parallelism. Independent tasks (like creating a
  database schema and writing a Dockerfile) can run concurrently, reducing
  total execution time.
- Failure recovery is structured. Instead of blind retries, the Director
  diagnoses failures and adapts the plan. This handles cases where the
  original approach was wrong, not just cases where a transient error
  occurred.
- The OODA framing gives us a clear vocabulary for what the agent is doing at
  any moment. Logs and events use these phase names, making debugging and
  monitoring intuitive.

**What is harder:**

- More LLM calls. Generating the initial graph costs tokens. Evaluating each
  node's result costs tokens. Diagnosing failures costs tokens. For a simple
  task, this overhead is wasteful. We mitigate this by only activating the
  Director for goals that the initial assessment judges as multi-step. Simple
  tasks bypass it and use the standard ReAct loop.
- Graph generation quality depends on the LLM. A weaker model might produce
  a graph with missing dependencies or poorly scoped nodes. The evaluation
  step catches some of these issues, but not all.
- The DAG evolution logic is complex. Modifying a running graph (adding nodes,
  rewiring dependencies) while other nodes are executing requires careful
  coordination. We handle this with a lock on the graph structure during
  evolution phases.
- Simple tasks take longer if they accidentally trigger the Director. The
  routing heuristic (which goals need the Director vs. which can use simple
  ReAct) is imperfect and sometimes gets it wrong.

The OODA loop gives Ryvos a genuine planning and adaptation capability that
sets it apart from simpler agent frameworks. The cost in additional LLM calls
is worth the improvement in reliability for complex goals.
