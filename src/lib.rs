use serde::{Deserialize, Serialize};
use worker::*;

const DEFAULT_FREQUENCY_HOURS: u64 = 4;
const CLIENT_MAX_AGE_SECS: u64 = 86_400;
const MAX_BODY_BYTES: usize = 25 * 1024 * 1024;

// KV metadata: everything about the image except the bytes. The KV value is
// the raw body (SVG, GIF, PNG, WebP, WebM...); the metadata field carries this.
// Freshness comes from last_success_ms vs frequency_hours, not a KV TTL —
// entries never expire on their own.
#[derive(Serialize, Deserialize)]
struct ImageRecord {
    source: String,
    frequency_hours: u64,
    #[serde(alias = "fetched_at_ms", default)]
    last_success_ms: u64,
    #[serde(default)]
    dead_since_ms: Option<u64>,
    content_type: String,
}

enum RefreshOutcome {
    Refreshed,
    SkippedFresh,
    ServedStale,
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new()
        .get_async("/", dashboard_page)
        .post_async("/", dashboard_add)
        .post_async("/delete/:name", dashboard_delete)
        .get_async("/refresh", refresh_all)
        .get_async("/fetch", fetch_source)
        .post_async("/upload", upload_image)
        .get("/dashboard.js", serve_dashboard_js)
        .get_async("/:name", serve_image);

    let mut req = req.clone_mut()?;
    let path = req.path_mut()?;
    if path.len() > 1 {
        *path = path.trim_end_matches('/').to_string();
    }

    router.run(req, env).await
}

// --- image serving ---------------------------------------------------

// Serves the dashboard's client script. It's registered ahead of the
// catch-all "/:name" image route so "dashboard.js" is never mistaken for
// an image name.
fn serve_dashboard_js(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let mut resp = Response::ok(include_str!("../assets/dashboard.js"))?;
    resp.headers_mut()
        .set("Content-Type", "application/javascript; charset=utf-8")?;
    resp.headers_mut()
        .set("Cache-Control", &format!("public, max-age={CLIENT_MAX_AGE_SECS}"))?;
    Ok(resp)
}

async fn serve_image(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let name = ctx.param("name").unwrap().to_string();
    let kv = ctx.kv("IMAGES")?;
    let host = req.url()?.host_str().unwrap_or("images.zbee.codes").to_string();

    let Some((rec, bytes, _)) = load_and_refresh(&name, &kv, &host, &ctx).await? else {
        return Response::error("Not found", 404);
    };

    let max_age = if rec.frequency_hours == 0 {
        CLIENT_MAX_AGE_SECS
    } else {
        std::cmp::min(rec.frequency_hours * 3600, CLIENT_MAX_AGE_SECS)
    };

    let mut resp = Response::from_bytes(bytes)?;
    resp.headers_mut().set("Content-Type", &rec.content_type)?;
    resp.headers_mut()
        .set("Cache-Control", &format!("public, max-age={max_age}"))?;
    Ok(resp)
}

// Loads body + metadata in one read, refreshes if due, and returns whatever
// bytes should be served (fresh or stale) so the serve and cron paths share
// the exact same logic.
async fn load_and_refresh(
    name: &str,
    kv: &KvStore,
    host: &str,
    ctx: &RouteContext<()>,
) -> Result<Option<(ImageRecord, Vec<u8>, RefreshOutcome)>> {
    let (bytes, rec) = kv.get(name).bytes_with_metadata::<ImageRecord>().await?;
    let (mut rec, bytes) = match (rec, bytes) {
        (Some(r), Some(b)) => (r, b),
        _ => return Ok(None), // missing, or a legacy JSON-in-value entry
    };

    let now = Date::now().as_millis();
    if rec.frequency_hours == 0 {
        return Ok(Some((rec, bytes, RefreshOutcome::SkippedFresh))); // upload-once
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
                // Re-put the stale body so the death stamp persists with it.
                store_image(kv, name, &rec, &bytes).await?;
                let _ = notify_discord(name, &rec, host, ctx).await;
            }
            Ok(Some((rec, bytes, RefreshOutcome::ServedStale)))
        }
    }
}

async fn store_image(kv: &KvStore, name: &str, rec: &ImageRecord, bytes: &[u8]) -> Result<()> {
    kv.put_bytes(name, bytes)?.metadata(rec)?.execute().await?;
    Ok(())
}

async fn refresh_all(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let key = query_param(&url, "key");
    let expected = match ctx.secret("REFRESH_KEY") {
        Ok(s) => s.to_string(),
        Err(_) => return Response::error("REFRESH_KEY secret not configured", 500),
    };
    if key.as_deref() != Some(expected.as_str()) {
        return Response::error("Forbidden", 403);
    }

    let host = url.host_str().unwrap_or("images.zbee.codes").to_string();
    let kv = ctx.kv("IMAGES")?;

    let mut names: Vec<String> = kv
        .list()
        .execute()
        .await?
        .keys
        .into_iter()
        .map(|k| k.name)
        .collect();
    names.sort();

    let (mut refreshed, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for name in names {
        match load_and_refresh(&name, &kv, &host, &ctx).await {
            Ok(Some((_, _, RefreshOutcome::Refreshed))) => refreshed += 1,
            Ok(Some((_, _, RefreshOutcome::SkippedFresh))) => skipped += 1,
            Ok(Some((_, _, RefreshOutcome::ServedStale))) => failed += 1,
            Ok(None) => skipped += 1, // vanished between list and read
            Err(e) => {
                failed += 1;
                console_log!("refresh error for {name}: {e}");
            }
        }
    }

    Response::from_json(&serde_json::json!({
        "refreshed": refreshed,
        "skipped_fresh": skipped,
        "failed": failed,
    }))
}

async fn fetch_upstream(source: &str) -> Result<(Vec<u8>, String)> {
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

// --- discord ----------------------------------------------------------

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
            {"name": "Refresh every", "value": frequency_label(rec.frequency_hours), "inline": true},
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

// --- dashboard ---------------------------------------------------------

async fn dashboard_page(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = dashboard_key(&ctx)?;
    let url = req.url()?;
    let message = match query_param(&url, "added") {
        Some(name) => {
            let host = url.host_str().unwrap_or("images.zbee.codes");
            success_block(&name, &format!("https://{host}/{name}"))
        }
        None => String::new(),
    };
    render_page_or_fragment(&req, &key, &gallery(&ctx).await?, &message)
}

async fn dashboard_add(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = dashboard_key(&ctx)?;

    let form = match req.form_data().await {
        Ok(f) => f,
        Err(_) => {
            return render_page_or_fragment(
                &req,
                &key,
                &gallery(&ctx).await?,
                &error_block("Invalid form payload"),
            )
        }
    };

    let get = |field: &str| -> Option<String> {
        match form.get(field) {
            Some(FormEntry::Field(v)) if !v.trim().is_empty() => Some(v.trim().to_string()),
            _ => None,
        }
    };

    let Some(name) = get("name") else {
        return render_page_or_fragment(
            &req,
            &key,
            &gallery(&ctx).await?,
            &error_block("A name is required"),
        );
    };
    if !valid_name(&name) {
        return render_page_or_fragment(
            &req,
            &key,
            &gallery(&ctx).await?,
            &error_block("Name must be letters, digits, - or _ (and not 'refresh')"),
        );
    }

    let Some(source) = get("source") else {
        return render_page_or_fragment(
            &req,
            &key,
            &gallery(&ctx).await?,
            &error_block("A source URL is required"),
        );
    };
    if !source.starts_with("http://") && !source.starts_with("https://") {
        return render_page_or_fragment(
            &req,
            &key,
            &gallery(&ctx).await?,
            &error_block("Source must start with http:// or https://"),
        );
    }

    let frequency_hours = parse_frequency(get("frequency"));

    // Fetch once now so we never store a broken entry, and the gallery
    // reflects reality immediately.
    let (bytes, content_type) = match fetch_upstream(&source).await {
        Ok(x) => x,
        Err(e) => {
            return render_page_or_fragment(
                &req,
                &key,
                &gallery(&ctx).await?,
                &error_block(&format!("Could not fetch source: {e}")),
            )
        }
    };

    let rec = ImageRecord {
        source,
        frequency_hours,
        last_success_ms: Date::now().as_millis(),
        dead_since_ms: None,
        content_type,
    };

    let kv = ctx.kv("IMAGES")?;
    if let Err(e) = store_image(&kv, &name, &rec, &bytes).await {
        return render_page_or_fragment(
            &req,
            &key,
            &gallery(&ctx).await?,
            &error_block(&format!("Could not store image: {e} (KV caps at 25 MiB)")),
        );
    }

    let host = req.url()?.host_str().unwrap_or("images.zbee.codes").to_string();
    let image_url = format!("https://{host}/{name}");
    render_page_or_fragment(
        &req,
        &key,
        &gallery(&ctx).await?,
        &success_block(&name, &image_url),
    )
}

async fn dashboard_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = dashboard_key(&ctx)?;
    let name = ctx.param("name").unwrap().to_string();

    ctx.kv("IMAGES")?.delete(&name).await?;

    render_page_or_fragment(
        &req,
        &key,
        &gallery(&ctx).await?,
        &success_block(&format!("Deleted \"{name}\""), ""),
    )
}

// Browser-side conversion endpoints -------------------------------------

async fn fetch_source(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let url = req.url()?;
    let Some(source) = query_param(&url, "url") else {
        return Response::error("Missing ?url=", 400);
    };
    if !source.starts_with("http://") && !source.starts_with("https://") {
        return Response::error("url must be http(s)", 400);
    }
    let (bytes, content_type) = fetch_upstream(&source).await?;
    let mut resp = Response::from_bytes(bytes)?;
    resp.headers_mut().set("Content-Type", &content_type)?;
    Ok(resp)
}

async fn upload_image(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let url = req.url()?;

    let Some(name) = query_param(&url, "name") else {
        return Response::error("Missing ?name=", 400);
    };
    if !valid_name(&name) {
        return Response::error("Invalid name", 400);
    }
    let Some(source) = query_param(&url, "source") else {
        return Response::error("Missing ?source=", 400);
    };
    let content_type = query_param(&url, "content_type")
        .unwrap_or_else(|| "image/webp".to_string());

    let bytes = req.bytes().await?;
    if bytes.len() > MAX_BODY_BYTES {
        return Response::error("Over 25 MiB KV limit", 413);
    }

    let rec = ImageRecord {
        source,
        frequency_hours: 0,
        last_success_ms: Date::now().as_millis(),
        dead_since_ms: None,
        content_type,
    };
    store_image(&ctx.kv("IMAGES")?, &name, &rec, &bytes).await?;

    Response::ok(format!("stored {name}"))
}

// --- gallery -----------------------------------------------------------

async fn gallery(ctx: &RouteContext<()>) -> Result<String> {
    let kv = ctx.kv("IMAGES")?;
    let mut names: Vec<String> = kv
        .list()
        .execute()
        .await?
        .keys
        .into_iter()
        .map(|k| k.name)
        .collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        let is_video = matches!(
            kv.get(&name).bytes_with_metadata::<ImageRecord>().await,
            Ok((_, Some(r))) if r.content_type.starts_with("video/")
        );
        out.push_str(&gallery_tile(&name, is_video));
    }
    Ok(out)
}

fn gallery_tile(name: &str, is_video: bool) -> String {
    let media = if is_video {
        format!(
            r#"<video src="/{name}" alt="{name}" loading="lazy" muted loop autoplay playsinline preload="metadata" class="w-full h-auto block"></video>"#,
            name = name
        )
    } else {
        format!(
            r#"<img src="/{name}" alt="{name}" loading="lazy" class="w-full h-auto block">"#,
            name = name
        )
    };
    format!(
        r#"<figure class="tile group relative mb-4 break-inside-avoid rounded-lg border border-zinc-800/60 overflow-hidden bg-zinc-950 cursor-pointer" data-name="{name}" style="view-transition-name: tile-{name}" onclick="copyImage(this.dataset.name)">
  {media}
  <figcaption class="absolute bottom-2 left-2 text-[10px] text-zinc-400 bg-black/60 px-1.5 py-0.5 rounded">{name}</figcaption>
  <button type="button" aria-label="Delete {name}" title="Delete {name}" data-name="{name}"
    onclick="event.stopPropagation(); openConfirm(this.dataset.name)"
    class="absolute bottom-2 right-2 w-7 h-7 flex items-center justify-center rounded
           bg-red-950/90 hover:bg-red-900 border border-red-900/50 text-red-400
           text-lg leading-none opacity-0 group-hover:opacity-100 transition">&times;</button>
</figure>"#,
        name = name,
        media = media
    )
}

// --- html rendering ------------------------------------------------------

// Every mutating request the dashboard makes (add/update, delete) is issued
// via fetch() from the page's own script, which tags itself with this
// header. On a plain browser navigation (no JS, bookmarked link, etc.) the
// header is absent and we fall back to a full page render.
fn is_partial_request(req: &Request) -> bool {
    matches!(req.headers().get("X-Requested-With"), Ok(Some(v)) if v == "fetch")
}

// The bit of the page that actually changes on every action: the status
// message plus the gallery. Kept as its own element so the client can swap
// it wholesale and the View Transition API has a single, stable node to
// diff old/new content against.
fn dashboard_fragment(gallery: &str, message: &str) -> String {
    format!(
        r#"<div id="dashboard-content">
  {message}
  <section class="mt-12">
    <h2 class="text-xs uppercase tracking-wide text-zinc-500 mb-4 text-center">Current images</h2>
    <div class="columns-2 md:columns-3 lg:columns-4 gap-4">
      {gallery}
    </div>
  </section>
</div>"#,
        message = message,
        gallery = gallery
    )
}

fn render_dashboard(key: &str, gallery: &str, message: &str) -> String {
    include_str!("../assets/dashboard.html")
        .replace("__KEY__", key)
        .replace("__CONTENT__", &dashboard_fragment(gallery, message))
}

// Fragment-only response for script-driven requests (fast, animatable with
// a View Transition on the client), full page otherwise.
fn render_page_or_fragment(req: &Request, key: &str, gallery: &str, message: &str) -> Result<Response> {
    if is_partial_request(req) {
        html(dashboard_fragment(gallery, message))
    } else {
        html(render_dashboard(key, gallery, message))
    }
}

fn success_block(name: &str, url: &str) -> String {
    format!(
        r#"<div class="max-w-2xl mx-auto mt-6 animate-slide-down">
  <h2 class="text-sm font-semibold text-emerald-500 mb-2">{name}</h2>
  <div class="group relative">
    <div class="absolute -inset-0.5 bg-emerald-900/30 blur opacity-75 group-hover:opacity-100 transition rounded"></div>
    <div id="result-url" class="relative bg-black p-3 pr-24 rounded border border-zinc-800 font-mono text-xs break-all text-zinc-300 select-all">{url}</div>
    <button type="button" id="copy-btn" onclick="copyResult()"
      class="absolute right-2 top-1/2 -translate-y-1/2 rounded bg-emerald-950 hover:bg-emerald-900
             border border-emerald-900/50 text-emerald-400 text-xs px-3 py-1.5
             opacity-0 group-hover:opacity-100 transition">copy</button>
  </div>
</div>"#,
        name = name,
        url = url
    )
}

fn error_block(msg: &str) -> String {
    format!(
        r#"<div class="max-w-2xl mx-auto mt-6 animate-slide-down">
  <div class="bg-red-950/30 border border-red-900/50 p-4 rounded-md">
    <h2 class="text-sm font-semibold text-red-500 mb-1">Nope</h2>
    <p class="text-xs text-red-400/80">{msg}</p>
  </div>
</div>"#,
        msg = msg
    )
}

// --- helpers -------------------------------------------------------------

fn authorized(req: &Request, ctx: &RouteContext<()>) -> Result<bool> {
    let url = req.url()?;
    let Some(key) = query_param(&url, "key") else {
        return Ok(false);
    };
    let expected = ctx.secret("DASHBOARD_KEY")?.to_string();
    Ok(key == expected)
}

fn dashboard_key(ctx: &RouteContext<()>) -> Result<String> {
    Ok(ctx.secret("DASHBOARD_KEY")?.to_string())
}

fn parse_frequency(value: Option<String>) -> u64 {
    match value.as_deref() {
        Some("0") => 0,
        Some("1") => 1,
        Some("24") => 24,
        Some("168") => 168,
        Some("720") => 720,
        _ => DEFAULT_FREQUENCY_HOURS,
    }
}

fn frequency_label(hours: u64) -> String {
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

fn valid_name(name: &str) -> bool {
    name.len() <= 64
        && name != "refresh"
        && name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn html(body: String) -> Result<Response> {
    let mut resp = Response::ok(body)?;
    resp.headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(resp)
}
