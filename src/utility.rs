use worker::*;

/// Checks the dashboard key supplied with a request.
pub(crate) fn authorized(req: &Request, ctx: &RouteContext<()>) -> Result<bool> {
    let url = req.url()?;
    let Some(key) = query_param(&url, "key") else {
        return Ok(false);
    };
    let expected = ctx.secret("DASHBOARD_KEY")?.to_string();
    Ok(key == expected)
}

/// Reads the dashboard key used when rendering page forms.
pub(crate) fn dashboard_key(ctx: &RouteContext<()>) -> Result<String> {
    Ok(ctx.secret("DASHBOARD_KEY")?.to_string())
}

/// Converts a form frequency value into hours.
pub(crate) fn parse_frequency(value: Option<String>) -> u64 {
    match value.as_deref() {
        Some("0") => 0,
        Some("1") => 1,
        Some("24") => 24,
        Some("168") => 168,
        Some("720") => 720,
        _ => crate::DEFAULT_FREQUENCY_HOURS,
    }
}

/// Formats a refresh interval for status and notification messages.
pub(crate) fn frequency_label(hours: u64) -> String {
    match hours {
        0 => "never".to_string(),
        1 => "hourly".to_string(),
        4 => "4 hours".to_string(),
        24 => "daily".to_string(),
        168 => "weekly".to_string(),
        720 => "monthly".to_string(),
        _ => format!("{hours}h"),
    }
}

/// Validates names used as KV keys and public URL segments.
pub(crate) fn valid_name(name: &str) -> bool {
    name.len() <= 64
        && name != "refresh"
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Returns one decoded query parameter from a URL.
pub(crate) fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Wraps a string in an HTML response with its content type set.
pub(crate) fn html(body: String) -> Result<Response> {
    let mut resp = Response::ok(body)?;
    resp.headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(resp)
}

/// Removes a supported media extension from a public name.
pub(crate) fn strip_known_extension(name: &str) -> &str {
    for ext in [
        ".gif", ".png", ".jpg", ".jpeg", ".webp", ".svg", ".webm", ".mp4", ".avif",
    ] {
        if let Some(stripped) = name.strip_suffix(ext) {
            return stripped;
        }
    }
    name
}

/// Returns the public URL extension associated with a MIME type.
pub(crate) fn extension_for(content_type: &str) -> &'static str {
    match content_type {
        "image/gif" => ".gif",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "image/svg+xml" => ".svg",
        "video/webm" => ".webm",
        "video/mp4" => ".mp4",
        "image/avif" => ".avif",
        _ => "",
    }
}
