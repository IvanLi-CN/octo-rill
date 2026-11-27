use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::Response,
};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

const STATIC_ASSET_EXTENSIONS: &[&str] = &[
    "avif",
    "bmp",
    "css",
    "eot",
    "gif",
    "html",
    "ico",
    "jpeg",
    "jpg",
    "js",
    "json",
    "map",
    "mjs",
    "otf",
    "pdf",
    "png",
    "svg",
    "ttf",
    "txt",
    "wasm",
    "webmanifest",
    "webp",
    "woff",
    "woff2",
    "xml",
];

pub(crate) fn attach_static_site_routes<S>(app: Router<S>, static_dir: PathBuf) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let index = static_dir.join("index.html");
    let spa_static = Arc::new(SpaStaticPaths {
        static_dir,
        index_path: index,
    });

    app.fallback({
        move |request: Request| {
            let spa_static = Arc::clone(&spa_static);
            async move { spa_document_fallback_handler(request, spa_static).await }
        }
    })
}

#[derive(Clone)]
struct SpaStaticPaths {
    static_dir: PathBuf,
    index_path: PathBuf,
}

async fn spa_document_fallback_handler(
    request: Request,
    spa_static: Arc<SpaStaticPaths>,
) -> Response {
    if looks_like_static_asset_path(request.uri().path())
        && !is_admin_app_path(request.uri().path())
    {
        let request_path = request.uri().path().to_owned();
        return match ServeDir::new(spa_static.static_dir.clone())
            .oneshot(request)
            .await
        {
            Ok(response) => {
                let mut response = into_axum_body(response);
                apply_static_site_cache_headers(
                    &request_path,
                    false,
                    response.status(),
                    response.headers_mut(),
                );
                response
            }
            Err(err) => match err {},
        };
    }

    if should_serve_spa_shell(request.method(), request.uri(), request.headers()) {
        let request_path = request.uri().path().to_owned();
        return match ServeFile::new(spa_static.index_path.clone())
            .oneshot(request)
            .await
        {
            Ok(response) => {
                let mut response = into_axum_body(response);
                apply_static_site_cache_headers(
                    &request_path,
                    true,
                    response.status(),
                    response.headers_mut(),
                );
                response
            }
            Err(err) => match err {},
        };
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .expect("404 fallback response should build")
}

fn into_axum_body<B>(response: axum::http::Response<B>) -> Response
where
    B: axum::body::HttpBody<Data = axum::body::Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    response.map(Body::new)
}

fn should_serve_spa_shell(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
    matches!(*method, Method::GET | Method::HEAD)
        && !is_reserved_backend_path(uri.path())
        && (!looks_like_static_asset_path(uri.path()) || is_admin_app_path(uri.path()))
        && (is_spa_entry_path(uri.path()) || accepts_html_document(headers))
}

fn is_reserved_backend_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/") || path == "/auth" || path.starts_with("/auth/")
}

fn is_admin_app_path(path: &str) -> bool {
    path == "/admin" || path.starts_with("/admin/")
}

fn is_spa_entry_path(path: &str) -> bool {
    path == "/" || path == "/admin"
}

fn apply_static_site_cache_headers(
    path: &str,
    spa_shell: bool,
    status: StatusCode,
    headers: &mut HeaderMap,
) {
    let cache_control: &'static str = if spa_shell || path == "/sw.js" {
        "no-store, no-cache, must-revalidate"
    } else if path == "/manifest.webmanifest" {
        "no-cache"
    } else if status.is_success()
        && (path.starts_with("/assets/") || is_hashed_pwa_asset_path(path))
    {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
}

fn is_hashed_pwa_asset_path(path: &str) -> bool {
    let Some(file_name) = path.strip_prefix("/pwa/") else {
        return false;
    };
    let Some(file_name) = file_name.rsplit('/').next() else {
        return false;
    };
    let Some(stem) = file_name.strip_suffix(".png") else {
        return false;
    };
    let Some((basename, digest)) = stem.rsplit_once('.') else {
        return false;
    };

    !basename.is_empty()
        && digest.len() == 16
        && digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn looks_like_static_asset_path(path: &str) -> bool {
    let Some(segment) = path.rsplit('/').next() else {
        return false;
    };
    if segment.is_empty() {
        return false;
    }

    let Some((_, extension)) = segment.rsplit_once('.') else {
        return false;
    };

    STATIC_ASSET_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn accepts_html_document(headers: &HeaderMap) -> bool {
    if headers
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("document"))
    {
        return true;
    }

    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html") || value.contains("application/xhtml+xml"))
}
