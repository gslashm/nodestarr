//! Minimal untyped access to the Docker API.
//!
//! bollard's typed models follow the newest API version. A few endpoints
//! changed their response shape between Docker releases (`/system/df` did in
//! API 1.52), so Nodestarr reads those as plain JSON and understands both shapes.

use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{AppError, Result};

/// Where the Docker API listens, parsed from `DOCKER_HOST`.
enum Endpoint<'a> {
    Unix(&'a str),
    Tcp(&'a str),
}

fn endpoint(host: &str) -> Result<Endpoint<'_>> {
    if let Some(path) = host.strip_prefix("unix://") {
        Ok(Endpoint::Unix(path))
    } else if host.starts_with('/') {
        Ok(Endpoint::Unix(host))
    } else if let Some(addr) = host.strip_prefix("tcp://").or_else(|| host.strip_prefix("http://")) {
        Ok(Endpoint::Tcp(addr.trim_end_matches('/')))
    } else {
        Err(AppError::Docker(format!("unsupported DOCKER_HOST '{host}'")))
    }
}

/// GETs `path` (for example "/system/df") and parses the JSON response.
pub async fn get_json(host: &str, path: &str, timeout: Duration) -> Result<serde_json::Value> {
    let fut = async {
        let (status, body) = match endpoint(host)? {
            Endpoint::Unix(p) => send(tokio::net::UnixStream::connect(p).await?, path).await?,
            Endpoint::Tcp(a) => send(tokio::net::TcpStream::connect(a).await?, path).await?,
        };
        if !status.is_success() {
            let msg = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(AppError::Docker(msg));
        }
        serde_json::from_slice(&body).map_err(|e| AppError::Docker(format!("unexpected response from Docker: {e}")))
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| AppError::Docker(format!("Docker took longer than {}s to answer {path}", timeout.as_secs())))?
}

async fn send<S>(stream: S, path: &str) -> Result<(http::StatusCode, Bytes)>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| AppError::Docker(e.to_string()))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = http::Request::get(path)
        .header(http::header::HOST, "docker")
        .header(http::header::USER_AGENT, crate::USER_AGENT)
        .body(Empty::<Bytes>::new())
        .map_err(|e| AppError::Docker(e.to_string()))?;
    let resp = sender.send_request(req).await.map_err(|e| AppError::Docker(e.to_string()))?;
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| AppError::Docker(e.to_string()))?
        .to_bytes();
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        assert!(matches!(endpoint("unix:///var/run/docker.sock"), Ok(Endpoint::Unix("/var/run/docker.sock"))));
        assert!(matches!(endpoint("/run/docker.sock"), Ok(Endpoint::Unix("/run/docker.sock"))));
        assert!(matches!(endpoint("tcp://10.0.0.2:2375"), Ok(Endpoint::Tcp("10.0.0.2:2375"))));
        assert!(endpoint("ssh://host").is_err());
    }
}
