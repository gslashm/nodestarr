//! Delivery to each notification service.

use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::settings::{NotificationProvider, ProviderConfig, Settings, Severity};

fn color(s: Severity) -> u32 {
    match s {
        Severity::Info => 0x3b82f6,
        Severity::Success => 0x22c55e,
        Severity::Warning => 0xf59e0b,
        Severity::Error => 0xef4444,
    }
}

fn emoji(s: Severity) -> &'static str {
    match s {
        Severity::Info => "ℹ️",
        Severity::Success => "✅",
        Severity::Warning => "⚠️",
        Severity::Error => "🚨",
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Plain-text body: message plus fields.
fn plain_body(n: &Notification) -> String {
    let mut out = n.message.clone();
    for (k, v) in &n.fields {
        out.push_str(&format!("\n{k}: {v}"));
    }
    if let Some(u) = &n.url {
        out.push_str(&format!("\n{u}"));
    }
    out
}

async fn check(resp: reqwest::Response, service: &str) -> Result<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(AppError::upstream(format!("{service} returned HTTP {status}: {}", crate::util::truncate(body.trim(), 300))))
}

pub async fn deliver(
    http: &reqwest::Client,
    settings: &Settings,
    p: &NotificationProvider,
    n: &Notification,
) -> Result<()> {
    let instance = settings.general.instance_name.clone();
    let timeout = Duration::from_secs(20);
    match &p.config {
        ProviderConfig::Discord { webhook_url, username, mention } => {
            let fields: Vec<Value> = n
                .fields
                .iter()
                .map(|(k, v)| json!({ "name": k, "value": crate::util::truncate(v, 1000), "inline": true }))
                .collect();
            let mut embed = json!({
                "title": crate::util::truncate(&n.title, 250),
                "description": crate::util::truncate(&n.message, 4000),
                "color": color(n.severity),
                "fields": fields,
                "timestamp": chrono::DateTime::from_timestamp(n.ts, 0).map(|d| d.to_rfc3339()),
                "footer": { "text": instance },
            });
            if let Some(u) = &n.url {
                embed["url"] = json!(u);
            }
            let mut body =
                json!({ "embeds": [embed], "allowed_mentions": { "parse": ["users", "roles", "everyone"] } });
            if !username.is_empty() {
                body["username"] = json!(username);
            }
            if !mention.is_empty() && n.severity >= Severity::Warning {
                body["content"] = json!(mention);
            }
            let resp = http.post(webhook_url).json(&body).timeout(timeout).send().await?;
            check(resp, "Discord").await
        }
        ProviderConfig::Slack { webhook_url } => {
            let mut text = format!("{} *{}*\n{}", emoji(n.severity), n.title, n.message);
            for (k, v) in &n.fields {
                text.push_str(&format!("\n*{k}:* {v}"));
            }
            if let Some(u) = &n.url {
                text.push_str(&format!("\n<{u}|Open in {instance}>"));
            }
            let resp = http.post(webhook_url).json(&json!({ "text": text })).timeout(timeout).send().await?;
            check(resp, "Slack").await
        }
        ProviderConfig::Telegram { bot_token, chat_id, topic_id } => {
            let mut text =
                format!("{} <b>{}</b>\n{}", emoji(n.severity), html_escape(&n.title), html_escape(&n.message));
            for (k, v) in &n.fields {
                text.push_str(&format!("\n<b>{}:</b> {}", html_escape(k), html_escape(v)));
            }
            if let Some(u) = &n.url {
                text.push_str(&format!("\n<a href=\"{}\">Open in {}</a>", html_escape(u), html_escape(&instance)));
            }
            let mut body = json!({
                "chat_id": chat_id,
                "text": crate::util::truncate(&text, 4000),
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            });
            if let Ok(t) = topic_id.parse::<i64>() {
                body["message_thread_id"] = json!(t);
            }
            let resp = http
                .post(format!("https://api.telegram.org/bot{bot_token}/sendMessage"))
                .json(&body)
                .timeout(timeout)
                .send()
                .await?;
            check(resp, "Telegram").await
        }
        ProviderConfig::Ntfy { server, topic, token, username, password } => {
            let (priority, tags) = match n.severity {
                Severity::Error => ("5", "rotating_light"),
                Severity::Warning => ("4", "warning"),
                Severity::Success => ("3", "white_check_mark"),
                Severity::Info => ("3", "information_source"),
            };
            let mut req = http
                .post(format!("{}/{}", server.trim_end_matches('/'), topic))
                .header("Title", header_safe(&n.title))
                .header("Priority", priority)
                .header("Tags", tags)
                .body(plain_body(n))
                .timeout(timeout);
            if let Some(u) = &n.url {
                req = req.header("Click", u.as_str());
            }
            if !token.is_empty() {
                req = req.bearer_auth(token);
            } else if !username.is_empty() {
                req = req.basic_auth(username, Some(password));
            }
            check(req.send().await?, "ntfy").await
        }
        ProviderConfig::Gotify { server, app_token } => {
            let priority = match n.severity {
                Severity::Error => 8,
                Severity::Warning => 5,
                _ => 3,
            };
            let mut extras = json!({ "client::display": { "contentType": "text/markdown" } });
            if let Some(u) = &n.url {
                extras["client::notification"] = json!({ "click": { "url": u } });
            }
            let mut message = n.message.clone();
            for (k, v) in &n.fields {
                message.push_str(&format!("\n\n**{k}:** {v}"));
            }
            let resp = http
                .post(format!("{}/message", server.trim_end_matches('/')))
                .header("X-Gotify-Key", app_token)
                .json(&json!({ "title": n.title, "message": message, "priority": priority, "extras": extras }))
                .timeout(timeout)
                .send()
                .await?;
            check(resp, "Gotify").await
        }
        ProviderConfig::Pushover { user_key, app_token, device } => {
            let priority = if n.severity == Severity::Error { "1" } else { "0" };
            let mut form = vec![
                ("token", app_token.clone()),
                ("user", user_key.clone()),
                ("title", n.title.clone()),
                ("message", crate::util::truncate(&plain_body(n), 1000)),
                ("priority", priority.to_string()),
            ];
            if !device.is_empty() {
                form.push(("device", device.clone()));
            }
            if let Some(u) = &n.url {
                form.push(("url", u.clone()));
            }
            let resp =
                http.post("https://api.pushover.net/1/messages.json").form(&form).timeout(timeout).send().await?;
            check(resp, "Pushover").await
        }
        ProviderConfig::Webhook { url, method, headers, body_template } => {
            let payload = json!({
                "kind": n.kind,
                "severity": n.severity,
                "title": n.title,
                "message": n.message,
                "fields": n.fields.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<String, Value>>(),
                "target": n.target,
                "target_name": n.target_name,
                "url": n.url,
                "timestamp": n.ts,
                "instance": instance,
            });
            let body = if body_template.trim().is_empty() {
                serde_json::to_string(&payload)?
            } else {
                let mut env = minijinja::Environment::new();
                env.add_template("body", body_template)
                    .map_err(|e| AppError::bad_request(format!("template error: {e}")))?;
                env.get_template("body")
                    .and_then(|t| t.render(minijinja::value::Serde(&payload)))
                    .map_err(|e| AppError::bad_request(format!("template error: {e}")))?
            };
            let method =
                reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes()).unwrap_or(reqwest::Method::POST);
            let mut req = http.request(method, url).timeout(timeout);
            let has_ct = headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if !has_ct {
                req = req.header("Content-Type", "application/json");
            }
            check(req.body(body).send().await?, "Webhook").await
        }
        ProviderConfig::Email { host, port, security, username, password, from, to } => {
            send_email(host, *port, security, username, password, from, to, n, &instance).await
        }
    }
}

fn header_safe(s: &str) -> String {
    // HTTP headers must be visible ASCII; ntfy also accepts RFC 2047, but
    // replacing is simpler and robust.
    s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '?' }).collect()
}

#[allow(clippy::too_many_arguments)]
async fn send_email(
    host: &str,
    port: u16,
    security: &str,
    username: &str,
    password: &str,
    from: &str,
    to: &[String],
    n: &Notification,
    instance: &str,
) -> Result<()> {
    use lettre::message::{Mailbox, MultiPart, header::ContentType};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let from: Mailbox = from.parse().map_err(|e| AppError::bad_request(format!("invalid from address: {e}")))?;
    let mut builder = Message::builder().from(from).subject(format!("[{instance}] {}", n.title));
    for addr in to {
        let mb: Mailbox = addr.parse().map_err(|e| AppError::bad_request(format!("invalid recipient {addr}: {e}")))?;
        builder = builder.to(mb);
    }
    let text = plain_body(n);
    let mut rows = String::new();
    for (k, v) in &n.fields {
        rows.push_str(&format!(
            "<tr><td style=\"padding:4px 12px 4px 0;color:#64748b\">{}</td><td style=\"padding:4px 0\">{}</td></tr>",
            html_escape(k),
            html_escape(v)
        ));
    }
    let link = n
        .url
        .as_ref()
        .map(|u| {
            format!(
                "<p><a href=\"{}\" style=\"color:#2fb6a6\">Open in {}</a></p>",
                html_escape(u),
                html_escape(instance)
            )
        })
        .unwrap_or_default();
    let html = format!(
        "<div style=\"font-family:system-ui,sans-serif;max-width:560px\">\
         <div style=\"border-left:4px solid #{:06x};padding:8px 16px\">\
         <h2 style=\"margin:0 0 8px;font-size:18px\">{}</h2>\
         <p style=\"white-space:pre-line;margin:0 0 12px\">{}</p>\
         <table style=\"font-size:14px\">{rows}</table>{link}</div>\
         <p style=\"color:#94a3b8;font-size:12px\">Sent by {}</p></div>",
        color(n.severity),
        html_escape(&n.title),
        html_escape(&n.message),
        html_escape(instance)
    );
    let email = builder
        .multipart(MultiPart::alternative_plain_html(text, html))
        .map_err(|e| AppError::bad_request(format!("could not build email: {e}")))?;
    let _ = ContentType::TEXT_PLAIN;

    let transport = match security {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
        _ => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)),
    }
    .map_err(|e| AppError::upstream(format!("SMTP setup failed: {e}")))?;
    let mut transport = transport.port(port).timeout(Some(Duration::from_secs(20)));
    if !username.is_empty() {
        transport = transport.credentials(Credentials::new(username.to_string(), password.to_string()));
    }
    transport.build().send(email).await.map_err(|e| AppError::upstream(format!("SMTP: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        let n = Notification::new("update.applied", Severity::Success, "Sonarr updated", "4.0.9 → 4.0.10")
            .field("Image", "lscr.io/linuxserver/sonarr:latest");
        let b = plain_body(&n);
        assert!(b.contains("Image: lscr.io"));
        assert_eq!(html_escape("<a&b>"), "&lt;a&amp;b&gt;");
        assert_eq!(header_safe("Sonarr → ok"), "Sonarr ? ok");
    }

    #[tokio::test]
    async fn webhook_template_renders() {
        // Render the template path without sending: emulate with a bad URL and
        // check the error is a network error, not a template error.
        crate::app::init_tls();
        let http = reqwest::Client::new();
        let p = NotificationProvider {
            id: "w".into(),
            name: "W".into(),
            enabled: true,
            events: vec![],
            min_severity: Severity::Info,
            config: ProviderConfig::Webhook {
                url: "http://127.0.0.1:9/hook".into(),
                method: "POST".into(),
                headers: Default::default(),
                body_template: "{\"text\": {{ title | tojson }} }".into(),
            },
        };
        let n = Notification::new("test", Severity::Info, "Hello \"world\"", "m");
        let err = deliver(&http, &Settings::default(), &p, &n).await.unwrap_err();
        assert!(!err.to_string().contains("template"), "{err}");
    }
}
