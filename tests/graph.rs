use graphql_static_bac::contracts::TypeKind;
use graphql_static_bac::graph::{build_type_graph, GraphEdgeKind, PlumbingIndex};
use graphql_static_bac::schema::parse_sdl;

#[test]
fn builds_field_and_sorted_type_condition_edges() {
    let schema = parse_sdl(
        r#"
        interface Node { id: ID! }
        type User implements Node { id: ID! }
        type Team implements Node { id: ID! }
        union Search = User | Team
        type Query { node: Node search: Search scalar: String }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();

    let query_edges: Vec<_> = graph
        .outgoing("type:Query")
        .iter()
        .map(|index| graph.edge(*index).edge_id.as_str())
        .collect();
    assert_eq!(query_edges, ["field:Query.node", "field:Query.search"]);
    assert!(!query_edges.contains(&"field:Query.scalar"));

    let node = graph.node("type:Node").unwrap();
    assert_eq!(node.kind, TypeKind::Interface);
    let conditions: Vec<_> = node
        .outgoing
        .iter()
        .filter_map(|index| {
            let edge = graph.edge(*index);
            (edge.kind == GraphEdgeKind::TypeCondition).then_some(edge.edge_id.as_str())
        })
        .collect();
    assert_eq!(
        conditions,
        ["type_condition:Node->Team", "type_condition:Node->User"]
    );
}

#[test]
fn retains_connection_and_edge_nodes() {
    let schema = parse_sdl(
        r#"
        type Query { users: UserConnection }
        type UserConnection { edges: [UserEdge!]! }
        type UserEdge { node: User! }
        type User { id: ID! }
        "#,
    )
    .unwrap()
    .data;
    let graph = build_type_graph(&schema).unwrap();
    assert!(graph.node("type:UserConnection").is_some());
    assert!(graph.node("type:UserEdge").is_some());
    assert_eq!(graph.outgoing("type:UserConnection").len(), 1);
    assert_eq!(graph.outgoing("type:UserEdge").len(), 1);
}

#[test]
fn plumbing_detection_is_structural_and_does_not_hide_query_node() {
    let schema = parse_sdl(
        r#"
        interface Node { id: ID! }
        type User implements Node { id: ID! }
        type PageInfo { hasNextPage: Boolean! }
        type UserConnection {
          pageInfo: PageInfo!
          nodes: [User!]!
          edges: [UserEdge!]!
        }
        type UserEdge { node: User! }
        type FakeConnection { nodes: [User!]! }
        type Ordinary { node: User! }
        type Query {
          node(id: ID!): Node
          users: UserConnection!
          fake: FakeConnection!
          ordinary: Ordinary!
        }
        "#,
    )
    .unwrap()
    .data;
    let plumbing = PlumbingIndex::build(&schema);

    assert!(plumbing.is_plumbing("field:UserConnection.nodes"));
    assert!(plumbing.is_plumbing("field:UserConnection.edges"));
    assert!(plumbing.is_plumbing("field:UserEdge.node"));
    assert!(!plumbing.is_plumbing("field:Query.node"));
    assert!(!plumbing.is_plumbing("field:FakeConnection.nodes"));
    assert!(!plumbing.is_plumbing("field:Ordinary.node"));
}
