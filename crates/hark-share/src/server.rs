//! LAN share server: `http://<lan-ip>:<port>/s/<token>` serves a read-only
//! page for a meeting or clip while the app runs. Tokens are 32 hex chars;
//! anything else is 404. Media supports HTTP range requests (seeking).

use crate::html::{render_standalone, PageData};
use axum::body::Body;
use axum::extract::{Path as AxPath, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use hark_store::Store;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::services::ServeFile;

#[derive(Clone)]
struct AppState {
    store: Arc<Store>,
    /// Directory holding rendered clips: `<root>/clips/<token>.mp4|mp3`.
    clips_dir: PathBuf,
}

pub struct ShareServer;

pub struct ShareServerHandle {
    pub port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ShareServerHandle {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ShareServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Best LAN address for links (falls back to 127.0.0.1).
pub fn lan_ip() -> String {
    local_ip_address::local_ip().map(|ip| ip.to_string()).unwrap_or_else(|_| "127.0.0.1".into())
}

pub fn share_url(port: u16, token: &str) -> String {
    format!("http://{}:{port}/s/{token}", lan_ip())
}

impl ShareServer {
    /// Bind on all interfaces and serve on a dedicated thread with its own runtime.
    pub fn start(store: Arc<Store>, clips_dir: PathBuf, port: u16) -> std::io::Result<ShareServerHandle> {
        let (tx, rx) = oneshot::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<u16>>();
        let thread = std::thread::Builder::new().name("hark-share".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            rt.block_on(async move {
                let state = AppState { store, clips_dir };
                let app = Router::new()
                    .route("/", get(|| async { Html("<p>Hark share server. Links look like /s/&lt;token&gt;.</p>") }))
                    .route("/s/{token}", get(page))
                    .route("/s/{token}/media", get(media))
                    .with_state(state);
                let addr = SocketAddr::from(([0, 0, 0, 0], port));
                let listener = match tokio::net::TcpListener::bind(addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let bound = listener.local_addr().map(|a| a.port()).unwrap_or(port);
                let _ = ready_tx.send(Ok(bound));
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = rx.await;
                    })
                    .await;
            });
        })?;
        let port = ready_rx.recv().map_err(|_| std::io::Error::other("share server thread died"))??;
        Ok(ShareServerHandle { port, shutdown: Some(tx), thread: Some(thread) })
    }
}

fn valid_token(t: &str) -> bool {
    t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn page(State(st): State<AppState>, AxPath(token): AxPath<String>) -> Response {
    if !valid_token(&token) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let store = st.store.clone();
    let t = token.clone();
    let result = tokio::task::spawn_blocking(move || -> Option<String> {
        let share = store.get_share(&t).ok().flatten()?;
        let meeting = store.get_meeting(&share.meeting_id).ok().flatten()?;
        let segments = store.segments(&share.meeting_id).ok()?;
        let summary = store.get_summary(&share.meeting_id).ok().flatten();
        let dir = store.recordings_dir(&share.meeting_id);
        let (is_video, clip) = if share.kind == "clip" {
            (dir.join("screen.mp4").exists(), Some((share.start_ms.unwrap_or(0), share.end_ms.unwrap_or(meeting.duration_ms))))
        } else {
            (dir.join("screen.mp4").exists(), None)
        };
        let media_url = format!("/s/{t}/media");
        Some(render_standalone(&PageData {
            meeting: &meeting,
            segments: &segments,
            summary: if clip.is_some() { None } else { summary.as_ref() },
            media_url: Some(&media_url),
            is_video,
            clip,
        }))
    })
    .await
    .ok()
    .flatten();
    match result {
        Some(html) => Html(html).into_response(),
        None => (StatusCode::NOT_FOUND, "This link is no longer available.").into_response(),
    }
}

async fn media(State(st): State<AppState>, AxPath(token): AxPath<String>, headers: HeaderMap) -> Response {
    if !valid_token(&token) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let store = st.store.clone();
    let t = token.clone();
    let clips = st.clips_dir.clone();
    let path = tokio::task::spawn_blocking(move || -> Option<PathBuf> {
        let share = store.get_share(&t).ok().flatten()?;
        let dir = store.recordings_dir(&share.meeting_id);
        if share.kind == "clip" {
            for ext in ["mp4", "mp3"] {
                let p = clips.join(format!("{t}.{ext}"));
                if p.exists() {
                    return Some(p);
                }
            }
            return None;
        }
        let video = dir.join("screen.mp4");
        Some(if video.exists() { video } else { dir.join("mix.wav") })
    })
    .await
    .ok()
    .flatten();
    let Some(path) = path else { return StatusCode::NOT_FOUND.into_response() };
    // ServeFile handles Range requests and content types.
    let mut req = Request::new(Body::empty());
    *req.headers_mut() = headers;
    use tower::ServiceExt;
    match ServeFile::new(&path).oneshot(req).await {
        Ok(resp) => {
            let (mut parts, body) = resp.into_parts();
            parts.headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
