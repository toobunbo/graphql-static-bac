mod envelope;
mod ids;
mod policy;
mod policy_oracle;
mod s0;
mod s1;
mod s2;
mod s3;
mod s3_dag;
mod s3_routes;
mod s4_seed;
mod seed_runtime;
mod type_ref;

pub use envelope::{AnalysisScope, ArtifactStatus, Envelope, Producer, StageId, Warning};
pub use ids::{arg_id, field_id, input_field_id, type_id};
pub use policy::{PolicyClass, PolicyHypothesesData, PolicyHypothesis};
pub use policy_oracle::{
    PolicyCandidateAuthContext, PolicyOracleData, PolicyOracleExecution,
    PolicyOracleExecutionStatus, PolicyOracleFailure, PolicyOracleFailureCode, PolicyOracleOutcome,
    PolicyOraclePhase, PolicyOracleResult, PolicyVictim, PolicyViolationCandidate,
    PolicyViolationCandidates, PolicyViolationVerdict,
};
pub use s0::{
    ArgumentDefinition, EnumValueDefinition, FieldDefinition, InputFieldDefinition, SchemaIrData,
    SchemaRoots, TypeDefinition,
};
pub use s1::{SelectedType, SinkRef, SinksData};
pub use s2::{
    ArgumentClassification, ArgumentsData, ClassifiedArgument, ClassifiedField, Confidence,
};
pub use s3::{
    CandidateAccessPath, CapOrigin, CycleTemplate, EnumerationStatus, PathEdge, PathEdgeKind,
    PathsData, TargetPaths,
};
pub use s3_dag::{
    CardinalityStatus, DagCanonicalWitness, DagComponent, DagComponentEdge, DagCoverage,
    DagEdge, DagOriginMode, DagRoutesData, DagSelectorFact, DagState, DagTerminal,
    DagTransition, DagTransitionEffect, FamilyCardinality, TargetDag,
};
pub use s3_routes::{
    BoundaryFamily, BoundarySource, Reachability, Route, RouteBoundary, RouteOrigin, RouteSelector,
    RouteCoverage, RouteSignature, RouteVerdict, RouteWitness, RoutesData, SelectorClass,
    SelectorContinuity, TargetRoutes,
};
pub use s4_seed::{
    AnchorInstanceRule, BindingSetPlan, Cardinality, CorrelationBasis, CorrelationBasisKind,
    CorrelationConstraint, DependencyDag, DependencyEdge, ExtractionAnchor, ExtractionMember,
    ExtractionPlan, PlanStatus, ProducerCandidate, ProducerDerivation, ProducerJob, ProducerLocus,
    ProducerStrategy, ProducerWitness, RequirementSource, RouteSeedPlan, SeedPlansData,
    SeedRequirement, StaticBinding, StaticBindingClass, TypeCompatibility, UnresolvedReason,
    UnresolvedRequirement,
};
pub use seed_runtime::{
    AdapterAttempt, BindingValidation, BindingValidationStatus, ExecutionKind, ExecutionRecord,
    ExtractionProvenance, FailureCode, RouteRuntimeResult, RouteRuntimeStatus, RuntimeBinding,
    RuntimeCoverage, RuntimeExecutionStatus, RuntimeFact, RuntimeFailure, SeedRuntimeData,
    VerifiedBindingSet,
};
pub use type_ref::{TypeKind, TypeRef, TypeRefError, TypeWrapper};
