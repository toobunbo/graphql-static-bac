use serde_json::Value;
use thiserror::Error;

use super::{GraphqlHttpRequest, RuntimeRequestProfile};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RequestAdapterError {
    #[error("request body must be an object or array")]
    InvalidBody,
    #[error("JSON Pointer {0} does not resolve inside the request body")]
    MissingPointer(String),
}

pub fn build_request(
    profile: &RuntimeRequestProfile,
    operation_name: &str,
    query: &str,
    variables: Value,
) -> Result<GraphqlHttpRequest, RequestAdapterError> {
    if !profile.request.body.is_object() && !profile.request.body.is_array() {
        return Err(RequestAdapterError::InvalidBody);
    }
    let mut body = profile.request.body.clone();
    set_pointer(
        &mut body,
        &profile.injection.operation_name_pointer,
        Value::String(operation_name.to_string()),
    )?;
    set_pointer(
        &mut body,
        &profile.injection.query_pointer,
        Value::String(query.to_string()),
    )?;
    set_pointer(&mut body, &profile.injection.variables_pointer, variables)?;
    Ok(GraphqlHttpRequest {
        url: profile.request.url.clone(),
        method: profile.request.method.clone(),
        headers: profile.request.headers.clone(),
        body,
        timeout_ms: profile.limits.timeout_ms,
    })
}

fn set_pointer(root: &mut Value, pointer: &str, value: Value) -> Result<(), RequestAdapterError> {
    let target = root
        .pointer_mut(pointer)
        .ok_or_else(|| RequestAdapterError::MissingPointer(pointer.to_string()))?;
    *target = value;
    Ok(())
}
