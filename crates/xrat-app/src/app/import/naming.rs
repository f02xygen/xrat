use xrat_support::http::HeaderMap;

/// Detects a subscription name from URL (fragment or query parameters) or HTTP response headers.
///
/// Precedence order:
/// 1. HTTP response headers (`Profile-Title`, then `Content-Disposition`)
/// 2. URL fragment (`#MySubscription`)
/// 3. URL query parameters (`?name=...`, `?title=...`, etc.)
pub fn detect_subscription_name(url: &str, headers: Option<&HeaderMap>) -> Option<String> {
    if let Some(headers) = headers {
        if let Some(name) = extract_subscription_name_from_headers(headers) {
            return Some(name);
        }
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
    // 1. Check `profile-title` header (supports plain text or `base64:<data>`)
    if let Some(val) = headers.get("profile-title") {
        if let Ok(s) = val.to_str() {
            let trimmed = s.trim();
            if let Some(b64_part) = trimmed.strip_prefix("base64:") {
                if let Ok(decoded) = xrat_support::decode::b64_decode_text(b64_part) {
                    let decoded_trimmed = decoded.trim();
                    if !decoded_trimmed.is_empty() {
                        return Some(decoded_trimmed.to_string());
                    }
                }
            } else if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    // 2. Check `content-disposition` header
    if let Some(val) = headers.get("content-disposition") {
        if let Ok(s) = val.to_str() {
            if let Some(filename) = parse_content_disposition_filename(s) {
                return Some(filename);
            }
        }
    }

    None
}

/// Parses the filename from a Content-Disposition header, stripping extensions.
pub fn parse_content_disposition_filename(header_val: &str) -> Option<String> {
    let mut plain_filename = None;
    let mut ext_filename = None;

    for part in header_val.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("filename*=") {
            let val = val.trim().trim_matches('"');
            let encoded = if let Some((_, rest)) = val.split_once("''") {
                rest
            } else {
                val
            };
            let decoded = percent_encoding::percent_decode_str(encoded)
                .decode_utf8_lossy()
                .trim()
                .to_string();
            if !decoded.is_empty() {
                ext_filename = Some(clean_filename(&decoded));
            }
        } else if let Some(val) = part.strip_prefix("filename=") {
            let val = val.trim().trim_matches('"');
            if !val.is_empty() {
                plain_filename = Some(clean_filename(val));
            }
        }
    }

    ext_filename.or(plain_filename)
}

fn clean_filename(filename: &str) -> String {
    let name = filename.trim();
    for ext in &[".yaml", ".yml", ".json", ".txt", ".conf", ".sub"] {
        if let Some(stripped) = name.strip_suffix(ext) {
            return stripped.trim().to_string();
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrat_support::http::{HeaderName, HeaderValue};

    #[test]
    fn detects_name_from_url_fragment() {
        assert_eq!(
            detect_subscription_name_from_url("https://example.com/api/sub#MyVPN"),
            Some("MyVPN".to_string())
        );

        assert_eq!(
            detect_subscription_name_from_url(
                "https://example.com/sub#%D0%9C%D0%BE%D1%8F%20%D0%9F%D0%BE%D0%B4%D0%BF%D0%B8%D1%81%D0%BA%D0%B0"
            ),
            Some("Моя Подписка".to_string())
        );

        assert_eq!(
            detect_subscription_name_from_url("https://example.com/sub#"),
            None
        );
    }

    #[test]
    fn detects_name_from_url_query() {
        assert_eq!(
            detect_subscription_name_from_url("https://example.com/sub?token=123&name=FastVPN"),
            Some("FastVPN".to_string())
        );

        assert_eq!(
            detect_subscription_name_from_url("https://example.com/sub?title=Work%20VPN"),
            Some("Work VPN".to_string())
        );

        assert_eq!(
            detect_subscription_name_from_url("https://example.com/sub?token=123"),
            None
        );
    }

    #[test]
    fn detects_name_from_profile_title_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("profile-title"),
            HeaderValue::from_static("SuperProxy"),
        );
        assert_eq!(
            extract_subscription_name_from_headers(&headers),
            Some("SuperProxy".to_string())
        );

        // Base64 encoded UTF-8
        let mut b64_headers = HeaderMap::new();
        b64_headers.insert(
            HeaderName::from_static("profile-title"),
            HeaderValue::from_static("base64:0JzQvtGPINCf0L7QtNC/0LjRgdC60LA="),
        );
        assert_eq!(
            extract_subscription_name_from_headers(&b64_headers),
            Some("Моя Подписка".to_string())
        );
    }

    #[test]
    fn detects_name_from_content_disposition_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-disposition"),
            HeaderValue::from_static("attachment; filename=\"MyCluster.yaml\""),
        );
        assert_eq!(
            extract_subscription_name_from_headers(&headers),
            Some("MyCluster".to_string())
        );

        let mut utf8_headers = HeaderMap::new();
        utf8_headers.insert(
            HeaderName::from_static("content-disposition"),
            HeaderValue::from_static(
                "attachment; filename*=UTF-8''%D0%9F%D0%BE%D0%B4%D0%BF%D0%B8%D1%81%D0%BA%D0%B0.yml",
            ),
        );
        assert_eq!(
            extract_subscription_name_from_headers(&utf8_headers),
            Some("Подписка".to_string())
        );
    }

    #[test]
    fn header_takes_precedence_over_url_fragment() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("profile-title"),
            HeaderValue::from_static("ServerTitle"),
        );
        let name = detect_subscription_name("https://example.com/sub#UrlFragment", Some(&headers));
        assert_eq!(name, Some("ServerTitle".to_string()));
    }
}
