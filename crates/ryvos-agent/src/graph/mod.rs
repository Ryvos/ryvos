//! Graph Execution Engine — DAG-based multi-step workflow orchestration.
//!
//! A workflow is a directed graph of `Node`s connected by `Edge`s.
//! Each node is an independent agent run (via `AgentRuntime::run_with_goal`).
//! Edges define transitions with conditions (Always, OnSuccess, OnFailure,
//! Conditional expression, LlmDecide).
//!
//! The `GraphExecutor` walks the graph from an entry node, executing each node
//! and following edges based on conditions, passing data between nodes via a
//! shared context map.

pub mod edge;
pub mod executor;
pub mod handoff;
pub mod node;

pub use edge::{Edge, EdgeCondition};
pub use executor::{ExecutionResult, GraphExecutor, NodeResult};
pub use handoff::HandoffContext;
pub use node::Node;
