use worker::*;

use crate::{utility, ImageRecord};

/// Describes how a stored image was handled during a refresh attempt.
pub(crate) enum RefreshOutcome {
    /// The image was fetched and written back to KV.
    Refreshed,
    /// The image was configured to never be refreshed.
    SkippedPermanent,
    /// The stored image was still within its refresh interval.
    SkippedFresh,
    /// The upstream failed, so the previous bytes were served.
    ServedStale,
}

/// Loads metadata and bytes, refreshing the image when its interval expires.
pub(crate) async fn load_and_refresh(
    name: &str,
    kv: &KvStore,
    host: &str,
    ctx: &RouteContext<()>,
) -> Result<Option<(ImageRecord, Vec<u8>, RefreshOutcome)>> {
    let (bytes, rec) = kv.get(name).bytes_with_metadata::<ImageRecord>().await?;
    let (mut rec, bytes) = match (rec, bytes) {
        (Some(r), Some(b)) => (r, b),
        _ => return Ok(None),
    };

    let now = Date::now().as_millis();
    if rec.frequency_hours == 0 {
        return Ok(Some((rec, bytes, RefreshOutcome::SkippedPermanent)));
    }
    let age_hours = now.saturating_sub(rec.last_success_ms) / 3_600_000;
    if age_hours < rec.frequency_hours {
        return Ok(Some((rec, bytes, RefreshOutcome::SkippedFresh)));
    }

    match fetch_upstream(&rec.source).await {
        Ok((new_bytes, content_type)) => {
            rec.content_type = content_type;
            rec.last_success_ms = now;
            rec.dead_since_ms = None;
            store_image(kv, name, &rec, &new_bytes).await?;
            Ok(Some((rec, new_bytes, RefreshOutcome::Refreshed)))
        }
        Err(e) => {
            console_log!("refresh failed for {name}: {e}");
            if rec.dead_since_ms.is_none() {
                rec.dead_since_ms = Some(now);
                store_image(kv, name, &rec, &bytes).await?;
                let _ = notify_discord(name, &rec, host, ctx).await;
            }
            Ok(Some((rec, bytes, RefreshOutcome::ServedStale)))
        }
    }
}

/// Writes media bytes and their metadata to the image KV namespace.
pub(crate) async fn store_image(kv: &KvStore, name: &str, rec: &ImageRecord, bytes: &[u8]) -> Result<()> {
    kv.put_bytes(name, bytes)?.metadata(rec)?.execute().await?;
    Ok(())
}

/// Fetches source bytes and their MIME type from an upstream URL.
pub(crate) async fn fetch_upstream(source: &str) -> Result<(Vec<u8>, String)> {
    let headers = Headers::new();
    headers.set("User-Agent", "img-proxy-worker")?;
    headers.set("Accept", "image/svg+xml,image/*;q=0.8,*/*;q=0.5")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);

    let request = Request::new_with_init(source, &init)?;
    let mut resp = Fetch::Request(request).send().await?;

    if resp.status_code() != 200 {
        return Err(Error::from(format!("upstream returned {}", resp.status_code())));
    }

    let content_type = resp
        .headers()
        .get("Content-Type")?
        .unwrap_or_else(|| "image/svg+xml".to_string());
    let bytes = resp.bytes().await?;
    Ok((bytes, content_type))
}

/// Sends a refresh failure notification when a Discord webhook is configured.
async fn notify_discord(
    name: &str,
    rec: &ImageRecord,
    host: &str,
    ctx: &RouteContext<()>,
) -> Result<()> {
    let webhook = match ctx.secret("DISCORD_WEBHOOK") {
        Ok(s) => s.to_string(),
        Err(_) => return Ok(()),
    };
    if webhook.is_empty() {
        return Ok(());
    }

    let dead_unix = rec.dead_since_ms.unwrap_or_else(|| Date::now().as_millis()) / 1000;
    let mut embed = serde_json::json!({
        "title": "img-proxy: refresh failing",
        "description": format!("`{name}` couldn't be refreshed and is serving its last good copy."),
        "color": 0xef4444,
        "fields": [
            {"name": "Image", "value": format!("https://{host}/{name}"), "inline": false},
            {"name": "Source", "value": rec.source.clone(), "inline": false},
            {"name": "Refresh every", "value": utility::frequency_label(rec.frequency_hours), "inline": true},
            {"name": "Dead since", "value": format!("<t:{dead_unix}:R>"), "inline": true}
        ]
    });

    if rec.content_type.starts_with("image/") && !rec.content_type.starts_with("image/svg") {
        embed["thumbnail"] = serde_json::json!({ "url": format!("https://{host}/{name}") });
    }

    let payload = serde_json::json!({ "embeds": [embed] });
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(serde_json::to_string(&payload)?.into()));

    let request = Request::new_with_init(&webhook, &init)?;
    let resp = Fetch::Request(request).send().await?;
    if resp.status_code() >= 300 {
        console_log!("discord webhook returned {}", resp.status_code());
    }
    Ok(())
}
