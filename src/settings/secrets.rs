//! Keeping secrets out of the browser.
//!
//! The UI receives settings with every secret replaced by a placeholder. When
//! it saves, any field still holding the placeholder keeps the stored value,
//! so passwords and tokens never round-trip through the client.

use serde_json::Value;

/// Sent to the UI instead of a stored secret; sending it back means "unchanged".
pub const SECRET_PLACEHOLDER: &str = "__berth_secret__";

/// Field names whose values are secret wherever they appear.
const SECRET_KEYS: &[&str] = &[
    "password",
    "token_secret",
    "github_token",
    "bot_token",
    "webhook_url",
    "app_token",
    "user_key",
    "token",
    "qbittorrent_password",
    "api_key",
];

/// Header maps (generic webhooks) may carry credentials; mask all values.
const SECRET_MAPS: &[&str] = &["headers"];

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

/// Replaces non-empty secret values with the placeholder, in place.
pub fn mask_secrets(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if is_secret_key(key) {
                    if let Value::String(s) = val {
                        if !s.is_empty() {
                            *s = SECRET_PLACEHOLDER.to_string();
                        }
                    }
                } else if SECRET_MAPS.contains(&key.as_str()) {
                    if let Value::Object(headers) = val {
                        for hv in headers.values_mut() {
                            if let Value::String(s) = hv {
                                if !s.is_empty() {
                                    *s = SECRET_PLACEHOLDER.to_string();
                                }
                            }
                        }
                    }
                } else {
                    mask_secrets(val);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask_secrets),
        _ => {}
    }
}

/// Restores placeholders in `incoming` from `current`.
///
/// Arrays of objects with an `id` field (notification providers, rules) are
/// matched by id so reordering or deleting entries keeps secrets attached to
/// the right item; other arrays are matched by position.
pub fn merge_secrets(incoming: &mut Value, current: &Value) {
    match (incoming, current) {
        (Value::Object(inc), Value::Object(cur)) => {
            for (key, val) in inc.iter_mut() {
                let Some(cur_val) = cur.get(key) else {
                    // New key: a stray placeholder can't be resolved, clear it.
                    clear_placeholders(val);
                    continue;
                };
                if val.as_str() == Some(SECRET_PLACEHOLDER) {
                    *val = cur_val.clone();
                } else {
                    merge_secrets(val, cur_val);
                }
            }
        }
        (Value::Array(inc), Value::Array(cur)) => {
            for (idx, item) in inc.iter_mut().enumerate() {
                let matched = item
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| cur.iter().find(|c| c.get("id").and_then(Value::as_str) == Some(id)))
                    .or_else(|| {
                        if item.get("id").is_none() { cur.get(idx) } else { None }
                    });
                match matched {
                    Some(cur_item) => merge_secrets(item, cur_item),
                    None => clear_placeholders(item),
                }
            }
        }
        (inc, _) => clear_placeholders(inc),
    }
}

fn clear_placeholders(v: &mut Value) {
    match v {
        Value::String(s) if s == SECRET_PLACEHOLDER => s.clear(),
        Value::Object(m) => m.values_mut().for_each(clear_placeholders),
        Value::Array(a) => a.iter_mut().for_each(clear_placeholders),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_nested_secrets() {
        let mut v = json!({
            "proxmox": {"token_secret": "abc", "url": "https://x"},
            "providers": [{"id": "a", "config": {"webhook_url": "https://discord/x", "username": "bot"}}],
            "empty": {"password": ""},
            "hook": {"headers": {"Authorization": "Bearer t"}}
        });
        mask_secrets(&mut v);
        assert_eq!(v["proxmox"]["token_secret"], SECRET_PLACEHOLDER);
        assert_eq!(v["proxmox"]["url"], "https://x");
        assert_eq!(v["providers"][0]["config"]["webhook_url"], SECRET_PLACEHOLDER);
        assert_eq!(v["providers"][0]["config"]["username"], "bot");
        assert_eq!(v["empty"]["password"], "");
        assert_eq!(v["hook"]["headers"]["Authorization"], SECRET_PLACEHOLDER);
    }

    #[test]
    fn merges_by_id_and_position() {
        let current = json!({
            "token_secret": "real",
            "providers": [
                {"id": "a", "config": {"webhook_url": "https://a"}},
                {"id": "b", "config": {"webhook_url": "https://b"}}
            ]
        });
        let mut incoming = json!({
            "token_secret": SECRET_PLACEHOLDER,
            "providers": [
                {"id": "b", "config": {"webhook_url": SECRET_PLACEHOLDER}},
                {"id": "c", "config": {"webhook_url": SECRET_PLACEHOLDER}}
            ]
        });
        merge_secrets(&mut incoming, &current);
        assert_eq!(incoming["token_secret"], "real");
        assert_eq!(incoming["providers"][0]["config"]["webhook_url"], "https://b");
        assert_eq!(incoming["providers"][1]["config"]["webhook_url"], "");
    }

    #[test]
    fn new_value_replaces_secret() {
        let current = json!({"password": "old"});
        let mut incoming = json!({"password": "new"});
        merge_secrets(&mut incoming, &current);
        assert_eq!(incoming["password"], "new");
    }
}
