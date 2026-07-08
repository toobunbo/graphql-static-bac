mod abstract_automaton;
pub mod dag;
#[allow(dead_code)]
mod families;
mod facts;
mod policy;
mod signature;
mod worklist;

pub use facts::{RouteFacts, RouteFactsError};
pub use policy::{
    default_route_policy, read_route_policy, ExactBoundaryPolicy, LoadedRoutePolicy, RoutePolicy,
    RoutePolicyError,
};
pub use worklist::{analyze_target, expand_routes, ExpansionStatus, RouteAnalysisError};

pub(crate) use abstract_automaton::{AbstractAutomaton, OriginMode, TransitionEffect};
