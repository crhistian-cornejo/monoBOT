//! Cloudflare runtime: reads the release bucket (`RELEASES`) for `route`.
use crate::{
    content_type, disposition, download_path, route, Route, DOWNLOADS_KEY, FEED_KEY, IMMUTABLE,
    INDEX, NO_STORE,
};
use worker::{event, Bucket, Context, Env, Headers, Method, Request, Response, Result};

fn headers(pairs: &[(&str, &str)]) -> Result<Headers> {
    let headers = Headers::new();
    headers.set("X-Content-Type-Options", "nosniff")?;
    for (name, value) in pairs {
        headers.set(name, value)?;
    }
    Ok(headers)
}

fn text(status: u16, body: &str) -> Result<Response> {
    Ok(Response::ok(body)?
        .with_status(status)
        .with_headers(headers(&[
            ("Content-Type", "text/plain; charset=utf-8"),
            ("Cache-Control", NO_STORE),
        ])?))
}

/// One object, streamed. `HEAD` answers with its headers only.
async fn object(bucket: &Bucket, key: &str, head: bool, cache: &str) -> Result<Response> {
    let found = if head {
        bucket.head(key).await?
    } else {
        bucket.get(key).execute().await?
    };
    let Some(found) = found else {
        return text(404, "Not found\n");
    };
    let size = found.size().to_string();
    let etag = found.http_etag();
    let mut pairs = vec![
        ("Content-Type", content_type(key)),
        ("Cache-Control", cache),
        ("Content-Length", size.as_str()),
        ("ETag", etag.as_str()),
        // The download page reads the version from another origin.
        ("Access-Control-Allow-Origin", "*"),
    ];
    let attachment = disposition(key);
    if let Some(attachment) = &attachment {
        pairs.push(("Content-Disposition", attachment));
    }
    let response = match found.body() {
        Some(body) if !head => Response::from_body(body.response_body()?)?,
        _ => Response::empty()?,
    };
    Ok(response.with_headers(headers(&pairs)?))
}

#[event(fetch)]
async fn fetch(request: Request, env: Env, _ctx: Context) -> Result<Response> {
    let bucket = env.bucket("RELEASES")?;
    let url = request.url()?;
    let head = request.method() == Method::Head;
    match route(request.method().as_ref(), url.path()) {
        Route::Index => text(200, INDEX),
        Route::Feed => object(&bucket, FEED_KEY, head, NO_STORE).await,
        Route::Downloads => object(&bucket, DOWNLOADS_KEY, head, NO_STORE).await,
        Route::File { key } => object(&bucket, &key, head, IMMUTABLE).await,
        Route::Download(key) => {
            let Some(listed) = bucket.get(DOWNLOADS_KEY).execute().await? else {
                return text(404, "No release has been published yet\n");
            };
            let bytes = match listed.body() {
                Some(body) => body.bytes().await?,
                None => Vec::new(),
            };
            let Some(path) = download_path(&bytes, &key) else {
                return text(404, "This installer is not in the latest release\n");
            };
            let location = url.join(&path)?.to_string();
            Ok(Response::empty()?.with_status(302).with_headers(headers(&[
                ("Location", location.as_str()),
                ("Cache-Control", NO_STORE),
            ])?))
        }
        Route::NotFound => text(404, "Not found\n"),
        Route::MethodNotAllowed => Ok(Response::ok("Only GET and HEAD\n")?
            .with_status(405)
            .with_headers(headers(&[
                ("Content-Type", "text/plain; charset=utf-8"),
                ("Allow", "GET, HEAD"),
            ])?)),
    }
}
