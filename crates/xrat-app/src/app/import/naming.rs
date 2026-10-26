use xrat_support::http::HeaderMap;

/// Detects a subscription name from URL (fragment or query parameters) or HTTP response headers.
///
/// Precedence order:
/// 1. HTTP response headers (`Profile-Title`, then `Content-Disposition`)
/// 2. URL fragment (`#MySubscription`)
/// 3. URL query parameters (`?name=...`, `?title=...`, etc.)
pub fn detect_subscription_name(url: &str, headers: Option<&HeaderMap>) -> Option<String> {
    if let Some(headers) = headers
        && let Some(name) = extract_subscription_name_from_headers(headers)
    {
        return Some(name);
    }
    detect_subscription_name_from_url(url)
}

/// Detects a subscription name from a URL fragment or query parameters.
pub fn detect_subscription_name_from_url(url_str: &str) -> Option<String> {
    let parsed = url::Url::parse(url_str).ok()?;

    // 1. Fragment after '#' (e.g. https://domain.com/sub#MySubscription)
    if let Some(fragment) = parsed.fragment() {
        let trimmed = fragment.trim();
        if !trimmed.is_empty() {
            let decoded = percent_encoding::percent_decode_str(trimmed)
                .decode_utf8_lossy()
                .trim()
                .to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }

    // 2. Query parameters (e.g. ?name=..., ?title=..., ?tag=..., ?remark=...)
    for (key, val) in parsed.query_pairs() {
        if matches!(key.as_ref(), "name" | "title" | "tag" | "remark") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

/// Extracts a subscription profile name from HTTP response headers.
pub fn extract_subscription_name_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(title) = headers
        .get("profile-title")
        .and_then(|value| value.to_str().ok())
    {
        let trimmed = title.trim();
        if let Some(encoded) = trimmed.strip_prefix("base64:") {
            if let Ok(decoded) = xrat_support::decode::b64_decode_text(encoded) {
                let name = decoded.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        } else if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    headers
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
}

/// Parses the filename from a Content-Disposition header, stripping extensions.
pub fn parse_content_disposition_filename(header_val: &str) -> Option<String> {
    let mut quoted = false;
    let mut escaped = false;
    let parameters = header_val.split(|character| {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == ';' && !quoted {
            return true;
        }
        false
    });
    let mut plain_filename = None;
    let mut extended_filename = None;
    for parameter in parameters.skip(1) {
        let Some((key, value)) = parameter.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !key.eq_ignore_ascii_case("filename") && !key.eq_ignore_ascii_case("filename*") {
            continue;
        }
        let Some(value) = parameter_value(value.trim()) else {
            continue;
        };
        if key.eq_ignore_ascii_case("filename*") {
            extended_filename =
                decode_extended_filename(&value).and_then(|name| clean_filename(&name));
        } else {
            plain_filename = clean_filename(&value);
        }
    }
    extended_filename.or(plain_filename)
}

fn parameter_value(value: &str) -> Option<String> {
    let Some(quoted) = value.strip_prefix('"') else {
        return Some(value.to_string());
    };
    let mut characters = quoted.chars();
    let mut decoded = String::new();
    while let Some(character) = characters.next() {
        match character {
            '\\' => decoded.push(characters.next()?),
            '"' => return characters.next().is_none().then_some(decoded),
            _ => decoded.push(character),
        }
    }
    None
}

fn decode_extended_filename(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?;
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let bytes = percent_encoding::percent_decode_str(encoded).collect::<Vec<_>>();
    if charset.eq_ignore_ascii_case("utf-8") {
        String::from_utf8(bytes).ok()
    } else if charset.eq_ignore_ascii_case("iso-8859-1") {
        Some(bytes.into_iter().map(char::from).collect())
    } else {
        None
    }
}

fn clean_filename(filename: &str) -> Option<String> {
    let name = filename.trim();
    let cleaned = match name.rsplit_once('.') {
        Some((stem, extension))
            if ["yaml", "yml", "json", "txt", "conf", "sub"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known)) =>
        {
            stem.trim()
        }
        _ => name,
    };
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

#[cfg(test)]
mod tests;
