//! Small API clients for the supported apps.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::docker::Entry;
use crate::error::{AppError, Result};
use crate::integrations::{AppStatus, Creds, HealthIssue};

fn api_versions(entry: &Entry) -> &'static [&'static str] {
    match entry.summary.app.as_deref() {
        Some("lidarr" | "readarr" | "prowlarr") => &["v1"],
        _ => &["v3", "v5"],
    }
}

async fn get_json(http: &reqwest::Client, url: &str, headers: &[(&str, &str)]) -> Result<Value> {
    let mut req = http.get(url).header("Accept", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::upstream("the app rejected the API key"));
    }
    if !status.is_success() {
        return Err(AppError::upstream(format!("HTTP {status}")));
    }
    Ok(resp.json().await?)
}

fn key(creds: &Creds) -> Result<&str> {
    creds.api_key.as_deref().ok_or_else(|| AppError::conflict("no API key found; add one in the container's settings"))
}

fn level_of(t: &str) -> String {
    match t.to_ascii_lowercase().as_str() {
        "error" => "error".into(),
        "warning" => "warning".into(),
        _ => "notice".into(),
    }
}

pub async fn status(
    http: &reqwest::Client,
    kind: &str,
    entry: &Entry,
    creds: &Creds,
    st: &mut AppStatus,
) -> Result<()> {
    let base = creds.base_url.trim_end_matches('/');
    match kind {
        "arr" => {
            let k = key(creds)?;
            let mut last_err = None;
            for v in api_versions(entry) {
                let url = format!("{base}/api/{v}/system/status");
                match get_json(http, &url, &[("X-Api-Key", k)]).await {
                    Ok(s) => {
                        st.reachable = true;
                        st.version = s["version"].as_str().map(str::to_string);
                        st.branch = s["branch"].as_str().map(str::to_string);
                        if let Ok(Value::Array(items)) =
                            get_json(http, &format!("{base}/api/{v}/health"), &[("X-Api-Key", k)]).await
                        {
                            st.health = items
                                .iter()
                                .map(|h| HealthIssue {
                                    level: level_of(h["type"].as_str().unwrap_or("notice")),
                                    message: h["message"].as_str().unwrap_or_default().to_string(),
                                    source: h["source"].as_str().map(str::to_string),
                                    wiki: h["wikiUrl"].as_str().map(str::to_string),
                                })
                                .collect();
                        }
                        if entry.summary.app.as_deref() != Some("prowlarr") {
                            if let Ok(q) =
                                get_json(http, &format!("{base}/api/{v}/queue/status"), &[("X-Api-Key", k)]).await
                            {
                                if let Some(n) = q["totalCount"].as_i64() {
                                    st.details.insert("Queue".into(), n.to_string());
                                }
                            }
                        } else if let Ok(Value::Array(ix)) =
                            get_json(http, &format!("{base}/api/{v}/indexerstatus"), &[("X-Api-Key", k)]).await
                        {
                            st.details.insert("Failing indexers".into(), ix.len().to_string());
                        }
                        return Ok(());
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            Err(last_err.unwrap_or_else(|| AppError::upstream("no API version answered")))
        }
        "bazarr" => {
            let k = key(creds)?;
            let s = get_json(http, &format!("{base}/api/system/status"), &[("X-API-KEY", k)]).await?;
            st.reachable = true;
            st.version = s["data"]["bazarr_version"].as_str().map(str::to_string);
            if let Ok(h) = get_json(http, &format!("{base}/api/system/health"), &[("X-API-KEY", k)]).await {
                if let Some(items) = h["data"].as_array() {
                    st.health = items
                        .iter()
                        .map(|i| HealthIssue {
                            level: "warning".into(),
                            message: format!(
                                "{}: {}",
                                i["object"].as_str().unwrap_or(""),
                                i["issue"].as_str().unwrap_or("")
                            ),
                            source: None,
                            wiki: None,
                        })
                        .collect();
                }
            }
            Ok(())
        }
        "sabnzbd" => {
            let k = key(creds)?;
            let v = get_json(http, &format!("{base}/api?mode=version&output=json&apikey={k}"), &[]).await?;
            st.reachable = true;
            st.version = v["version"].as_str().map(str::to_string);
            if let Ok(q) = get_json(http, &format!("{base}/api?mode=queue&output=json&limit=1&apikey={k}"), &[]).await {
                let q = &q["queue"];
                if let Some(n) = q["noofslots_total"].as_i64().or_else(|| q["noofslots"].as_i64()) {
                    st.details.insert("Queue".into(), n.to_string());
                }
                if let Some(s) = q["status"].as_str() {
                    st.details.insert("Status".into(), s.to_string());
                }
                if let Some(s) = q["speed"].as_str().filter(|s| !s.trim().is_empty() && *s != "0 ") {
                    st.details.insert("Speed".into(), format!("{}/s", s.trim()));
                }
            }
            if let Ok(w) = get_json(http, &format!("{base}/api?mode=warnings&output=json&apikey={k}"), &[]).await {
                if let Some(items) = w["warnings"].as_array() {
                    st.health = items
                        .iter()
                        .rev()
                        .take(5)
                        .map(|i| HealthIssue {
                            level: if i["type"].as_str() == Some("ERROR") { "error".into() } else { "warning".into() },
                            message: i["text"].as_str().or(i.as_str()).unwrap_or_default().to_string(),
                            source: None,
                            wiki: None,
                        })
                        .collect();
                }
            }
            Ok(())
        }
        "tautulli" => {
            let k = key(creds)?;
            let info = get_json(http, &format!("{base}/api/v2?apikey={k}&cmd=get_tautulli_info"), &[]).await?;
            st.reachable = true;
            st.version = info["response"]["data"]["tautulli_version"].as_str().map(str::to_string);
            if let Ok(a) = get_json(http, &format!("{base}/api/v2?apikey={k}&cmd=get_activity"), &[]).await {
                if let Some(n) = a["response"]["data"]["stream_count"]
                    .as_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .or(a["response"]["data"]["stream_count"].as_u64().map(|n| n as u32))
                {
                    st.details.insert("Streams".into(), n.to_string());
                }
            }
            Ok(())
        }
        "seerr" => {
            let mut headers = Vec::new();
            if let Some(k) = creds.api_key.as_deref() {
                headers.push(("X-Api-Key", k));
            }
            let s = get_json(http, &format!("{base}/api/v1/status"), &headers).await?;
            st.reachable = true;
            st.version = s["version"].as_str().map(str::to_string);
            st.update_available = s["updateAvailable"].as_bool();
            if let Some(n) = s["commitsBehind"].as_i64().filter(|n| *n > 0) {
                st.details.insert("Commits behind".into(), n.to_string());
            }
            if let Ok(c) = get_json(http, &format!("{base}/api/v1/request/count"), &headers).await {
                if let Some(n) = c["pending"].as_i64() {
                    st.details.insert("Pending requests".into(), n.to_string());
                }
            }
            Ok(())
        }
        "plex" => {
            let token = creds.api_key.as_deref().unwrap_or("");
            let id = get_json(http, &format!("{base}/identity"), &[("X-Plex-Token", token)]).await?;
            st.reachable = true;
            st.version = id["MediaContainer"]["version"].as_str().map(str::to_string);
            if !token.is_empty() {
                if let Ok(n) = plex_streams(http, base, token).await {
                    st.details.insert("Streams".into(), n.to_string());
                }
            } else {
                st.details.insert("Note".into(), "no Plex token found".into());
            }
            Ok(())
        }
        "qbittorrent" => {
            let client = qbit_login(http, base, creds).await?;
            let version = client.get(format!("{base}/api/v2/app/version")).send().await?;
            if !version.status().is_success() {
                return Err(AppError::upstream(format!(
                    "HTTP {} (set qBittorrent credentials in Settings > Integrations)",
                    version.status()
                )));
            }
            st.reachable = true;
            st.version = Some(version.text().await?.trim().trim_start_matches('v').to_string());
            if let Ok(info) = client.get(format!("{base}/api/v2/transfer/info")).send().await {
                if let Ok(v) = info.json::<Value>().await {
                    if let Some(dl) = v["dl_info_speed"].as_u64() {
                        st.details.insert("Download".into(), format!("{}/s", crate::util::human_bytes(dl)));
                    }
                    if let Some(ul) = v["up_info_speed"].as_u64() {
                        st.details.insert("Upload".into(), format!("{}/s", crate::util::human_bytes(ul)));
                    }
                }
            }
            Ok(())
        }
        "tdarr" => {
            let s = get_json(http, &format!("{base}/api/v2/status"), &[]).await?;
            st.reachable = true;
            st.version = s["version"].as_str().map(str::to_string);
            if let Ok(nodes) = get_json(http, &format!("{base}/api/v2/get-nodes"), &[]).await {
                if let Some(obj) = nodes.as_object() {
                    st.details.insert("Nodes".into(), obj.len().to_string());
                }
            }
            Ok(())
        }
        "immich" => {
            let v = get_json(http, &format!("{base}/api/server/version"), &[]).await?;
            st.reachable = true;
            if let (Some(a), Some(b), Some(c)) = (v["major"].as_u64(), v["minor"].as_u64(), v["patch"].as_u64()) {
                st.version = Some(format!("{a}.{b}.{c}"));
            }
            if let Some(k) = creds.api_key.as_deref() {
                if let Ok(stats) = get_json(http, &format!("{base}/api/server/statistics"), &[("x-api-key", k)]).await {
                    if let Some(p) = stats["photos"].as_u64() {
                        st.details.insert("Photos".into(), p.to_string());
                    }
                    if let Some(p) = stats["videos"].as_u64() {
                        st.details.insert("Videos".into(), p.to_string());
                    }
                }
            }
            Ok(())
        }
        "homeassistant" => {
            match creds.api_key.as_deref() {
                Some(token) => {
                    let auth = format!("Bearer {token}");
                    let c = get_json(http, &format!("{base}/api/config"), &[("Authorization", &auth)]).await?;
                    st.reachable = true;
                    st.version = c["version"].as_str().map(str::to_string);
                    if let Some(state) = c["state"].as_str() {
                        st.details.insert("State".into(), state.to_string());
                    }
                }
                None => {
                    let resp = http.get(format!("{base}/manifest.json")).send().await?;
                    st.reachable = resp.status().is_success();
                    st.details.insert("Note".into(), "add a long-lived token for version and state".into());
                }
            }
            Ok(())
        }
        other => Err(AppError::conflict(format!("unsupported integration {other}"))),
    }
}

async fn plex_streams(http: &reqwest::Client, base: &str, token: &str) -> Result<u32> {
    let s = get_json(http, &format!("{base}/status/sessions"), &[("X-Plex-Token", token)]).await?;
    Ok(s["MediaContainer"]["size"].as_u64().unwrap_or(0) as u32)
}

/// Active streams for the Plex guard.
pub async fn streams(http: &reqwest::Client, kind: &str, creds: &Creds) -> Result<u32> {
    let base = creds.base_url.trim_end_matches('/');
    match kind {
        "plex" => plex_streams(http, base, creds.api_key.as_deref().unwrap_or("")).await,
        "tautulli" => {
            let k = key(creds)?;
            let a = get_json(http, &format!("{base}/api/v2?apikey={k}&cmd=get_activity"), &[]).await?;
            let d = &a["response"]["data"]["stream_count"];
            Ok(d.as_str().and_then(|s| s.parse().ok()).or(d.as_u64().map(|n| n as u32)).unwrap_or(0))
        }
        _ => Err(AppError::conflict("not a streaming integration")),
    }
}

/// Logs into qBittorrent when credentials are configured; returns a client
/// carrying the session cookie.
async fn qbit_login(http: &reqwest::Client, base: &str, creds: &Creds) -> Result<reqwest::Client> {
    let (Some(user), Some(pass)) = (&creds.username, &creds.password) else {
        return Ok(http.clone());
    };
    let resp = http
        .post(format!("{base}/api/v2/auth/login"))
        .header("Referer", base)
        .form(&[("username", user.as_str()), ("password", pass.as_str())])
        .send()
        .await?;
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with("SID="))
        .map(|c| c.split(';').next().unwrap_or("").to_string())
        .ok_or_else(|| AppError::upstream("qBittorrent login failed (check the username and password)"))?;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_str(&cookie).map_err(|e| AppError::upstream(e.to_string()))?,
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(20))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(Into::into)
}

/// Runs Sonarr/Radarr/... "Backup" command and downloads the resulting zip.
pub async fn arr_backup(http: &reqwest::Client, entry: &Entry, creds: &Creds, dir: &Path) -> Result<PathBuf> {
    let base = creds.base_url.trim_end_matches('/');
    let k = key(creds)?;
    let v = api_versions(entry)[0];
    let resp = http
        .post(format!("{base}/api/{v}/command"))
        .header("X-Api-Key", k)
        .json(&serde_json::json!({ "name": "Backup" }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::upstream(format!("starting the backup failed: HTTP {}", resp.status())));
    }
    let cmd: Value = resp.json().await?;
    let id = cmd["id"].as_i64().ok_or_else(|| AppError::upstream("no command id returned"))?;
    let started = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let c = get_json(http, &format!("{base}/api/{v}/command/{id}"), &[("X-Api-Key", k)]).await?;
        match c["status"].as_str() {
            Some("completed") => break,
            Some("failed") | Some("aborted") | Some("cancelled") => {
                return Err(AppError::upstream(format!(
                    "the app's backup {}",
                    c["status"].as_str().unwrap_or("failed")
                )));
            }
            _ => {}
        }
        if started.elapsed() > Duration::from_secs(600) {
            return Err(AppError::upstream("the app's backup took too long"));
        }
    }
    let list = get_json(http, &format!("{base}/api/{v}/system/backup"), &[("X-Api-Key", k)]).await?;
    let newest = list
        .as_array()
        .and_then(|a| {
            a.iter()
                .filter(|b| b["type"].as_str() == Some("manual"))
                .max_by_key(|b| b["time"].as_str().unwrap_or("").to_string())
        })
        .ok_or_else(|| AppError::upstream("the app reported no manual backup"))?;
    let path = newest["path"].as_str().ok_or_else(|| AppError::upstream("backup has no path"))?;
    let name = newest["name"].as_str().unwrap_or("backup.zip");
    let resp = http.get(format!("{base}{path}")).header("X-Api-Key", k).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::upstream(format!("downloading the backup failed: HTTP {}", resp.status())));
    }
    let bytes = resp.bytes().await?;
    let dest = dir.join(crate::util::slug(name.trim_end_matches(".zip")) + ".zip");
    tokio::fs::write(&dest, &bytes).await?;
    Ok(dest)
}
