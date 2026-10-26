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

#[test]
fn disposition_handles_quoted_parameters_and_extended_language() {
    for (header, expected) in [
        (r#"attachment; filename="Work;VPN.yaml""#, "Work;VPN"),
        (r#"attachment; filename="Work\"VPN.yaml""#, "Work\"VPN"),
        ("attachment; filename*=UTF-8'en'Work%20VPN.yaml", "Work VPN"),
        (
            "attachment; filename=plain.yaml; filename*=UTF-8'fa'%D8%B4%D8%A8%DA%A9%D9%87.yml",
            "شبکه",
        ),
        ("attachment; FILENAME = work.YAML", "work"),
        (
            "attachment; filename=plain.yaml; filename*=unknown''invalid",
            "plain",
        ),
        (
            "attachment; filename=plain.yaml; filename*=UTF-8''%FF",
            "plain",
        ),
    ] {
        assert_eq!(
            parse_content_disposition_filename(header).as_deref(),
            Some(expected),
            "{header}"
        );
    }
}

#[test]
fn empty_or_malformed_filename_falls_back_to_url_metadata() {
    for value in [
        r#"attachment; filename=".yaml""#,
        r#"attachment; filename="unterminated"#,
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("content-disposition", HeaderValue::from_str(value).unwrap());
        assert_eq!(
            detect_subscription_name("https://example.com/sub#UrlProvider", Some(&headers))
                .as_deref(),
            Some("UrlProvider")
        );
    }
}
