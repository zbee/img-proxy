use worker::*;

use crate::{utility, ImageRecord};

/// Builds gallery markup from the eventually consistent KV listing.
pub(crate) async fn gallery(
    ctx: &RouteContext<()>,
    prepend: Option<(&str, &ImageRecord)>,
    skip: Option<&str>,
) -> Result<String> {
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
    if let Some((name, rec)) = prepend {
        out.push_str(&gallery_tile(
            name,
            rec.content_type.starts_with("video/"),
            utility::extension_for(&rec.content_type),
        ));
    }
    for name in names {
        if skip == Some(name.as_str()) {
            continue;
        }
        if let Some((p, _)) = prepend {
            if p == name.as_str() {
                continue;
            }
        }
        let (is_video, ext) = match kv.get(&name).bytes_with_metadata::<ImageRecord>().await {
            Ok((_, Some(r))) => (
                r.content_type.starts_with("video/"),
                utility::extension_for(&r.content_type),
            ),
            _ => (false, ""),
        };
        out.push_str(&gallery_tile(&name, is_video, ext));
    }
    Ok(out)
}

/// Builds one gallery tile for an image or video.
fn gallery_tile(name: &str, is_video: bool, ext: &str) -> String {
    let media = if is_video {
        format!(
            r#"<video src="/{name}" alt="{name}" muted loop autoplay playsinline preload="metadata" class="w-full h-auto block"></video>"#,
            name = name
        )
    } else {
        format!(
            r#"<img src="/{name}" alt="{name}" class="w-full h-auto block">"#,
            name = name
        )
    };
    format!(
        r#"<figure class="tile group relative mb-4 break-inside-avoid rounded-xl border border-mocha-surface0/80 hover:border-mocha-mauve/50 overflow-hidden bg-mocha-mantle shadow-tile hover:shadow-glow transition-all duration-200 cursor-pointer" data-name="{name}" data-ext="{ext}" style="view-transition-name: tile-{name}">
  {media}
  <figcaption class="absolute bottom-2 left-2 text-[10px] text-mocha-text bg-mocha-crust/80 backdrop-blur-sm px-2 py-0.5 rounded-md border border-mocha-surface0/60 opacity-0 group-hover:opacity-100 transition">{name}</figcaption>
  <button type="button" aria-label="Delete {name}" title="Delete {name}" data-name="{name}"
    class="absolute bottom-2 right-2 w-7 h-7 flex items-center justify-center rounded-md
           bg-mocha-crust/80 hover:bg-mocha-red border border-mocha-surface0/60 hover:border-mocha-red text-mocha-subtext0 hover:text-mocha-crust
           text-lg leading-none opacity-0 group-hover:opacity-100 transition cursor-pointer">&times;</button>
</figure>"#,
        name = name,
        ext = ext,
        media = media
    )
}

/// Reports whether the request asks for the dashboard fragment only.
fn is_partial_request(req: &Request) -> bool {
    matches!(req.headers().get("X-Requested-With"), Ok(Some(v)) if v == "fetch")
}

/// Renders the dashboard content fragment swapped by the client script.
fn dashboard_fragment(gallery: &str, message: &str) -> String {
    format!(
        r#"<div id="dashboard-content">
  {message}
  <section class="mt-12">
    <h2 class="text-xs uppercase font-medium tracking-wider text-mocha-subtext0 mb-4 text-center">Current images</h2>
    <div class="columns-2 md:columns-3 lg:columns-4 gap-4">
      {gallery}
    </div>
  </section>
</div>"#,
        message = message,
        gallery = gallery
    )
}

/// Chooses a full-page or fragment response based on the request headers.
pub(crate) fn render_page_or_fragment(
    req: &Request,
    key: &str,
    gallery: &str,
    message: &str,
) -> Result<Response> {
    if is_partial_request(req) {
        utility::html(dashboard_fragment(gallery, message))
    } else {
        let page = include_str!("../assets/dashboard.html")
            .replace("__KEY__", key)
            .replace("__CONTENT__", &dashboard_fragment(gallery, message));
        utility::html(page)
    }
}

/// Renders a simple green dashboard status message.
pub(crate) fn success_block_simple(message: &str) -> String {
    format!(
        r#"<div class="max-w-2xl mx-auto mt-6 animate-slide-down">
  <div class="bg-mocha-green/10 border border-mocha-green/30 p-4 rounded-xl text-center">
    <p class="text-sm text-mocha-green">{message}</p>
  </div>
</div>"#,
        message = message
    )
}

/// Renders a success message containing a copyable public URL.
pub(crate) fn success_block_with_url(name: &str, url: &str) -> String {
    format!(
        r#"<div class="max-w-2xl mx-auto mt-6 animate-slide-down">
  <h2 class="text-sm font-semibold text-mocha-green mb-2">Added "{name}"</h2>
  <div class="group relative">
    <div class="absolute -inset-0.5 bg-mocha-green/20 blur opacity-75 group-hover:opacity-100 transition rounded-lg"></div>
    <div class="relative flex items-center bg-mocha-crust p-1 rounded-lg border border-mocha-surface0">
      <input type="text" readonly value="{url}" id="result-url"
        class="flex-grow bg-transparent p-2 font-mono text-xs text-mocha-text outline-none">
      <button type="button" id="copy-btn"
        class="rounded-md bg-mocha-green/15 hover:bg-mocha-green/25
               border border-mocha-green/30 text-mocha-green text-xs font-medium px-3 py-1.5 mr-1
               opacity-0 group-hover:opacity-100 transition cursor-pointer">copy</button>
    </div>
  </div>
</div>"#,
        name = name,
        url = url
    )
}

/// Renders a red dashboard error message.
pub(crate) fn error_block(msg: &str) -> String {
    format!(
        r#"<div class="max-w-2xl mx-auto mt-6 animate-slide-down">
  <div class="bg-mocha-red/10 border border-mocha-red/30 p-4 rounded-xl">
    <h2 class="text-sm font-semibold text-mocha-red mb-1">Nope</h2>
    <p class="text-xs text-mocha-red/80">{msg}</p>
  </div>
</div>"#,
        msg = msg
    )
}
