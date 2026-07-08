use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterCandidate {
    pub value: Value,
    pub chain: Vec<String>,
}

pub fn adapter_candidates(value: &Value, consumer_is_list: bool) -> Vec<AdapterCandidate> {
    let mut output = vec![AdapterCandidate {
        value: value.clone(),
        chain: vec!["identity".to_string()],
    }];
    if consumer_is_list && !value.is_array() {
        output.push(AdapterCandidate {
            value: Value::Array(vec![value.clone()]),
            chain: vec!["scalar_to_singleton_list".to_string()],
        });
    }
    if !consumer_is_list {
        if let Some(values) = value.as_array() {
            for value in values {
                output.push(AdapterCandidate {
                    value: value.clone(),
                    chain: vec!["list_to_scalar_candidates".to_string()],
                });
            }
        }
    }
    let existing = output.clone();
    for candidate in existing {
        if let Some(value) = strip_global_id(&candidate.value) {
            let mut chain = candidate.chain;
            chain.push("global_id_to_payload".to_string());
            output.push(AdapterCandidate { value, chain });
        }
    }
    output.dedup_by(|left, right| left.value == right.value && left.chain == right.chain);
    output
}

fn strip_global_id(value: &Value) -> Option<Value> {
    match value {
        Value::String(value) => {
            let (prefix, payload) = value.split_once(':')?;
            if prefix.is_empty() || payload.is_empty() || payload.contains(':') {
                return None;
            }
            Some(Value::String(payload.to_string()))
        }
        Value::Array(values) => {
            let converted: Option<Vec<_>> = values.iter().map(strip_global_id).collect();
            converted.map(Value::Array)
        }
        _ => None,
    }
}
