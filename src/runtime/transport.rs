use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct GraphqlHttpRequest {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphqlHttpResponse {
    pub status_code: u16,
    pub body: Value,
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("could not serialize request body: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not start curl: {0}")]
    Spawn(std::io::Error),
    #[error("could not write curl request body: {0}")]
    Write(std::io::Error),
    #[error("curl failed: {0}")]
    Curl(String),
    #[error("curl response did not contain an HTTP status")]
    MissingStatus,
    #[error("response body is not JSON: {0}")]
    InvalidJson(serde_json::Error),
}

pub trait GraphqlTransport {
    fn execute(&self, request: &GraphqlHttpRequest) -> Result<GraphqlHttpResponse, TransportError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CurlTransport;

impl GraphqlTransport for CurlTransport {
    fn execute(&self, request: &GraphqlHttpRequest) -> Result<GraphqlHttpResponse, TransportError> {
        let timeout_seconds = (request.timeout_ms.max(1) as f64 / 1000.0).to_string();
        let mut command = Command::new("curl");
        command
            .args(["-sS", "-X", &request.method, "--max-time", &timeout_seconds])
            .arg(&request.url);
        for (name, value) in &request.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command
            .args(["--data-binary", "@-", "-w", "\n%{http_code}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(TransportError::Spawn)?;
        child
            .stdin
            .take()
            .expect("piped curl stdin exists")
            .write_all(&serde_json::to_vec(&request.body)?)
            .map_err(TransportError::Write)?;
        let output = child.wait_with_output().map_err(TransportError::Spawn)?;
        if !output.status.success() {
            return Err(TransportError::Curl(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let output = String::from_utf8_lossy(&output.stdout);
        let (body, status) = output
            .rsplit_once('\n')
            .ok_or(TransportError::MissingStatus)?;
        let status_code = status
            .trim()
            .parse::<u16>()
            .map_err(|_| TransportError::MissingStatus)?;
        let body = serde_json::from_str(body).map_err(TransportError::InvalidJson)?;
        Ok(GraphqlHttpResponse { status_code, body })
    }
}
