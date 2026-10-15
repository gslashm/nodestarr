//! Surgical edits of compose files.
//!
//! Changing a service's image tag must not reformat the user's file, drop
//! comments or reorder keys. The YAML is parsed only to *locate* the
//! `services.<name>.image` scalar; then exactly those bytes are replaced.

use saphyr_parser::{Event, Parser, ScalarStyle, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct ScalarPos {
    /// Byte range of the scalar in the source, including quotes.
    pub start: usize,
    pub end: usize,
    pub value: String,
    pub style: ScalarStyle,
}

type Ev<'a> = (Event<'a>, Span);

struct Walker<'a> {
    parser: Parser<'a, saphyr_parser::StrInput<'a>>,
}

impl<'a> Walker<'a> {
    fn next(&mut self) -> Result<Ev<'a>, String> {
        match self.parser.next_event() {
            Some(Ok(ev)) => Ok(ev),
            Some(Err(e)) => Err(format!("YAML error: {e}")),
            None => Err("unexpected end of YAML".into()),
        }
    }

    /// Skips the remainder of a value whose first event was `first`.
    fn skip_value(&mut self, first: &Event<'a>) -> Result<(), String> {
        let mut depth = match first {
            Event::MappingStart(..) | Event::SequenceStart(..) => 1,
            _ => 0,
        };
        while depth > 0 {
            match self.next()?.0 {
                Event::MappingStart(..) | Event::SequenceStart(..) => depth += 1,
                Event::MappingEnd | Event::SequenceEnd => depth -= 1,
                Event::StreamEnd => return Err("unexpected end of YAML".into()),
                _ => {}
            }
        }
        Ok(())
    }
}

/// Byte range adjustment: saphyr reports char offsets for some inputs, so
/// convert defensively by checking the slice at the reported range.
fn locate(src: &str, span: &Span, value: &str, style: ScalarStyle) -> Option<(usize, usize)> {
    let (s, e) = (span.start.index(), span.end.index());
    let fits = |s: usize, e: usize| {
        e <= src.len()
            && s <= e
            && src.is_char_boundary(s)
            && src.is_char_boundary(e)
            && slice_matches(&src[s..e], value, style)
    };
    if fits(s, e) {
        return Some((s, e));
    }
    // Fall back to treating the indices as char offsets.
    let to_byte =
        |ci: usize| src.char_indices().nth(ci).map(|(b, _)| b).or((ci == src.chars().count()).then_some(src.len()));
    let (bs, be) = (to_byte(s)?, to_byte(e)?);
    if fits(bs, be) { Some((bs, be)) } else { None }
}

fn slice_matches(slice: &str, value: &str, style: ScalarStyle) -> bool {
    match style {
        ScalarStyle::Plain => slice.trim_end() == value,
        ScalarStyle::SingleQuoted => slice.starts_with('\'') && slice.ends_with('\''),
        ScalarStyle::DoubleQuoted => slice.starts_with('"') && slice.ends_with('"'),
        _ => false,
    }
}

/// Reads a scalar value at the top level of the document (e.g. `name`).
pub fn top_level_scalar(src: &str, key: &str) -> Option<String> {
    let mut w = Walker { parser: Parser::new_from_str(src) };
    loop {
        match w.next().ok()?.0 {
            Event::StreamStart | Event::DocumentStart(_) => continue,
            Event::MappingStart(..) => break,
            _ => return None,
        }
    }
    loop {
        let (k, _) = w.next().ok()?;
        match k {
            Event::MappingEnd => return None,
            Event::Scalar(ref name, ..) if name.as_ref() == key => {
                let (v, _) = w.next().ok()?;
                return match v {
                    Event::Scalar(val, ..) => Some(val.to_string()),
                    _ => None,
                };
            }
            other => {
                w.skip_value(&other).ok()?;
                let (v, _) = w.next().ok()?;
                w.skip_value(&v).ok()?;
            }
        }
    }
}

/// Finds `services.<service>.image` in a compose document.
///
/// Returns `Ok(None)` when the service or its `image` key is absent (for
/// example it is built locally, or inherited through a YAML merge key).
pub fn find_service_image(src: &str, service: &str) -> Result<Option<ScalarPos>, String> {
    let mut w = Walker { parser: Parser::new_from_str(src) };
    // StreamStart, DocumentStart, then the root node.
    loop {
        match w.next()?.0 {
            Event::StreamStart | Event::DocumentStart(_) => continue,
            Event::MappingStart(..) => break,
            Event::StreamEnd => return Ok(None),
            _ => return Err("compose file must be a mapping at the top level".into()),
        }
    }
    // Root mapping: look for "services".
    loop {
        let (key, _) = w.next()?;
        match key {
            Event::MappingEnd => return Ok(None),
            Event::Scalar(ref k, ..) if k.as_ref() == "services" => {
                let (val, _) = w.next()?;
                if !matches!(val, Event::MappingStart(..)) {
                    return Ok(None);
                }
                return find_in_services(&mut w, src, service);
            }
            other => {
                w.skip_value(&other)?;
                let (val, _) = w.next()?;
                w.skip_value(&val)?;
            }
        }
    }
}

fn find_in_services(w: &mut Walker<'_>, src: &str, service: &str) -> Result<Option<ScalarPos>, String> {
    loop {
        let (key, _) = w.next()?;
        match key {
            Event::MappingEnd => return Ok(None),
            Event::Scalar(ref k, ..) if k.as_ref() == service => {
                let (val, _) = w.next()?;
                if !matches!(val, Event::MappingStart(..)) {
                    return Ok(None);
                }
                // Service mapping: look for "image".
                loop {
                    let (k2, _) = w.next()?;
                    match k2 {
                        Event::MappingEnd => return Ok(None),
                        Event::Scalar(ref name, ..) if name.as_ref() == "image" => {
                            let (v, span) = w.next()?;
                            return match v {
                                Event::Scalar(value, style, ..) => {
                                    let value = value.to_string();
                                    let (start, end) = locate(src, &span, &value, style)
                                        .ok_or_else(|| "could not locate the image value in the file".to_string())?;
                                    // Plain scalars may include trailing spaces in the span.
                                    let end = if style == ScalarStyle::Plain {
                                        start + src[start..end].trim_end().len()
                                    } else {
                                        end
                                    };
                                    Ok(Some(ScalarPos { start, end, value, style }))
                                }
                                Event::Alias(_) => {
                                    Err("this service's image comes from a YAML alias; edit the anchor instead".into())
                                }
                                _ => Err("image is not a plain value".into()),
                            };
                        }
                        other => {
                            w.skip_value(&other)?;
                            let (v, _) = w.next()?;
                            w.skip_value(&v)?;
                        }
                    }
                }
            }
            other => {
                w.skip_value(&other)?;
                let (v, _) = w.next()?;
                w.skip_value(&v)?;
            }
        }
    }
}

/// Renders a scalar in the same quoting style as the original.
fn render(value: &str, style: ScalarStyle) -> String {
    match style {
        ScalarStyle::SingleQuoted => format!("'{}'", value.replace('\'', "''")),
        ScalarStyle::DoubleQuoted => serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\"")),
        _ => {
            let plain_ok = !value.is_empty()
                && !value.contains(": ")
                && !value.contains(" #")
                && !value.starts_with(|c: char| "!&*-?[]{}|>'\"%@`,#".contains(c));
            if plain_ok { value.to_string() } else { serde_json::to_string(value).unwrap_or_default() }
        }
    }
}

/// Replaces the scalar at `pos` with `new_value`, preserving quoting.
pub fn replace_scalar(src: &str, pos: &ScalarPos, new_value: &str) -> String {
    let mut out = String::with_capacity(src.len() + new_value.len());
    out.push_str(&src[..pos.start]);
    out.push_str(&render(new_value, pos.style));
    out.push_str(&src[pos.end..]);
    out
}

/// One `${...}` expression inside an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub struct VarRef {
    pub name: String,
    /// Default from `${VAR:-default}` / `${VAR-default}`.
    pub default: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Finds `${VAR}`, `${VAR:-x}`, `${VAR-x}`, `${VAR:?err}` and `$VAR` references.
pub fn variables(s: &str) -> Vec<VarRef> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'$' {
                i += 2; // escaped "$$"
                continue;
            }
            if bytes[i + 1] == b'{' {
                if let Some(close) = s[i + 2..].find('}') {
                    let inner = &s[i + 2..i + 2 + close];
                    let (name, default) = if let Some((n, d)) = inner.split_once(":-") {
                        (n, Some(d.to_string()))
                    } else if let Some((n, d)) = inner.split_once('-') {
                        (n, Some(d.to_string()))
                    } else if let Some((n, _)) = inner.split_once(":?") {
                        (n, None)
                    } else if let Some((n, _)) = inner.split_once('?') {
                        (n, None)
                    } else {
                        (inner, None)
                    };
                    out.push(VarRef { name: name.to_string(), default, start: i, end: i + 3 + close });
                    i += 3 + close;
                    continue;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                let len =
                    s[i + 1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len() - i - 1);
                out.push(VarRef { name: s[i + 1..i + 1 + len].to_string(), default: None, start: i, end: i + 1 + len });
                i += 1 + len;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// How to apply a tag change to an image value.
#[derive(Debug, Clone, PartialEq)]
pub enum TagEdit {
    /// Rewrite the literal value in the compose file.
    Literal(String),
    /// Set the tag variable (usually in `.env`) to the new tag.
    EnvVar { name: String, value: String },
    /// The whole image reference lives in this variable; the caller resolves
    /// its current value and swaps the tag.
    EnvImage { name: String },
}

/// Works out how to point an image value at `new_tag`.
///
/// Handles plain references ("repo:tag", "repo" without a tag) and the two
/// common interpolation styles: a variable tag ("repo:${TAG:-latest}") and
/// a variable for the whole image ("${SONARR_IMAGE}").
pub fn plan_tag_edit(raw: &str, new_tag: &str) -> Result<TagEdit, String> {
    let vars = variables(raw);
    if vars.is_empty() {
        let digest_free = raw.split('@').next().unwrap_or(raw);
        let last_slash = digest_free.rfind('/').map(|i| i + 1).unwrap_or(0);
        let base = match digest_free[last_slash..].rfind(':') {
            Some(i) => &digest_free[..last_slash + i],
            None => digest_free,
        };
        return Ok(TagEdit::Literal(format!("{base}:{new_tag}")));
    }
    if vars.len() == 1 {
        let v = &vars[0];
        if v.start == 0 && v.end == raw.len() {
            return Ok(TagEdit::EnvImage { name: v.name.clone() });
        }
        if v.end == raw.len() && raw[..v.start].ends_with(':') {
            return Ok(TagEdit::EnvVar { name: v.name.clone(), value: new_tag.to_string() });
        }
    }
    Err(format!(
        "the image '{raw}' uses variables in a way Nodestarr can't safely rewrite; edit the compose file directly"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"# media stack
name: media
x-common: &common
  restart: unless-stopped
services:
  plex:
    <<: *common
    image: lscr.io/linuxserver/plex:latest   # keep latest
    network_mode: host
  sonarr:
    image: "lscr.io/linuxserver/sonarr:develop"
    environment:
      - TZ=Europe/Berlin
  radarr:
    image: 'lscr.io/linuxserver/radarr'
  tdarr:
    build: ./tdarr
  ünïcode:
    image: ghcr.io/x/ü:1.0
"#;

    #[test]
    fn finds_and_replaces_plain_with_comment() {
        let pos = find_service_image(DOC, "plex").unwrap().unwrap();
        assert_eq!(pos.value, "lscr.io/linuxserver/plex:latest");
        let out = replace_scalar(DOC, &pos, "lscr.io/linuxserver/plex:1.41.0");
        assert!(out.contains("image: lscr.io/linuxserver/plex:1.41.0   # keep latest"));
        assert_eq!(out.len(), DOC.len() - "latest".len() + "1.41.0".len());
    }

    #[test]
    fn keeps_quote_styles() {
        let pos = find_service_image(DOC, "sonarr").unwrap().unwrap();
        let out = replace_scalar(DOC, &pos, "lscr.io/linuxserver/sonarr:latest");
        assert!(out.contains(r#"image: "lscr.io/linuxserver/sonarr:latest""#));
        let pos = find_service_image(DOC, "radarr").unwrap().unwrap();
        let out = replace_scalar(DOC, &pos, "lscr.io/linuxserver/radarr:nightly");
        assert!(out.contains("image: 'lscr.io/linuxserver/radarr:nightly'"));
    }

    #[test]
    fn missing_or_built_services() {
        assert_eq!(find_service_image(DOC, "tdarr").unwrap(), None);
        assert_eq!(find_service_image(DOC, "nope").unwrap(), None);
    }

    #[test]
    fn handles_multibyte_text() {
        let pos = find_service_image(DOC, "ünïcode").unwrap().unwrap();
        assert_eq!(pos.value, "ghcr.io/x/ü:1.0");
        let out = replace_scalar(DOC, &pos, "ghcr.io/x/ü:2.0");
        assert!(out.contains("image: ghcr.io/x/ü:2.0\n"));
    }

    #[test]
    fn tag_plans() {
        assert_eq!(
            plan_tag_edit("lscr.io/linuxserver/sonarr:latest", "develop").unwrap(),
            TagEdit::Literal("lscr.io/linuxserver/sonarr:develop".into())
        );
        assert_eq!(plan_tag_edit("nginx", "1.27").unwrap(), TagEdit::Literal("nginx:1.27".into()));
        assert_eq!(plan_tag_edit("localhost:5000/app", "2").unwrap(), TagEdit::Literal("localhost:5000/app:2".into()));
        assert_eq!(
            plan_tag_edit("lscr.io/linuxserver/sonarr:${SONARR_TAG:-latest}", "develop").unwrap(),
            TagEdit::EnvVar { name: "SONARR_TAG".into(), value: "develop".into() }
        );
        assert_eq!(plan_tag_edit("${IMG}", "2").unwrap(), TagEdit::EnvImage { name: "IMG".into() });
        assert!(plan_tag_edit("${REG}/app:${TAG}", "2").is_err());
    }

    #[test]
    fn variable_parsing() {
        let v = variables("a:${TAG:-latest}-$SUFFIX$$x");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "TAG");
        assert_eq!(v[0].default.as_deref(), Some("latest"));
        assert_eq!(v[1].name, "SUFFIX");
    }
}
