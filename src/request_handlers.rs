use serde_json::json;
use worker::*;

use crate::{
    html_components, image_storage, utility, ImageRecord, CLIENT_MAX_AGE_SECS,
};

/// Builds the worker router and dispatches one incoming request.
pub(crate) async fn run(req: Request, env: Env) -> Result<Response> {
    let router = Router::new()
        .get_async("/", dashboard_page)
        .post_async("/", dashboard_add)
        .post_async("/delete/:name", dashboard_delete)
        .get_async("/refresh", refresh_all)
        .get("/dashboard.js", serve_dashboard_js)
        .get_async("/:name", serve_image);

    let mut req = req.clone_mut()?;
    let path = req.path_mut()?;
    if path.len() > 1 {
        *path = path.trim_end_matches('/').to_string();
    }

    router.run(req, env).await
}

/// Serves the dashboard's client script.
fn serve_dashboard_js(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let mut resp = Response::ok(include_str!("../assets/dashboard.js"))?;
    resp.headers_mut()
        .set("Content-Type", "application/javascript; charset=utf-8")?;
    resp.headers_mut()
        .set("Cache-Control", &format!("public, max-age={CLIENT_MAX_AGE_SECS}"))?;
    Ok(resp)
}

/// Serves one stored image or video, refreshing it when necessary.
async fn serve_image(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let name = ctx.param("name").unwrap().to_string();
    let name = utility::strip_known_extension(&name).to_string();
    let kv = ctx.kv("IMAGES")?;
    let host = req.url()?.host_str().unwrap_or("images.zbee.codes").to_string();

    let Some((rec, bytes, _)) = image_storage::load_and_refresh(&name, &kv, &host, &ctx).await? else {
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

/// Refreshes all due images and returns a JSON summary of the results.
async fn refresh_all(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let key = utility::query_param(&url, "key");
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

    let (mut refreshed, mut skipped_fresh, mut skipped_permanent, mut failed) =
        (0usize, 0usize, 0usize, 0usize);
    for name in names {
        match image_storage::load_and_refresh(&name, &kv, &host, &ctx).await {
            Ok(Some((_, _, image_storage::RefreshOutcome::Refreshed))) => refreshed += 1,
            Ok(Some((_, _, image_storage::RefreshOutcome::SkippedPermanent))) => {
                skipped_permanent += 1
            }
            Ok(Some((_, _, image_storage::RefreshOutcome::SkippedFresh))) => skipped_fresh += 1,
            Ok(Some((_, _, image_storage::RefreshOutcome::ServedStale))) => failed += 1,
            Ok(None) => skipped_fresh += 1,
            Err(e) => {
                failed += 1;
                console_log!("refresh error for {name}: {e}");
            }
        }
    }

    Response::from_json(&json!({
        "refreshed": refreshed,
        "skipped_fresh": skipped_fresh,
        "skipped_permanent": skipped_permanent,
        "failed": failed,
    }))
}

/// Renders the authenticated dashboard page.
async fn dashboard_page(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !utility::authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = utility::dashboard_key(&ctx)?;
    let url = req.url()?;
    let host = url.host_str().unwrap_or("images.zbee.codes").to_string();
    let mut prepend: Option<(String, ImageRecord)> = None;
    let message = match utility::query_param(&url, "added") {
        Some(name) => {
            match ctx.kv("IMAGES")?.get(&name).bytes_with_metadata::<ImageRecord>().await {
                Ok((_, Some(rec))) => {
                    let ext = utility::extension_for(&rec.content_type);
                    prepend = Some((name.clone(), rec));
                    html_components::success_block_with_url(
                        &name,
                        &format!("https://{host}/{name}{ext}"),
                    )
                }
                _ => html_components::success_block_with_url(
                    &name,
                    &format!("https://{host}/{name}"),
                ),
            }
        }
        None => String::new(),
    };

    let tiles = html_components::gallery(
        &ctx,
        prepend.as_ref().map(|(n, r)| (n.as_str(), r)),
        None,
    )
    .await?;
    html_components::render_page_or_fragment(&req, &key, &tiles, &message)
}

/// Validates, fetches, and stores an image submitted by the dashboard.
async fn dashboard_add(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !utility::authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = utility::dashboard_key(&ctx)?;
    let form = match req.form_data().await {
        Ok(f) => f,
        Err(_) => {
            return html_components::render_page_or_fragment(
                &req,
                &key,
                &html_components::gallery(&ctx, None, None).await?,
                &html_components::error_block("Invalid form payload"),
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
        return render_error(&req, &ctx, &key, "A name is required").await;
    };
    if !utility::valid_name(&name) {
        return render_error(
            &req,
            &ctx,
            &key,
            "Name must be letters, digits, - or _ (and not 'refresh')",
        )
        .await;
    }
    let Some(source) = get("source") else {
        return render_error(&req, &ctx, &key, "A source URL is required").await;
    };
    if !source.starts_with("http://") && !source.starts_with("https://") {
        return render_error(
            &req,
            &ctx,
            &key,
            "Source must start with http:// or https://",
        )
        .await;
    }

    let frequency_hours = utility::parse_frequency(get("frequency"));
    let (bytes, content_type) = match image_storage::fetch_upstream(&source).await {
        Ok(x) => x,
        Err(e) => return render_error(&req, &ctx, &key, &format!("Could not fetch source: {e}")).await,
    };
    let ext = utility::extension_for(&content_type);
    let rec = ImageRecord {
        source,
        frequency_hours,
        last_success_ms: Date::now().as_millis(),
        dead_since_ms: None,
        content_type,
    };

    let kv = ctx.kv("IMAGES")?;
    if let Err(e) = image_storage::store_image(&kv, &name, &rec, &bytes).await {
        return render_error(
            &req,
            &ctx,
            &key,
            &format!("Could not store image: {e} (KV caps at 25 MiB)"),
        )
        .await;
    }

    let host = req.url()?.host_str().unwrap_or("images.zbee.codes").to_string();
    let image_url = format!("https://{host}/{name}{ext}");
    let tiles = html_components::gallery(&ctx, Some((&name, &rec)), None).await?;
    html_components::render_page_or_fragment(
        &req,
        &key,
        &tiles,
        &html_components::success_block_with_url(&name, &image_url),
    )
}

/// Renders a validation or storage error alongside the current gallery.
async fn render_error(
    req: &Request,
    ctx: &RouteContext<()>,
    key: &str,
    message: &str,
) -> Result<Response> {
    let tiles = html_components::gallery(ctx, None, None).await?;
    html_components::render_page_or_fragment(req, key, &tiles, &html_components::error_block(message))
}

/// Deletes a stored image and renders the updated dashboard.
async fn dashboard_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if !utility::authorized(&req, &ctx)? {
        return Response::error("Forbidden", 403);
    }
    let key = utility::dashboard_key(&ctx)?;
    let name = ctx.param("name").unwrap().to_string();
    let name = utility::strip_known_extension(&name).to_string();
    ctx.kv("IMAGES")?.delete(&name).await?;

    let tiles = html_components::gallery(&ctx, None, Some(&name)).await?;
    html_components::render_page_or_fragment(
        &req,
        &key,
        &tiles,
        &html_components::success_block_simple(&format!("Deleted \"{name}\"")),
    )
}
