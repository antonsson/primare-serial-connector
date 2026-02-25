use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::error::{ApiResult, AppError};
use crate::protocol::ir_remote;
use crate::serial::SerialConnection;
use crate::state::AppState;

// ---- Type-safe enums for API requests ----

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    On,
    Off,
    Toggle,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuAction {
    Enter,
    Exit,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrSource {
    Front,
    Back,
}

async fn with_serial<T>(
    state: &Arc<AppState>,
    op: impl for<'a> FnOnce(&'a mut SerialConnection) -> Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'a>>,
) -> ApiResult<T> {
    let mut serial = state.get_serial().await?;
    let result = op(&mut serial).await;

    if let Err(err) = &result {
        if err.should_disconnect() {
            drop(serial);
            state.disconnect().await;
        }
    }

    result
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health",          get(health_check))
        .route("/status",          get(get_status))
        .route("/power",           get(get_power).post(set_power))
        .route("/volume",          get(get_volume).post(set_volume))
        .route("/input",           get(get_input).post(set_input))
        .route("/mute",            get(get_mute).post(set_mute))
        .route("/balance",         get(get_balance).post(set_balance))
        .route("/dim",             get(get_dim).post(set_dim))
        .route("/menu",            post(menu_action))
        .route("/ir_input",        get(get_ir_input).post(set_ir_input))
        .route("/info",            get(get_info))
        .route("/input/current/name",      get(get_current_input_name))
        .route("/input/:id/name",  get(get_input_name))
        .route("/factory_reset",   post(factory_reset))
}

// ---- Health ----

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub connected: bool,
    pub port: String,
}

async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let connected = state.is_connected().await;
    Json(HealthResponse {
        status: "ok".into(),
        connected,
        port: state.config.port.clone(),
    })
}

// ---- Status ----

#[derive(Serialize)]
pub struct StatusResponse {
    pub power: bool,
    pub volume: u8,
    pub input: u8,
    pub mute: bool,
    pub balance: i8,
    pub dim: u8,
}

async fn get_status(State(state): State<Arc<AppState>>) -> ApiResult<Json<StatusResponse>> {
    let status = with_serial(&state, |s| {
        Box::pin(async move {
            Ok(StatusResponse {
                power: s.get_power().await?,
                volume: s.get_volume().await?,
                input: s.get_input().await?,
                mute: s.get_mute().await?,
                balance: s.get_balance().await?,
                dim: s.get_dim().await?,
            })
        })
    })
    .await?;
    Ok(Json(status))
}

// ---- Power ----

#[derive(Serialize)]
pub struct PowerResponse {
    pub power: bool,
}

#[derive(Deserialize)]
pub struct PowerRequest {
    pub state: PowerState,
}

async fn get_power(State(state): State<Arc<AppState>>) -> ApiResult<Json<PowerResponse>> {
    let power = with_serial(&state, |s| Box::pin(async move { s.get_power().await })).await?;
    Ok(Json(PowerResponse { power }))
}

async fn set_power(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PowerRequest>,
) -> ApiResult<Json<PowerResponse>> {
    let power = with_serial(&state, |s| {
        Box::pin(async move {
            match body.state {
                PowerState::On => s.set_power(true).await,
                PowerState::Off => s.set_power(false).await,
                PowerState::Toggle => s.toggle_power().await,
            }
        })
    })
    .await?;
    Ok(Json(PowerResponse { power }))
}

// ---- Volume ----

#[derive(Serialize)]
pub struct VolumeResponse {
    pub volume: u8,
}

#[derive(Deserialize)]
pub struct VolumeRequest {
    /// Absolute level 0-79
    pub level: Option<u8>,
    /// Relative step: +1 or -1
    pub step: Option<i8>,
}

async fn get_volume(State(state): State<Arc<AppState>>) -> ApiResult<Json<VolumeResponse>> {
    let volume = with_serial(&state, |s| Box::pin(async move { s.get_volume().await })).await?;
    Ok(Json(VolumeResponse { volume }))
}

async fn set_volume(
    State(state): State<Arc<AppState>>,
    Json(body): Json<VolumeRequest>,
) -> ApiResult<Json<VolumeResponse>> {
    let volume = with_serial(&state, |s| {
        Box::pin(async move {
            match (body.level, body.step) {
                (Some(level), _) => s.set_volume(level).await,
                (_, Some(step)) => s.step_volume(step > 0).await,
                _ => Err(AppError::InvalidParameter(
                    "Provide either 'level' (0-79) or 'step' (+1/-1)".into(),
                )),
            }
        })
    })
    .await?;
    Ok(Json(VolumeResponse { volume }))
}

// ---- Input ----

#[derive(Serialize)]
pub struct InputResponse {
    pub input: u8,
}

#[derive(Deserialize)]
pub struct InputRequest {
    /// Direct input 1-7
    pub input: Option<u8>,
    pub step: Option<Direction>,
}

async fn get_input(State(state): State<Arc<AppState>>) -> ApiResult<Json<InputResponse>> {
    let input = with_serial(&state, |s| Box::pin(async move { s.get_input().await })).await?;
    Ok(Json(InputResponse { input }))
}

async fn set_input(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InputRequest>,
) -> ApiResult<Json<InputResponse>> {
    let input = with_serial(&state, |s| {
        Box::pin(async move {
            match (body.input, body.step) {
                (Some(i), _) => s.set_input(i).await,
                (_, Some(Direction::Up)) => s.step_input(true).await,
                (_, Some(Direction::Down)) => s.step_input(false).await,
                _ => Err(AppError::InvalidParameter(
                    "Provide either 'input' (1-7) or 'step' (up/down)".into(),
                )),
            }
        })
    })
    .await?;
    Ok(Json(InputResponse { input }))
}

// ---- Mute ----

#[derive(Serialize)]
pub struct MuteResponse {
    pub mute: bool,
}

#[derive(Deserialize)]
pub struct MuteRequest {
    /// true, false, or null for toggle
    pub state: Option<bool>,
}

async fn get_mute(State(state): State<Arc<AppState>>) -> ApiResult<Json<MuteResponse>> {
    let mute = with_serial(&state, |s| Box::pin(async move { s.get_mute().await })).await?;
    Ok(Json(MuteResponse { mute }))
}

async fn set_mute(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MuteRequest>,
) -> ApiResult<Json<MuteResponse>> {
    let mute = with_serial(&state, |s| {
        Box::pin(async move {
            match body.state {
                Some(v) => s.set_mute(v).await,
                None => s.toggle_mute().await,
            }
        })
    })
    .await?;
    Ok(Json(MuteResponse { mute }))
}

// ---- Balance ----

#[derive(Serialize)]
pub struct BalanceResponse {
    pub balance: i8,
}

#[derive(Deserialize)]
pub struct BalanceRequest {
    /// Direct value -9 to +9
    pub value: Option<i8>,
    /// Relative step
    pub step: Option<i8>,
}

async fn get_balance(State(state): State<Arc<AppState>>) -> ApiResult<Json<BalanceResponse>> {
    let balance = with_serial(&state, |s| Box::pin(async move { s.get_balance().await })).await?;
    Ok(Json(BalanceResponse { balance }))
}

async fn set_balance(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BalanceRequest>,
) -> ApiResult<Json<BalanceResponse>> {
    let balance = with_serial(&state, |s| {
        Box::pin(async move {
            match (body.value, body.step) {
                (Some(v), _) => s.set_balance(v).await,
                (_, Some(step)) => s.step_balance(step).await,
                _ => Err(AppError::InvalidParameter(
                    "Provide either 'value' (-9..9) or 'step'".into(),
                )),
            }
        })
    })
    .await?;
    Ok(Json(BalanceResponse { balance }))
}

// ---- Dim ----

#[derive(Serialize)]
pub struct DimResponse {
    /// 0=off, 1-3=brightness levels
    pub dim: u8,
}

#[derive(Deserialize)]
pub struct DimRequest {
    /// Direct level 0-3
    pub level: Option<u8>,
    /// true = step to next level
    #[serde(default)]
    pub step: bool,
}

async fn get_dim(State(state): State<Arc<AppState>>) -> ApiResult<Json<DimResponse>> {
    let dim = with_serial(&state, |s| Box::pin(async move { s.get_dim().await })).await?;
    Ok(Json(DimResponse { dim }))
}

async fn set_dim(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DimRequest>,
) -> ApiResult<Json<DimResponse>> {
    let dim = with_serial(&state, |s| {
        Box::pin(async move {
            match (body.level, body.step) {
                (Some(l), _) => s.set_dim(l).await,
                (None, true) => s.step_dim().await,
                _ => Err(AppError::InvalidParameter(
                    "Provide either 'level' (0-3) or 'step': true".into(),
                )),
            }
        })
    })
    .await?;
    Ok(Json(DimResponse { dim }))
}

// ---- Menu ----

#[derive(Deserialize)]
pub struct MenuRequest {
    pub action: MenuAction,
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

async fn menu_action(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MenuRequest>,
) -> ApiResult<Json<OkResponse>> {
    with_serial(&state, |s| {
        Box::pin(async move {
            match body.action {
                MenuAction::Enter => s.menu_enter().await?,
                MenuAction::Exit => s.menu_exit().await?,
                MenuAction::Up => s.menu_nav(ir_remote::STEP_UP).await?,
                MenuAction::Down => s.menu_nav(ir_remote::STEP_DOWN).await?,
                MenuAction::Right => s.menu_nav(ir_remote::ARROW_RIGHT).await?,
                MenuAction::Left => s.menu_nav(ir_remote::ARROW_LEFT).await?,
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}

fn ir_source_str(back: bool) -> &'static str {
    if back { "back" } else { "front" }
}

// ---- IR Input ----

#[derive(Serialize)]
pub struct IrInputResponse {
    /// "front" or "back"
    pub source: String,
}

#[derive(Deserialize)]
pub struct IrInputRequest {
    pub source: IrSource,
}

async fn get_ir_input(State(state): State<Arc<AppState>>) -> ApiResult<Json<IrInputResponse>> {
    let back = with_serial(&state, |s| Box::pin(async move { s.get_ir_input().await })).await?;
    Ok(Json(IrInputResponse { source: ir_source_str(back).into() }))
}

async fn set_ir_input(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IrInputRequest>,
) -> ApiResult<Json<IrInputResponse>> {
    let back = with_serial(&state, |s| {
        Box::pin(async move {
            match body.source {
                IrSource::Back => s.set_ir_input(true).await,
                IrSource::Front => s.set_ir_input(false).await,
            }
        })
    })
    .await?;
    Ok(Json(IrInputResponse { source: ir_source_str(back).into() }))
}

// ---- Info ----

#[derive(Serialize)]
pub struct InfoResponse {
    pub product_line: String,
    pub model: String,
    pub firmware: String,
}

async fn get_info(State(state): State<Arc<AppState>>) -> ApiResult<Json<InfoResponse>> {
    let info = with_serial(&state, |s| {
        Box::pin(async move {
            Ok(InfoResponse {
                product_line: s.get_product_line().await?,
                model: s.get_model_name().await?,
                firmware: s.get_version().await?,
            })
        })
    })
    .await?;
    Ok(Json(info))
}

// ---- Input names ----

#[derive(Serialize)]
pub struct InputNameResponse {
    pub name: String,
}

async fn get_current_input_name(State(state): State<Arc<AppState>>) -> ApiResult<Json<InputNameResponse>> {
    let name = with_serial(&state, |s| Box::pin(async move { s.get_input_name_current().await })).await?;
    Ok(Json(InputNameResponse { name }))
}

async fn get_input_name(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
) -> ApiResult<Json<InputNameResponse>> {
    let name = with_serial(&state, |s| Box::pin(async move { s.get_input_name(id).await })).await?;
    Ok(Json(InputNameResponse { name }))
}

// ---- Factory reset ----

#[derive(Deserialize)]
pub struct FactoryResetRequest {
    pub confirm: bool,
}

async fn factory_reset(
    State(state): State<Arc<AppState>>,
    Json(body): Json<FactoryResetRequest>,
) -> ApiResult<Json<OkResponse>> {
    if !body.confirm {
        return Err(AppError::InvalidParameter(
            "Set 'confirm': true to proceed with factory reset".into()
        ));
    }
    with_serial(&state, |s| {
        Box::pin(async move {
            s.factory_reset().await?;
            Ok(())
        })
    })
    .await?;
    Ok(Json(OkResponse { ok: true }))
}
