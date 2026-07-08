use graphql_parser::parse_query;
use serde_json::{json, Value};

#[test]
fn correlation_demo_queries_parse() {
    let query = include_str!("../output/seed-finder.correlation-demo.graphql");
    let document = parse_query::<String>(query).expect("correlation demo queries should parse");
    assert_eq!(document.definitions.len(), 2);
}

#[test]
fn joint_co_read_preserves_profile_deck_correlation() {
    let plan: Value = serde_json::from_str(include_str!(
        "../output/seed-finder.correlation-plan-demo.json"
    ))
    .expect("correlation plan should be valid JSON");
    assert_eq!(
        plan.pointer("/producer_jobs/0/strategy"),
        Some(&json!("joint_co_read"))
    );
    assert_eq!(plan.pointer("/dependency_dag/acyclic"), Some(&json!(true)));

    let response: Value = serde_json::from_str(include_str!(
        "../output/seed-finder.correlation-response-demo.json"
    ))
    .expect("simulated response should be valid JSON");
    let actual = extract_binding_sets(&response);

    let expected: Value = serde_json::from_str(include_str!(
        "../output/seed-finder.correlation-bindings-demo.json"
    ))
    .expect("expected bindings should be valid JSON");
    assert_eq!(
        actual,
        expected
            .get("binding_sets")
            .and_then(Value::as_array)
            .expect("expected binding_sets array")
            .clone()
    );

    assert_eq!(actual.len(), 3);
    assert!(!contains_pair(
        &actual,
        "FootballUserSportProfile:profile-1",
        "profile-2-deck-c"
    ));
    assert!(!contains_pair(
        &actual,
        "FootballUserSportProfile:profile-2",
        "profile-1-deck-a"
    ));
}

fn extract_binding_sets(response: &Value) -> Vec<Value> {
    let users = response
        .pointer("/data/usersPaginated/nodes")
        .and_then(Value::as_array)
        .expect("usersPaginated.nodes should be an array");
    let mut tuples = Vec::new();

    for (anchor_index, user) in users.iter().enumerate() {
        let Some(profile) = user
            .get("footballUserProfile")
            .filter(|value| !value.is_null())
        else {
            continue;
        };
        let profile_id = profile
            .get("id")
            .and_then(Value::as_str)
            .expect("profile id should be a string");
        let decks = profile
            .pointer("/decks/nodes")
            .and_then(Value::as_array)
            .expect("profile decks should be an array");

        for (dependent_index, deck) in decks.iter().enumerate() {
            let slug = deck
                .get("slug")
                .and_then(Value::as_str)
                .expect("deck slug should be a string");
            tuples.push(json!({
                "bindings": {
                    "arg:Query.node.id": profile_id,
                    "arg:FootballUserSportProfile.deck.slug": slug
                },
                "provenance": {
                    "anchor_path": format!(
                        "data.usersPaginated.nodes[{anchor_index}].footballUserProfile"
                    ),
                    "anchor_index": anchor_index,
                    "dependent_index": dependent_index
                }
            }));
        }
    }

    tuples
}

fn contains_pair(binding_sets: &[Value], profile_id: &str, deck_slug: &str) -> bool {
    binding_sets.iter().any(|binding_set| {
        binding_set.pointer("/bindings/arg:Query.node.id") == Some(&json!(profile_id))
            && binding_set.pointer("/bindings/arg:FootballUserSportProfile.deck.slug")
                == Some(&json!(deck_slug))
    })
}
