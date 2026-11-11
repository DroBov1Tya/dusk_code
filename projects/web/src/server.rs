//! The routes, and starting and stopping the server.

use crate::{
    auth::{Auth, Login},
    bridge,
    files::{assets, cards, outputs},
    guard::{self, Hosts},
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use dusk_core::client::Hub;
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, watch},
    task::JoinHandle,
};
use tracing::info;

/// Browsers attached at once.
const MAX_BROWSERS: usize = 16;
const LOGIN_BODY_LIMIT: usize = 1024;
const STOP_WAIT: Duration = Duration::from_secs(3);

/// The port the web client is served on unless the user chooses another.
pub const DEFAULT_PORT: u16 = 8080;

pub struct Config {
    /// Where to listen. Loopback unless the user asked for more.
    pub bind: SocketAddr,
    /// Host names (with port) besides the bound address a request may use, such as a
    /// name behind a proxy.
    pub allowed_hosts: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            allowed_hosts: Vec::new(),
        }
    }
}

pub struct WebServer {
    addr: SocketAddr,
    token: String,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl WebServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The address to open: the token is in the fragment, which a browser never sends
    /// anywhere. Shown to the user once; never logged.
    pub fn sign_in_url(&self) -> String {
        // A browser cannot open "any address"; the machine's own name reaches it.
        let host = match self.addr.ip().is_unspecified() {
            true => format!("localhost:{}", self.addr.port()),
            false => self.addr.to_string(),
        };
        format!("http://{host}/#{}", self.token)
    }

    pub fn is_exposed(&self) -> bool {
        !guard::is_local(self.addr.ip())
    }

    /// Ends every browser's stream and stops listening.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        if tokio::time::timeout(STOP_WAIT, self.task).await.is_err() {
            info!("the web server did not stop in time");
        }
    }
}

struct Shared {
    hub: Arc<Hub>,
    auth: Auth,
    hosts: Hosts,
    browsers: Arc<Semaphore>,
    stop: watch::Receiver<bool>,
}

/// Starts serving the web client of `hub` on `config.bind`.
pub async fn start(hub: Arc<Hub>, config: Config) -> Result<WebServer> {
    let listener = TcpListener::bind(config.bind).await.with_context(|| {
        format!(
            "port {} is busy or not allowed on {}; choose another port",
            config.bind.port(),
            config.bind.ip()
        )
    })?;
    let addr = listener.local_addr()?;
    let auth = Auth::new()?;
    let token = auth.token().to_owned();
    let (stop, stopped) = watch::channel(false);
    let shared = Arc::new(Shared {
        hub,
        auth,
        hosts: Hosts::for_bind(addr, &config.allowed_hosts),
        browsers: Arc::new(Semaphore::new(MAX_BROWSERS)),
        stop: stopped.clone(),
    });
    let app = Router::new()
        .route(
            "/api/login",
            post(login).layer(DefaultBodyLimit::max(LOGIN_BODY_LIMIT)),
        )
        .route("/api/logout", post(logout))
        .route("/api/session", get(session))
        .route("/api/ws", get(socket))
        .route("/api/card/{id}/{part}", get(card))
        .route("/api/output", get(output))
        .fallback(get(file))
        .layer(middleware::from_fn_with_state(Arc::clone(&shared), gate))
        .with_state(shared);
    let mut shutdown = stopped;
    let task = tokio::spawn(async move {
        let serve = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown.wait_for(|stop| *stop).await;
        });
        if let Err(error) = serve.await {
            tracing::warn!("the web server failed: {error}");
        }
    });
    Ok(WebServer {
        addr,
        token,
        stop,
        task,
    })
}

/// Every request: the host must be ours, a request that changes anything or opens the
/// stream must come from our own page, and every answer gets the security headers.
async fn gate(State(shared): State<Arc<Shared>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let quiet = matches!(*request.method(), Method::GET | Method::HEAD)
        && request.uri().path() != "/api/ws";
    let allowed = shared.hosts.host_ok(headers) && shared.hosts.origin_ok(headers, !quiet);
    let mut response = if allowed {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    };
    guard::secure(
        response.headers_mut(),
        // Read from the page as it is served, so a rebuilt page keeps its own hash.
        &assets::inline_script_sources(),
        shared.hosts.first(),
    );
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginBody {
    token: String,
}

async fn login(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    let secure = prefixed_cookie(&headers);
    match shared
        .auth
        .login(&body.token, &peer.ip().to_string(), secure)
    {
        Login::Granted(cookie) => with_cookie(StatusCode::NO_CONTENT, &cookie),
        Login::WrongToken => StatusCode::UNAUTHORIZED.into_response(),
        Login::Limited(seconds) => {
            let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
    }
}

async fn logout(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let cookie = shared.auth.logout(&headers, prefixed_cookie(&headers));
    with_cookie(StatusCode::NO_CONTENT, &cookie)
}

async fn session(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> StatusCode {
    match shared.auth.signed_in(&headers) {
        true => StatusCode::NO_CONTENT,
        false => StatusCode::UNAUTHORIZED,
    }
}

async fn socket(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !shared.auth.signed_in(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(permit) = Arc::clone(&shared.browsers).try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (hub, stop) = (Arc::clone(&shared.hub), shared.stop.clone());
    let (checker, cookies) = (Arc::clone(&shared), headers);
    upgrade
        .max_message_size(bridge::MAX_FRAME)
        .max_frame_size(bridge::MAX_FRAME)
        .on_upgrade(move |socket| async move {
            bridge::run(socket, hub, stop, move || checker.auth.signed_in(&cookies)).await;
            drop(permit);
        })
}

async fn card(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Path((id, part)): Path<(String, String)>,
) -> Response {
    if !shared.auth.signed_in(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let folder = shared.hub.status().artifacts_dir;
    let Some(path) = cards::locate(&folder, &id, &part) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((bytes, kind)) = cards::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    response
}

#[derive(Deserialize)]
struct OutputQuery {
    path: String,
}

async fn output(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<OutputQuery>,
) -> Response {
    if !shared.auth.signed_in(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some((bytes, kind)) = outputs::read_picture(&shared.hub, &query.path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    response
}

async fn file(uri: axum::http::Uri) -> Response {
    match assets::get(uri.path()) {
        Some(asset) => {
            let mut response = Response::new(Body::from(asset.body));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            );
            response
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A `__Host-` cookie needs `Secure`, which a browser takes over plain http only on a
/// loopback page.
fn prefixed_cookie(headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(|host| {
            ["localhost:", "127.0.0.1:", "[::1]:"]
                .iter()
                .any(|local| host.to_ascii_lowercase().starts_with(local))
        })
}

fn with_cookie(status: StatusCode, cookie: &str) -> Response {
    let mut response = status.into_response();
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}
