mod builder;
mod model;
mod plumbing;
mod reachability;

pub use builder::{build_type_graph, GraphBuildError};
pub use model::{GraphEdge, GraphEdgeKind, GraphNode, TypeGraph};
pub use plumbing::PlumbingIndex;
pub use reachability::reverse_reachable;
