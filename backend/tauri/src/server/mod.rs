pub(crate) use crate::utils::candy::get_reqwest_client;
use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    extract::Query,
    http::{Response, StatusCode},
    routing::get,
    Router,
};
use base64::{prelude::BASE64_STANDARD, Engine};
use bytes::Bytes;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use tracing_attributes::instrument;
use url::Url;
pub static SERVER_PORT: Lazy<u16> = Lazy::new(|| port_scanner::request_open_port().unwrap());

#[derive(Deserialize)]
struct CacheIcon {
    /// should be encoded as base64
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheFile<'n> {
    mime: Cow<'n, str>,
    bytes: Bytes,
}

async fn cache_icon_inner<'n>(url: &str) -> Result<CacheFile<'n>> {
    let url = BASE64_STANDARD.decode(url)?;
    let url = String::from_utf8_lossy(&url);
    let url = Url::parse(&url)?;
    // get filename
    let hash = Sha256::digest(url.as_str().as_bytes());
    let cache_dir = crate::utils::dirs::cache_dir()?.join("icons");
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir)?;
    }
    let cache_file = cache_dir.join(format!("{:x}.bin", hash));
    if cache_file.exists() {
        let cache_file = tokio::fs::read(cache_file).await?;
        let cache_file: CacheFile = bincode::deserialize(&cache_file)?;
        return Ok(cache_file);
    }
    let client = get_reqwest_client()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let mime = response
        .headers()
        .get("content-type")
        .ok_or(anyhow!("no content-type"))?
        .to_str()?
        .to_string();

    let bytes = response.bytes().await?;
    let data = CacheFile {
        mime: Cow::Owned(mime),
        bytes,
    };
    tokio::fs::write(cache_file, bincode::serialize(&data)?).await?;
    Ok(data)
}

async fn cache_icon(query: Query<CacheIcon>) -> Response<Body> {
    match cache_icon_inner(&query.url).await {
        Ok(data) => {
            let mut response = Response::new(Body::from(data.bytes));
            response
                .headers_mut()
                .insert("content-type", data.mime.parse().unwrap());
            response
        }
        Err(e) => {
            let mut response = Response::new(Body::from(e.to_string()));
            *response.status_mut() = StatusCode::BAD_REQUEST;
            response
        }
    }
}

#[instrument]
pub async fn run(port: u16) -> std::io::Result<()> {
    let app = Router::new().route("/cache/icon", get(cache_icon));
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    tracing::debug!(
        "internal http server listening on {}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await
}
