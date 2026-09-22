use tracing::{Level, info, span};
use uuid::Uuid;
use warp::{Filter, http::Response, hyper::Body};

const APPLE_APP_SITE_ASSOCIATION: &str = include_str!("../static/apple-app-site-association");
const ANDROID_ASSET_LINKS: &str = include_str!("../static/assetlinks.json");
const LOCKBOOK_LOGO: &[u8] =
    include_bytes!("../../public-site/static/favicon/web-app-manifest-512x512.png");

pub fn static_routes(
    public_url: &str,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let public_origin = canonical_https_origin(public_url).unwrap_or_else(|| {
        panic!("PUBLIC_URL must be an HTTPS origin without credentials, path, query, or fragment")
    });
    open_route(public_origin)
        .or(well_known_route())
        .or(logo_route())
}

fn open_route(
    public_origin: String,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("open")
        .and(warp::path::param::<Uuid>())
        .and(warp::path::end())
        .map(move |uuid: Uuid| {
            let span = span!(Level::INFO, "matched_request", method = "GET", route = "/open");
            let _enter = span.enter();
            info!(%uuid, "external link routed");
            warp::reply::html(get_files_preview_html(&public_origin, uuid))
        })
}

fn well_known_route() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path(".well-known")
        .and(
            warp::path("apple-app-site-association")
                .map(|| json_response(APPLE_APP_SITE_ASSOCIATION))
                .or(warp::path("assetlinks.json").map(|| json_response(ANDROID_ASSET_LINKS)))
                .unify(),
        )
        .and(warp::path::end())
}

fn logo_route() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("lockbook-logo.png")
        .and(warp::path::end())
        .map(|| {
            Response::builder()
                .header("Content-Type", "image/png")
                .header("Cache-Control", "public, max-age=86400")
                .body(Body::from(LOCKBOOK_LOGO))
                .unwrap()
        })
}

fn json_response(body: &'static str) -> Response<Body> {
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

pub fn get_files_preview_html(public_origin: &str, uuid: Uuid) -> String {
    let server = urlencoding::encode(public_origin);
    let uuid = uuid.to_string();
    let file = urlencoding::encode(&uuid);
    let handoff = format!("lb://open?server={server}&amp;file={file}");
    let logo = format!("{public_origin}/lockbook-logo.png");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Open shared note in Lockbook</title>
    <meta name="description" content="Someone shared a Lockbook note with you.">
    <meta property="og:title" content="Open shared note in Lockbook">
    <meta property="og:description" content="Someone shared a Lockbook note with you.">
    <meta property="og:type" content="website">
    <meta property="og:image" content="{logo}">
    <meta name="twitter:card" content="summary">
    <meta name="twitter:title" content="Open shared note in Lockbook">
    <meta name="twitter:description" content="Someone shared a Lockbook note with you.">
    <meta name="twitter:image" content="{logo}">
    <style>
        :root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
        body {{ min-height: 100vh; margin: 0; display: grid; place-items: center; background: #f5f3fb; color: #201a29; }}
        main {{ width: min(30rem, calc(100% - 3rem)); padding: 2.5rem; text-align: center; border-radius: 1.5rem; background: #fff; box-shadow: 0 1rem 3rem #251c3820; }}
        img {{ width: 5rem; height: 5rem; }}
        h1 {{ margin-bottom: .5rem; }}
        p {{ line-height: 1.5; color: #5f5868; }}
        a {{ display: inline-block; margin-top: 1rem; padding: .85rem 1.25rem; border-radius: 999px; background: #65558f; color: #fff; font-weight: 700; text-decoration: none; }}
        a:focus-visible {{ outline: 3px solid #ffb4ab; outline-offset: 3px; }}
        @media (prefers-color-scheme: dark) {{ body {{ background: #151218; color: #e9e0ec; }} main {{ background: #211f26; }} p {{ color: #cac4d0; }} }}
    </style>
</head>
<body>
    <main>
        <img src="{logo}" alt="Lockbook logo" width="80" height="80">
        <h1>A Lockbook note was shared with you</h1>
        <p>Open Lockbook to sync your account and view this access-controlled note.</p>
        <a href="{handoff}">Open in Lockbook</a>
    </main>
</body>
</html>"#,
    )
}

fn canonical_https_origin(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = if host.contains(':') { format!("[{host}]") } else { host };
    let port = parsed.port().filter(|port| *port != 443);
    Some(match port {
        Some(port) => format!("https://{host}:{port}"),
        None => format!("https://{host}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "a6743b18-c7ef-4960-9825-8022e2fa5672";

    #[tokio::test]
    async fn valid_open_route_has_explicit_escaped_handoff_and_metadata() {
        let response = warp::test::request()
            .path(&format!("/open/{ID}"))
            .reply(&static_routes("https://Notes.Example.com:443/"))
            .await;
        assert_eq!(response.status(), 200);
        let body = std::str::from_utf8(response.body()).unwrap();
        assert!(body.contains("<title>Open shared note in Lockbook</title>"));
        assert!(body.contains("Someone shared a Lockbook note with you."));
        assert!(body.contains("https://notes.example.com/lockbook-logo.png"));
        assert!(
            body.contains(&format!(
                "lb://open?server=https%3A%2F%2Fnotes.example.com&amp;file={ID}"
            ))
        );
        assert!(!body.contains("window.location"));
    }

    #[tokio::test]
    async fn invalid_open_routes_are_not_found() {
        for path in [format!("/open/not-a-uuid"), format!("/open/{ID}/extra")] {
            let response = warp::test::request()
                .path(&path)
                .reply(&static_routes("https://example.com"))
                .await;
            assert_eq!(response.status(), 404);
        }
    }

    #[tokio::test]
    async fn association_files_are_json_without_redirects() {
        for path in ["/.well-known/apple-app-site-association", "/.well-known/assetlinks.json"] {
            let response = warp::test::request()
                .path(path)
                .reply(&static_routes("https://example.com"))
                .await;
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["content-type"], "application/json");
        }

        let logo = warp::test::request()
            .path("/lockbook-logo.png")
            .reply(&static_routes("https://example.com"))
            .await;
        assert_eq!(logo.status(), 200);
        assert_eq!(logo.headers()["content-type"], "image/png");
    }

    #[test]
    fn configured_origin_must_be_safe_https() {
        assert_eq!(
            canonical_https_origin("https://EXAMPLE.com:443/"),
            Some("https://example.com".into())
        );
        for invalid in [
            "http://example.com",
            "https://user@example.com",
            "https://example.com/path",
            "https://example.com/?query=true",
        ] {
            assert_eq!(canonical_https_origin(invalid), None);
        }
    }
}
