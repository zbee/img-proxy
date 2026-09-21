use worker::*;

/// Default interval used when the form omits a refresh frequency.
const DEFAULT_FREQUENCY_HOURS: u64 = 4;
/// Maximum client cache lifetime for stored media and dashboard assets.
const CLIENT_MAX_AGE_SECS: u64 = 86_400;

mod html_components;
mod image_storage;
mod models;
mod request_handlers;
mod utility;

pub(crate) use models::ImageRecord;

/// Routes fetch events into the worker application.
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    request_handlers::run(req, env).await
}
