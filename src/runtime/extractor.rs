use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use crate::contracts::{ExtractionPlan, ExtractionProvenance};

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedValue {
    pub value: Value,
    pub provenance: ExtractionProvenance,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExtractionError {
    #[error("invalid extraction path {0}")]
    InvalidPath(String),
    #[error("extraction path {0} did not resolve")]
    PathMissing(String),
}

#[derive(Debug, Clone)]
struct Segment {
    name: String,
    list: bool,
}

pub fn extract_values(
    response: &Value,
    response_path: &str,
) -> Result<Vec<ExtractedValue>, ExtractionError> {
    let segments = parse_path(response_path)?;
    let mut output = Vec::new();
    walk(
        response,
        &segments,
        0,
        Vec::new(),
        Vec::new(),
        response_path,
        &mut output,
    );
    if output.is_empty() {
        return Err(ExtractionError::PathMissing(response_path.to_string()));
    }
    Ok(output)
}

pub fn extract_joint(
    response: &Value,
    plan: &ExtractionPlan,
) -> Result<Vec<BTreeMap<String, ExtractedValue>>, ExtractionError> {
    let Some(anchor) = &plan.anchor else {
        let mut rows = vec![BTreeMap::new()];
        for (requirement_id, member) in &plan.members {
            let values = extract_values(response, &member.response_path)?;
            let mut expanded = Vec::new();
            for row in rows {
                for value in &values {
                    let mut next = row.clone();
                    next.insert(requirement_id.clone(), value.clone());
                    expanded.push(next);
                }
            }
            rows = expanded;
        }
        return Ok(rows);
    };

    let anchors = extract_values(response, &anchor.response_path)?;
    let mut rows = Vec::new();
    for anchor_value in anchors {
        let mut anchor_rows = vec![BTreeMap::new()];
        for (requirement_id, member) in &plan.members {
            let values = extract_values(&anchor_value.value, &member.relative_path)?;
            let mut expanded = Vec::new();
            for row in anchor_rows {
                for value in &values {
                    let mut next = row.clone();
                    let mut value = value.clone();
                    let mut indices = anchor_value.provenance.list_indices.clone();
                    indices.extend(value.provenance.list_indices.clone());
                    value.provenance.list_indices = indices;
                    value.provenance.branch_path = format!(
                        "{}.{}",
                        anchor_value.provenance.branch_path, value.provenance.branch_path
                    );
                    next.insert(requirement_id.clone(), value);
                    expanded.push(next);
                }
            }
            anchor_rows = expanded;
        }
        rows.extend(anchor_rows);
    }
    Ok(rows)
}

fn parse_path(path: &str) -> Result<Vec<Segment>, ExtractionError> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    path.split('.')
        .map(|part| {
            let list = part.ends_with("[]");
            let name = part.strip_suffix("[]").unwrap_or(part);
            if name.is_empty() {
                return Err(ExtractionError::InvalidPath(path.to_string()));
            }
            Ok(Segment {
                name: name.to_string(),
                list,
            })
        })
        .collect()
}

fn walk(
    value: &Value,
    segments: &[Segment],
    position: usize,
    path: Vec<String>,
    indices: Vec<usize>,
    response_path: &str,
    output: &mut Vec<ExtractedValue>,
) {
    if position == segments.len() {
        if !value.is_null() {
            output.push(ExtractedValue {
                value: value.clone(),
                provenance: ExtractionProvenance {
                    response_path: response_path.to_string(),
                    branch_path: path.join("."),
                    list_indices: indices,
                },
            });
        }
        return;
    }
    let segment = &segments[position];
    let Some(child) = value.get(&segment.name) else {
        return;
    };
    let mut next_path = path;
    next_path.push(segment.name.clone());
    if segment.list {
        let Some(values) = child.as_array() else {
            return;
        };
        for (index, value) in values.iter().enumerate() {
            let mut next_indices = indices.clone();
            next_indices.push(index);
            walk(
                value,
                segments,
                position + 1,
                next_path.clone(),
                next_indices,
                response_path,
                output,
            );
        }
    } else {
        walk(
            child,
            segments,
            position + 1,
            next_path,
            indices,
            response_path,
            output,
        );
    }
}
