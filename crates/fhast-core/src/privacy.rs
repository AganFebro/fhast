pub fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "proxy-authorization" | "set-cookie" | "x-api-key"
    )
}

pub fn redact_header_value(name: &str, value: &str) -> String {
    if is_sensitive_header(name) {
        "<redacted>".to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{is_sensitive_header, redact_header_value};

    #[test]
    fn detects_sensitive_headers_case_insensitively() {
        assert!(is_sensitive_header("Cookie"));
        assert!(is_sensitive_header("authorization"));
        assert!(is_sensitive_header("X-Api-Key"));
        assert!(!is_sensitive_header("Referer"));
    }

    #[test]
    fn redacts_sensitive_values() {
        assert_eq!(
            redact_header_value("Authorization", "Bearer token"),
            "<redacted>"
        );
        assert_eq!(
            redact_header_value("Referer", "https://example.com"),
            "https://example.com"
        );
    }
}
