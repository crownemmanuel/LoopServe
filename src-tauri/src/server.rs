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
use serde::{Deserialize, Deserializer, Serialize};
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
/// Stream Deck XL is 4×8; allow a bit of headroom without letting the grid explode.
const MAX_GRID_SIDE: u16 = 16;

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

/// Where a bank lives on a Companion page, used by the "push style" helper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanionLocation {
    page: u16,
    row: u16,
    column: u16,
    #[serde(default)]
    push_style: bool,
}

/// A stable slot a Companion button fires. The bank id never changes; the asset behind it does.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bank {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    companion: Option<CompanionLocation>,
    #[serde(default)]
    updated_at: String,
}

impl Bank {
    fn new(id: String) -> Self {
        Self {
            id,
            label: None,
            asset_id: None,
            companion: None,
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    /// A bank with nothing on it is not worth persisting.
    fn is_blank(&self) -> bool {
        self.asset_id.is_none()
            && self.companion.is_none()
            && self.label.as_ref().is_none_or(|l| l.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GridConfig {
    rows: u16,
    columns: u16,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self { rows: 4, columns: 8 }
    }
}

impl GridConfig {
    fn slot_count(&self) -> usize {
        self.rows as usize * self.columns as usize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BanksState {
    #[serde(default)]
    banks: Vec<Bank>,
    #[serde(default)]
    grid: GridConfig,
}

struct InnerState {
    assets: Vec<Asset>,
    live: LiveState,
    banks: BanksState,
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
        let banks = read_json(&paths.data_dir.join("banks.json"), BanksState::default());
        let (live_tx, _) = broadcast::channel(64);

        Self {
            paths,
            inner: Arc::new(Mutex::new(InnerState { assets, live, banks })),
            live_tx,
        }
    }

    fn persist_assets(&self, assets: &[Asset]) {
        write_json(&self.paths.data_dir.join("assets.json"), assets);
    }

    fn persist_live(&self, live: &LiveState) {
        write_json(&self.paths.data_dir.join("state.json"), live);
    }

    fn persist_banks(&self, banks: &BanksState) {
        write_json(&self.paths.data_dir.join("banks.json"), banks);
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
        let payload = json!({ "type": "live", "live": self.get_live_asset() });
        let _ = self.live_tx.send(payload);
    }

    fn broadcast_banks(&self) {
        let payload = self.banks_payload();
        let _ = self.live_tx.send(json!({
            "type": "banks",
            "banks": payload["banks"],
            "grid": payload["grid"],
        }));
    }

    /// Every slot the grid defines, plus any stored bank outside it, with its asset resolved.
    fn banks_payload(&self) -> Value {
        let guard = self.inner.lock();
        let live_id = guard.live.live_id.clone();
        let grid = guard.banks.grid;

        let mut ids: Vec<String> = (1..=grid.slot_count()).map(|n| n.to_string()).collect();
        for bank in &guard.banks.banks {
            if !ids.iter().any(|id| id == &bank.id) {
                ids.push(bank.id.clone());
            }
        }

        let banks: Vec<Value> = ids
            .iter()
            .map(|id| {
                let bank = guard.banks.banks.iter().find(|b| &b.id == id);
                public_bank(id, bank, &guard.assets, live_id.as_ref())
            })
            .collect();

        json!({ "banks": banks, "grid": grid })
    }

    fn find_bank_payload(&self, id: &str) -> Value {
        let guard = self.inner.lock();
        let live_id = guard.live.live_id.clone();
        let bank = guard.banks.banks.iter().find(|b| b.id == id);
        public_bank(id, bank, &guard.assets, live_id.as_ref())
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

/// Lets a PATCH-style body tell "field absent" (`None`) apart from "field set to null"
/// (`Some(None)`), so `PUT /api/trigger/{id}` can clear one field without touching the rest.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

fn normalize_bank_id(raw: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let id = raw.trim();
    let valid = !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Bank id must be 1–32 characters of letters, digits, '-' or '_'."
            })),
        ));
    }
    Ok(id.to_string())
}

fn public_bank(id: &str, bank: Option<&Bank>, assets: &[Asset], live_id: Option<&String>) -> Value {
    let asset_id = bank.and_then(|b| b.asset_id.clone());
    let asset = asset_id
        .as_ref()
        .and_then(|aid| assets.iter().find(|a| &a.id == aid));
    let custom_label = bank
        .and_then(|b| b.label.clone())
        .filter(|l| !l.trim().is_empty());
    let label = custom_label
        .clone()
        .or_else(|| asset.map(|a| a.name.clone()));

    json!({
        "id": id,
        "label": label,
        "customLabel": custom_label,
        "assetId": asset_id,
        "asset": asset.map(|a| AppState::public_asset(a, None)),
        "companion": bank.and_then(|b| b.companion.clone()),
        "live": asset.is_some_and(|a| live_id == Some(&a.id)),
        // Binding points at an asset that has since been deleted.
        "missing": asset_id.is_some() && asset.is_none(),
        "updatedAt": bank.map(|b| b.updated_at.clone()),
    })
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
        .route("/api/trigger", get(list_banks).put(put_banks_config))
        .route(
            "/api/trigger/{bankId}",
            get(get_bank).put(put_bank).post(fire_bank).delete(delete_bank),
        )
        .route("/api/trigger/{bankId}/clear-binding", post(clear_bank_binding))
        .route("/api/companion/style", post(push_companion_style))
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
    let (removed, banks_changed) = {
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

        // Don't leave banks pointing at media that is gone.
        let before = guard.banks.banks.len();
        let bound = guard
            .banks
            .banks
            .iter()
            .any(|b| b.asset_id.as_ref() == Some(&removed.id));
        if bound {
            let now = Utc::now().to_rfc3339();
            for bank in guard.banks.banks.iter_mut() {
                if bank.asset_id.as_ref() == Some(&removed.id) {
                    bank.asset_id = None;
                    bank.updated_at = now.clone();
                }
            }
            guard.banks.banks.retain(|b| !b.is_blank());
            state.persist_banks(&guard.banks);
        }

        state.persist_assets(&guard.assets);
        (removed, bound || guard.banks.banks.len() != before)
    };
    let _ = fs::remove_file(state.paths.media_dir.join(&removed.filename));
    state.broadcast_live();
    if banks_changed {
        state.broadcast_banks();
    }
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

// ---------------------------------------------------------------------------
// Banks: stable slots that Companion buttons fire, so remapping happens here
// instead of inside Companion's button actions.
// ---------------------------------------------------------------------------

async fn list_banks(State(state): State<AppState>) -> Json<Value> {
    Json(state.banks_payload())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GridBody {
    grid: GridConfig,
}

async fn put_banks_config(
    State(state): State<AppState>,
    Json(body): Json<GridBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let grid = body.grid;
    if grid.rows == 0 || grid.columns == 0 || grid.rows > MAX_GRID_SIDE || grid.columns > MAX_GRID_SIDE
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Grid rows and columns must be between 1 and {MAX_GRID_SIDE}.")
            })),
        ));
    }

    {
        let mut guard = state.inner.lock();
        guard.banks.grid = grid;
        state.persist_banks(&guard.banks);
    }
    state.broadcast_banks();
    Ok(Json(state.banks_payload()))
}

async fn get_bank(
    State(state): State<AppState>,
    Path(bank_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = normalize_bank_id(&bank_id)?;
    Ok(Json(json!({ "bank": state.find_bank_payload(&id) })))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BankUpsert {
    #[serde(default, deserialize_with = "double_option")]
    asset_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    label: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    companion: Option<Option<CompanionLocation>>,
}

async fn put_bank(
    State(state): State<AppState>,
    Path(bank_id): Path<String>,
    body: Option<Json<BankUpsert>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = normalize_bank_id(&bank_id)?;
    let Json(body) = body.unwrap_or_default();

    {
        let mut guard = state.inner.lock();

        if let Some(Some(asset_id)) = body.asset_id.as_ref() {
            if !guard.assets.iter().any(|a| &a.id == asset_id) {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "Asset not found" })),
                ));
            }
        }

        let index = match guard.banks.banks.iter().position(|b| b.id == id) {
            Some(i) => i,
            None => {
                guard.banks.banks.push(Bank::new(id.clone()));
                guard.banks.banks.len() - 1
            }
        };

        if let Some(asset_id) = body.asset_id {
            guard.banks.banks[index].asset_id = asset_id;
        }
        if let Some(label) = body.label {
            guard.banks.banks[index].label = label.filter(|l| !l.trim().is_empty());
        }
        if let Some(companion) = body.companion {
            guard.banks.banks[index].companion = companion;
        }
        guard.banks.banks[index].updated_at = Utc::now().to_rfc3339();

        if guard.banks.banks[index].is_blank() {
            guard.banks.banks.remove(index);
        }
        state.persist_banks(&guard.banks);
    }

    state.broadcast_banks();
    Ok(Json(json!({ "bank": state.find_bank_payload(&id) })))
}

/// Unassign the asset but keep the slot (and its label / Companion location).
fn clear_binding(state: &AppState, id: &str) {
    {
        let mut guard = state.inner.lock();
        if let Some(index) = guard.banks.banks.iter().position(|b| b.id == id) {
            guard.banks.banks[index].asset_id = None;
            guard.banks.banks[index].updated_at = Utc::now().to_rfc3339();
            if guard.banks.banks[index].is_blank() {
                guard.banks.banks.remove(index);
            }
            state.persist_banks(&guard.banks);
        }
    }
    state.broadcast_banks();
}

async fn delete_bank(
    State(state): State<AppState>,
    Path(bank_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = normalize_bank_id(&bank_id)?;
    clear_binding(&state, &id);
    Ok(Json(json!({ "ok": true, "bank": state.find_bank_payload(&id) })))
}

async fn clear_bank_binding(
    State(state): State<AppState>,
    Path(bank_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = normalize_bank_id(&bank_id)?;
    clear_binding(&state, &id);
    Ok(Json(json!({ "ok": true, "bank": state.find_bank_payload(&id) })))
}

/// What a Companion button press lands on. Never 404s on an empty bank — a blank slot
/// is a normal state, and a hard error would just paint the Stream Deck button red.
async fn fire_bank(
    State(state): State<AppState>,
    Path(bank_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = normalize_bank_id(&bank_id)?;

    let asset_id = {
        let guard = state.inner.lock();
        guard
            .banks
            .banks
            .iter()
            .find(|b| b.id == id)
            .and_then(|b| b.asset_id.clone())
    };

    let Some(asset_id) = asset_id else {
        return Ok(Json(json!({
            "ok": true,
            "fired": false,
            "bankId": id,
            "reason": format!("Bank {id} has no asset assigned."),
            "live": state.get_live_asset(),
        })));
    };

    let exists = {
        let guard = state.inner.lock();
        guard.assets.iter().any(|a| a.id == asset_id)
    };
    if !exists {
        return Ok(Json(json!({
            "ok": true,
            "fired": false,
            "bankId": id,
            "assetId": asset_id,
            "reason": format!("Bank {id} points at an asset that no longer exists."),
            "live": state.get_live_asset(),
        })));
    }

    let live = state.set_live(Some(asset_id.clone()))?;
    Ok(Json(json!({
        "ok": true,
        "fired": true,
        "bankId": id,
        "assetId": asset_id,
        "live": live["live"],
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanionStyleBody {
    host: String,
    port: Option<u16>,
    page: u16,
    row: u16,
    column: u16,
    text: Option<String>,
    png64: Option<String>,
    bgcolor: Option<String>,
    color: Option<String>,
}

/// Proxy a button style push to Companion. Done server-side because the control UI is on a
/// different origin than Companion and Companion's HTTP API sends no CORS headers.
async fn push_companion_style(
    State(_state): State<AppState>,
    Json(body): Json<CompanionStyleBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let host = body.host.trim().trim_end_matches('/');
    if host.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Companion host is required." })),
        ));
    }
    let port = body.port.unwrap_or(8000);

    let mut payload = serde_json::Map::new();
    if let Some(text) = body.text {
        payload.insert("text".into(), Value::String(text));
    }
    if let Some(png64) = body.png64 {
        // Companion rejects png64 without the data-URI prefix, then strips it itself.
        let value = if png64.starts_with("data:") {
            png64
        } else {
            format!("data:image/png;base64,{png64}")
        };
        payload.insert("png64".into(), Value::String(value));
    }
    if let Some(bgcolor) = body.bgcolor {
        payload.insert("bgcolor".into(), Value::String(bgcolor));
    }
    if let Some(color) = body.color {
        payload.insert("color".into(), Value::String(color));
    }
    if payload.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Nothing to push — provide text and/or png64." })),
        ));
    }

    let url = format!(
        "http://{host}:{port}/api/location/{}/{}/{}/style",
        body.page, body.row, body.column
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("HTTP client error: {e}") })),
            )
        })?;

    let res = client
        .post(&url)
        .json(&Value::Object(payload))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": format!("Could not reach Companion at {host}:{port} ({e}). Is its HTTP API enabled?")
                })),
            )
        })?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("Companion returned {} — {}", status.as_u16(), text.trim()),
            })),
        ));
    }

    Ok(Json(json!({ "ok": true, "target": url, "response": text.trim() })))
}

async fn live_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.live_tx.subscribe();
    let initial = json!({ "type": "live", "live": state.get_live_asset() });

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Boots the real router on an ephemeral port with a throwaway data dir.
    async fn spawn_test_server() -> (String, PathBuf) {
        let root = std::env::temp_dir().join(format!("loopserve-test-{}", short_id()));
        let media_dir = root.join("media");
        let data_dir = root.join("data");
        fs::create_dir_all(&media_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();

        // Seed one asset without going through multipart upload.
        let now = Utc::now().to_rfc3339();
        let assets = vec![Asset {
            id: "asset-one".into(),
            name: "Stinger A".into(),
            kind: "image".into(),
            filename: "a.png".into(),
            original_name: "a.png".into(),
            created_at: now.clone(),
            updated_at: now,
        }];
        write_json(&data_dir.join("assets.json"), &assets);
        fs::write(media_dir.join("a.png"), b"not-a-real-png").unwrap();

        let state = AppState::new(ServerPaths {
            media_dir,
            data_dir,
            public_dir: root.join("public"),
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(state);
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        (format!("http://{addr}"), root)
    }

    #[tokio::test]
    async fn banks_map_assets_and_fire_live() {
        let (base, root) = spawn_test_server().await;
        let http = reqwest::Client::new();

        // Default grid materialises 4 x 8 empty slots.
        let listed: Value = http
            .get(format!("{base}/api/trigger"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed["grid"]["rows"], 4);
        assert_eq!(listed["banks"].as_array().unwrap().len(), 32);
        assert!(listed["banks"][0]["assetId"].is_null());

        // Firing an unmapped bank is a no-op, not an error.
        let fired: Value = http
            .post(format!("{base}/api/trigger/1"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(fired["fired"], false);

        // Assign, then fire.
        let assigned = http
            .put(format!("{base}/api/trigger/1"))
            .json(&json!({ "assetId": "asset-one", "label": "Opener" }))
            .send()
            .await
            .unwrap();
        assert!(assigned.status().is_success());

        let fired: Value = http
            .post(format!("{base}/api/trigger/1"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(fired["fired"], true);
        assert_eq!(fired["live"]["id"], "asset-one");

        let live: Value = http
            .get(format!("{base}/api/live"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(live["live"]["id"], "asset-one");

        // Custom label wins over the asset name; asset stays resolvable.
        let bank: Value = http
            .get(format!("{base}/api/trigger/1"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(bank["bank"]["label"], "Opener");
        assert_eq!(bank["bank"]["asset"]["name"], "Stinger A");
        assert_eq!(bank["bank"]["live"], true);

        // Bindings survive a restart.
        let persisted: Value =
            serde_json::from_str(&fs::read_to_string(root.join("data/banks.json")).unwrap())
                .unwrap();
        assert_eq!(persisted["banks"][0]["assetId"], "asset-one");

        // Unknown assets are rejected rather than silently stored.
        let bad = http
            .put(format!("{base}/api/trigger/2"))
            .json(&json!({ "assetId": "nope" }))
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), 404);

        // DELETE clears the binding but keeps the label.
        http.delete(format!("{base}/api/trigger/1"))
            .send()
            .await
            .unwrap();
        let bank: Value = http
            .get(format!("{base}/api/trigger/1"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(bank["bank"]["assetId"].is_null());
        assert_eq!(bank["bank"]["customLabel"], "Opener");

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn grid_resize_is_validated_and_persists() {
        let (base, root) = spawn_test_server().await;
        let http = reqwest::Client::new();

        let too_big = http
            .put(format!("{base}/api/trigger"))
            .json(&json!({ "grid": { "rows": 99, "columns": 1 } }))
            .send()
            .await
            .unwrap();
        assert_eq!(too_big.status(), 400);

        let resized: Value = http
            .put(format!("{base}/api/trigger"))
            .json(&json!({ "grid": { "rows": 2, "columns": 3 } }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resized["banks"].as_array().unwrap().len(), 6);

        // A bank outside the visible grid is still listed so its mapping is never orphaned.
        http.put(format!("{base}/api/trigger/20"))
            .json(&json!({ "assetId": "asset-one" }))
            .send()
            .await
            .unwrap();
        let listed: Value = http
            .get(format!("{base}/api/trigger"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let banks = listed["banks"].as_array().unwrap();
        assert_eq!(banks.len(), 7);
        assert_eq!(banks[6]["id"], "20");

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn deleting_an_asset_unbinds_its_banks() {
        let (base, root) = spawn_test_server().await;
        let http = reqwest::Client::new();

        http.put(format!("{base}/api/trigger/5"))
            .json(&json!({ "assetId": "asset-one" }))
            .send()
            .await
            .unwrap();
        http.delete(format!("{base}/api/assets/asset-one"))
            .send()
            .await
            .unwrap();

        let bank: Value = http
            .get(format!("{base}/api/trigger/5"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(bank["bank"]["assetId"].is_null());
        assert_eq!(bank["bank"]["missing"], false);

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn bank_ids_are_validated() {
        let (base, root) = spawn_test_server().await;
        let http = reqwest::Client::new();

        let res = http
            .post(format!("{base}/api/trigger/has%20space"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400);

        fs::remove_dir_all(root).ok();
    }
}
