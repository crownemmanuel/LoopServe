use axum::{
    body::{to_bytes, Body},
    extract::{DefaultBodyLimit, FromRequest, Multipart, Path, Request, State},
    http::{header, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use tokio::io::AsyncWriteExt;
use chrono::Utc;
use futures::stream::Stream;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    fs,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

const ALLOWED_EXT: &[&str] = &[
    "mp4", "webm", "mov", "m4v", "ogg", "jpg", "jpeg", "png", "gif", "webp", "svg",
];
const VIDEO_EXT: &[&str] = &["mp4", "webm", "mov", "m4v", "ogg"];
/// Large church loop files (500MB–1GB+) need a high ceiling.
const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ServerPaths {
    pub media_dir: PathBuf,
    pub data_dir: PathBuf,
    pub public_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub filename: String,
    pub original_name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicAsset {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    filename: String,
    original_name: String,
    url: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LiveState {
    live_id: Option<String>,
}

struct InnerState {
    assets: Vec<Asset>,
    live: LiveState,
}

#[derive(Clone)]
pub struct AppState {
    paths: ServerPaths,
    inner: Arc<Mutex<InnerState>>,
    live_tx: broadcast::Sender<Value>,
}

impl AppState {
    pub fn new(paths: ServerPaths) -> Self {
        fs::create_dir_all(&paths.media_dir).ok();
        fs::create_dir_all(&paths.data_dir).ok();

        let assets = read_json(&paths.data_dir.join("assets.json"), Vec::new());
        let live = read_json(&paths.data_dir.join("state.json"), LiveState::default());
        let (live_tx, _) = broadcast::channel(64);

        Self {
            paths,
            inner: Arc::new(Mutex::new(InnerState { assets, live })),
            live_tx,
        }
    }

    fn persist_assets(&self, assets: &[Asset]) {
        write_json(&self.paths.data_dir.join("assets.json"), assets);
    }

    fn persist_live(&self, live: &LiveState) {
        write_json(&self.paths.data_dir.join("state.json"), live);
    }

    fn public_asset(asset: &Asset, live: Option<bool>) -> PublicAsset {
        PublicAsset {
            id: asset.id.clone(),
            name: asset.name.clone(),
            kind: asset.kind.clone(),
            filename: asset.filename.clone(),
            original_name: asset.original_name.clone(),
            url: format!("/media/{}", urlencoding_encode(&asset.filename)),
            created_at: asset.created_at.clone(),
            updated_at: asset.updated_at.clone(),
            live,
        }
    }

    fn get_live_asset(&self) -> Option<PublicAsset> {
        let guard = self.inner.lock();
        let id = guard.live.live_id.as_ref()?;
        let asset = guard.assets.iter().find(|a| &a.id == id)?;
        Some(Self::public_asset(asset, None))
    }

    fn broadcast_live(&self) {
        let payload = json!({ "live": self.get_live_asset() });
        let _ = self.live_tx.send(payload);
    }

    fn set_live(&self, id: Option<String>) -> Result<Value, (StatusCode, Json<Value>)> {
        {
            let mut guard = self.inner.lock();
            if let Some(ref live_id) = id {
                if !guard.assets.iter().any(|a| &a.id == live_id) {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(json!({ "error": "Asset not found" })),
                    ));
                }
            }
            guard.live.live_id = id;
            self.persist_live(&guard.live);
        }
        self.broadcast_live();
        Ok(json!({ "live": self.get_live_asset() }))
    }
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &FsPath, fallback: T) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(fallback)
}

fn write_json<T: Serialize + ?Sized>(path: &FsPath, value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = fs::write(path, text);
    }
}

fn extension_of(name: &str) -> String {
    FsPath::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn media_kind(ext: &str) -> &'static str {
    if VIDEO_EXT.contains(&ext) {
        "video"
    } else {
        "image"
    }
}

fn urlencoding_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..10].to_string()
}

pub fn router(state: AppState) -> Router {
    let media = ServeDir::new(state.paths.media_dir.clone());
    let admin_page = ServeFile::new(state.paths.public_dir.join("admin.html"));
    let public = ServeDir::new(state.paths.public_dir.clone())
        .append_index_html_on_directories(true)
        .not_found_service(ServeFile::new(state.paths.public_dir.join("index.html")));

    Router::new()
        .route("/api/assets", get(list_assets).post(upload_asset))
        .route("/api/assets/{id}", put(update_asset).delete(delete_asset))
        .route("/api/live", get(get_live).put(put_live))
        .route("/api/live/{id}", post(post_live))
        .route("/api/events", get(live_events))
        .route("/api/system/shutdown", post(system_unsupported))
        .route("/api/system/reboot", post(system_unsupported))
        .route("/api/docs", get(api_docs_redirect))
        .route("/openapi.json", get(openapi_json))
        .route_service("/admin", admin_page.clone())
        .route_service("/admin.html", admin_page)
        .nest_service("/media", media)
        .fallback_service(public)
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn write_field_to_file(
    field: axum::extract::multipart::Field<'_>,
    dest: &FsPath,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to create file: {e}") })),
        )
    })?;

    let mut field = field;
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("Failed to write upload: {e}") })),
                    )
                })?;
            }
            Ok(None) => break,
            Err(e) => {
                let _ = tokio::fs::remove_file(dest).await;
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Upload failed: {e}") })),
                ));
            }
        }
    }

    file.flush().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to finalize upload: {e}") })),
        )
    })?;
    Ok(())
}

pub async fn start_server(state: AppState, port: u16) -> Result<(), String> {
    let app = router(state);
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("Failed to bind port {port}: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {e}"))
}

async fn list_assets(State(state): State<AppState>) -> Json<Value> {
    let guard = state.inner.lock();
    let live_id = guard.live.live_id.clone();
    let assets: Vec<_> = guard
        .assets
        .iter()
        .map(|a| {
            let is_live = live_id.as_ref() == Some(&a.id);
            AppState::public_asset(a, Some(is_live))
        })
        .collect();
    Json(json!({ "assets": assets, "liveId": live_id }))
}

async fn upload_asset(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let mut name: Option<String> = None;
    let mut stored_filename: Option<String> = None;
    let mut original_name: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "Upload parse failed ({e}). Large videos are supported up to 2GB — retry after the app reloads."
                )
            })),
        )
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "name" {
            name = Some(
                field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))))?
                    .trim()
                    .to_string(),
            );
        } else if field_name == "file" {
            let original = field
                .file_name()
                .map(|s| s.to_string())
                .ok_or((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Missing filename on upload." })),
                ))?;
            let ext = extension_of(&original);
            if !ALLOWED_EXT.iter().any(|e| *e == ext) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Unsupported file type: .{ext}") })),
                ));
            }
            let stored = format!(
                "{}-{}.{}",
                Utc::now().timestamp_millis(),
                &short_id()[..8],
                ext
            );
            let dest = state.paths.media_dir.join(&stored);
            write_field_to_file(field, &dest).await?;
            stored_filename = Some(stored);
            original_name = Some(original);
        }
    }

    let original = original_name.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "No file uploaded. Use form field 'file'." })),
    ))?;
    let stored = stored_filename.unwrap();
    let ext = extension_of(&original);

    let base_name = name
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            FsPath::new(&original)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("asset")
                .to_string()
        });

    let now = Utc::now().to_rfc3339();
    let asset = Asset {
        id: short_id(),
        name: base_name,
        kind: media_kind(&ext).to_string(),
        filename: stored,
        original_name: original,
        created_at: now.clone(),
        updated_at: now,
    };

    {
        let mut guard = state.inner.lock();
        guard.assets.push(asset.clone());
        state.persist_assets(&guard.assets);
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({ "asset": AppState::public_asset(&asset, None) })),
    ))
}

#[derive(Deserialize)]
struct RenameBody {
    name: Option<String>,
}

async fn update_asset(
    State(state): State<AppState>,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("multipart/form-data") {
        let multipart = Multipart::from_request(req, &state)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.body_text() }))))?;
        return update_asset_multipart(state, id, multipart).await;
    }

    let bytes = to_bytes(req.into_body(), 1024 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))))?;
    let rename: RenameBody = if bytes.is_empty() {
        RenameBody { name: None }
    } else {
        serde_json::from_slice(&bytes).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        })?
    };

    apply_update(state, id, rename.name, None, None)
}

async fn update_asset_multipart(
    state: AppState,
    id: String,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut name: Option<String> = None;
    let mut original_name: Option<String> = None;
    let mut stored_filename: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Upload parse failed ({e})") })),
        )
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "name" {
            let text = field
                .text()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))))?;
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                name = Some(trimmed);
            }
        } else if field_name == "file" {
            let original = field.file_name().map(|s| s.to_string()).ok_or((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing filename on upload." })),
            ))?;
            let ext = extension_of(&original);
            if !ALLOWED_EXT.iter().any(|e| *e == ext) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Unsupported file type: .{ext}") })),
                ));
            }
            let stored = format!(
                "{}-{}.{}",
                Utc::now().timestamp_millis(),
                &short_id()[..8],
                ext
            );
            let dest = state.paths.media_dir.join(&stored);
            write_field_to_file(field, &dest).await?;
            original_name = Some(original);
            stored_filename = Some(stored);
        }
    }

    apply_update(state, id, name, original_name, stored_filename)
}

fn apply_update(
    state: AppState,
    id: String,
    name: Option<String>,
    original_name: Option<String>,
    stored_filename: Option<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let has_file = original_name.is_some() && stored_filename.is_some();
    if !has_file && name.as_ref().is_none_or(|n| n.trim().is_empty()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Provide a new file and/or name to update this asset." })),
        ));
    }

    let mut old_filename: Option<String> = None;
    let updated = {
        let mut guard = state.inner.lock();
        let index = guard
            .assets
            .iter()
            .position(|a| a.id == id)
            .ok_or((StatusCode::NOT_FOUND, Json(json!({ "error": "Asset not found" }))))?;

        if has_file {
            let original = original_name.unwrap();
            let stored = stored_filename.unwrap();
            let ext = extension_of(&original);
            old_filename = Some(guard.assets[index].filename.clone());
            guard.assets[index].filename = stored;
            guard.assets[index].original_name = original;
            guard.assets[index].kind = media_kind(&ext).to_string();
        }

        if let Some(n) = name {
            let trimmed = n.trim().to_string();
            if !trimmed.is_empty() {
                guard.assets[index].name = trimmed;
            }
        }

        guard.assets[index].updated_at = Utc::now().to_rfc3339();
        let asset = guard.assets[index].clone();
        let is_live = guard.live.live_id.as_ref() == Some(&asset.id);
        state.persist_assets(&guard.assets);
        (asset, is_live)
    };

    if let Some(old) = old_filename {
        if old != updated.0.filename {
            let _ = fs::remove_file(state.paths.media_dir.join(old));
        }
    }
    if updated.1 {
        state.broadcast_live();
    }

    Ok(Json(json!({ "asset": AppState::public_asset(&updated.0, None) })))
}

async fn delete_asset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let removed = {
        let mut guard = state.inner.lock();
        let index = guard
            .assets
            .iter()
            .position(|a| a.id == id)
            .ok_or((StatusCode::NOT_FOUND, Json(json!({ "error": "Asset not found" }))))?;
        let removed = guard.assets.remove(index);
        if guard.live.live_id.as_ref() == Some(&removed.id) {
            guard.live.live_id = None;
            state.persist_live(&guard.live);
        }
        state.persist_assets(&guard.assets);
        removed
    };
    let _ = fs::remove_file(state.paths.media_dir.join(&removed.filename));
    state.broadcast_live();
    Ok(Json(json!({ "ok": true, "deleted": AppState::public_asset(&removed, None) })))
}

async fn get_live(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "live": state.get_live_asset() }))
}

#[derive(Deserialize)]
struct LiveBody {
    id: Option<Value>,
    name: Option<String>,
}

async fn put_live(
    State(state): State<AppState>,
    Json(body): Json<LiveBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id_null = matches!(body.id, Some(Value::Null)) || (body.id.is_none() && body.name.is_none());
    if id_null && body.name.is_none() {
        return state.set_live(None).map(Json);
    }
    if let Some(Value::String(id)) = body.id {
        return state.set_live(Some(id)).map(Json);
    }
    if let Some(name) = body.name {
        let found = {
            let guard = state.inner.lock();
            guard
                .assets
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(&name))
                .map(|a| a.id.clone())
        };
        let id = found.ok_or((StatusCode::NOT_FOUND, Json(json!({ "error": "Asset not found" }))))?;
        return state.set_live(Some(id)).map(Json);
    }
    state.set_live(None).map(Json)
}

async fn post_live(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if id == "clear" || id == "none" {
        return state.set_live(None).map(Json);
    }
    state.set_live(Some(id)).map(Json)
}

async fn live_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.live_tx.subscribe();
    let initial = json!({ "live": state.get_live_asset() });

    let stream = async_stream::stream! {
        yield Ok(Event::default().data(initial.to_string()));
        loop {
            match rx.recv().await {
                Ok(payload) => yield Ok(Event::default().data(payload.to_string())),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn system_unsupported() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Device power controls are for Raspberry Pi. Quit the desktop app from the settings window."
        })),
    )
}

async fn api_docs_redirect() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/openapi.json")
        .body(Body::empty())
        .unwrap()
}

async fn openapi_json(State(state): State<AppState>) -> Response {
    let path = state.paths.public_dir.parent().unwrap_or(&state.paths.public_dir).join("openapi.json");
    match fs::read_to_string(path) {
        Ok(text) => Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(text))
            .unwrap(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "openapi.json not found" })),
        )
            .into_response(),
    }
}
