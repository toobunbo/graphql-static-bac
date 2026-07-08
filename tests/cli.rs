use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn stage_s0_cli_writes_a_valid_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("schema_ir.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s0",
            "--input",
            "tests/fixtures/s0/sdl/minimal.graphql",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let artifact: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(artifact["stage"], "S0");
    assert_eq!(artifact["status"], "complete");
    assert_eq!(artifact["data"]["roots"]["query"], "Query");
}

#[test]
fn stage_s0_cli_uses_parse_exit_code() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("schema_ir.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s0",
            "--input",
            "tests/fixtures/s0/sdl/invalid.graphql",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(4));
    assert!(!output.exists());
}

#[test]
fn stage_s2_cli_writes_deterministic_recursive_classifications() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("args.first.json");
    let second = directory.path().join("args.second.json");
    for output in [&first, &second] {
        let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
            .args([
                "stage",
                "s2",
                "--schema-ir",
                "tests/fixtures/s0/expected/minimal-sdl.schema_ir.json",
                "--policy",
                "config/lexicons/argument-classifier-v1.json",
                "--output",
            ])
            .arg(output)
            .status()
            .unwrap();
        assert!(status.success());
    }

    let first_bytes = fs::read(&first).unwrap();
    assert_eq!(first_bytes, fs::read(&second).unwrap());
    let artifact: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_eq!(artifact["stage"], "S2");
    assert_eq!(
        artifact["data"]["classifier_model"],
        "argument-classifier-v1"
    );
    let arguments = artifact["data"]["fields"]["field:Query.search"]["arguments"]
        .as_array()
        .unwrap();
    assert!(arguments.iter().any(|argument| {
        argument["arg_path"] == "Query.search.filter.ownerIds"
            && argument["classifications"] == serde_json::json!(["object_selector"])
    }));
    assert!(arguments.iter().any(|argument| {
        argument["arg_path"] == "Query.search.filter.limit"
            && argument["classifications"] == serde_json::json!(["noise"])
    }));
    assert!(arguments.iter().any(|argument| {
        argument["arg_path"] == "Query.search.filter.role"
            && argument["classifications"] == serde_json::json!(["possible_selector"])
    }));
}

#[test]
fn enumerate_cli_writes_single_target_report() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("paths.user-device.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "enumerate",
            "--schema-ir",
            "tests/fixtures/user_device/s0_schema_ir.json",
            "--target",
            "UserDevice",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let artifact: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(artifact["stage"], "S3");
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["caps"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["sink_ref_ids"],
        serde_json::json!([])
    );
}

#[test]
fn stage_s3_cli_joins_s1_sink_refs() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("routes.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s3",
            "--schema-ir",
            "tests/fixtures/user_device/s0_schema_ir.json",
            "--sinks",
            "tests/fixtures/user_device/s1_sinks.json",
            "--args",
            "tests/fixtures/user_device/s2_args.json",
            "--policy",
            "config/profiles/route-analysis-v1.json",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let artifact: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(artifact["contract_version"], "2.2");
    assert_eq!(artifact["status"], "complete");
    assert_eq!(
        artifact["data"]["coverage"],
        "canonical_per_provenance"
    );
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["best_verdict"],
        "open"
    );
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["routes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["sink_ref_ids"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn stage_s3_cli_rejects_wrong_stage_input() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("routes.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s3",
            "--schema-ir",
            "tests/fixtures/user_device/s1_sinks.json",
            "--sinks",
            "tests/fixtures/user_device/s1_sinks.json",
            "--args",
            "tests/fixtures/user_device/s2_args.json",
            "--policy",
            "config/profiles/route-analysis-v1.json",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(4));
    assert!(!Path::new(&output).exists());
}

#[test]
fn route_cli_writes_a_single_target_v2_report() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("route.user-device.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "route",
            "--schema-ir",
            "tests/fixtures/user_device/s0_schema_ir.json",
            "--args",
            "tests/fixtures/user_device/s2_args.json",
            "--policy",
            "config/profiles/route-analysis-v1.json",
            "--target",
            "UserDevice",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let artifact: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(artifact["contract_version"], "2.2");
    assert_eq!(
        artifact["data"]["targets"]["type:UserDevice"]["routes"][0]["origin"],
        "global_id"
    );
}

#[test]
fn stage_s4_cli_writes_deterministic_seed_plans() {
    let directory = tempfile::tempdir().unwrap();
    let routes = directory.path().join("routes.json");
    let route_status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s3",
            "--schema-ir",
            "tests/fixtures/user_device/s0_schema_ir.json",
            "--sinks",
            "tests/fixtures/user_device/s1_sinks.json",
            "--args",
            "tests/fixtures/user_device/s2_args.json",
            "--policy",
            "config/profiles/route-analysis-v1.json",
            "--output",
        ])
        .arg(&routes)
        .status()
        .unwrap();
    assert!(route_status.success());

    let first = directory.path().join("seed-plans.first.json");
    let second = directory.path().join("seed-plans.second.json");
    for output in [&first, &second] {
        let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
            .args([
                "stage",
                "s4",
                "--schema-ir",
                "tests/fixtures/user_device/s0_schema_ir.json",
                "--args",
                "tests/fixtures/user_device/s2_args.json",
                "--routes",
            ])
            .arg(&routes)
            .arg("--output")
            .arg(output)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let first_bytes = fs::read(&first).unwrap();
    assert_eq!(first_bytes, fs::read(&second).unwrap());
    let artifact: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_eq!(artifact["stage"], "S4");
    assert_eq!(artifact["contract_version"], "1.0");
    assert_eq!(artifact["data"]["planning_model"], "seed-planning-v1");
    assert_eq!(
        artifact["data"]["routes"]
            .as_object()
            .unwrap()
            .values()
            .filter(|route| route["binding_set_plans"][0]["status"] == "executable")
            .count(),
        4
    );
}

#[test]
fn stage_s4_cli_rejects_legacy_s3_contract() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("seed-plans.json");
    let status = Command::new(env!("CARGO_BIN_EXE_graphql-static-bac"))
        .args([
            "stage",
            "s4",
            "--schema-ir",
            "tests/fixtures/user_device/s0_schema_ir.json",
            "--args",
            "tests/fixtures/user_device/s2_args.json",
            "--routes",
            "tests/fixtures/user_device/s3_paths.json",
            "--output",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(4));
    assert!(!output.exists());
}
