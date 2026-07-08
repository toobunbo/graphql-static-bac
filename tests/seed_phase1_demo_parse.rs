use graphql_parser::parse_query;

#[test]
fn phase1_seed_demo_queries_parse() {
    let query = include_str!("../output/seed-finder.phase1-demo.graphql");
    let document = parse_query::<String>(query).expect("phase 1 demo query should parse");
    assert_eq!(document.definitions.len(), 2);
}
