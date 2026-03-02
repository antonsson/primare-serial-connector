use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::error::{ApiResult, AppError};
use crate::protocol::ir_remote;
use crate::state::AppState;

// ---- Type-safe enums for API requests ----

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuAction {
    Enter,
    Exit,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IrSource {
    Front,
    Back,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/status", get(get_status))
        .route("/power", get(get_power).post(set_power))
        .route("/volume", get(get_volume).post(set_volume))
        .route("/input", get(get_input).post(set_input))
        .route("/mute", get(get_mute).post(set_mute))
        .route("/balance", get(get_balance).post(set_balance))
        .route("/dim", get(get_dim).post(set_dim))
        .route("/menu", post(menu_action))
        .route("/ir_input", get(get_ir_input).post(set_ir_input))
        .route("/info", get(get_info))
        .route("/input/current/name", get(get_current_input_name))
        .route("/input/:id/name", get(get_input_name))
        .route("/factory_reset", post(factory_reset))
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
    info!("HTTP GET /status");
    let mut conn = state.get_serial().await?;
    Ok(Json(StatusResponse {
        power: conn.get_power(),
        volume: conn.get_volume().await?,
        input: conn.get_input().await?,
        mute: false,
        balance: 0,
        dim: 2,
    }))
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
    info!("HTTP GET /power");
    let conn = state.get_serial().await?;
    Ok(Json(PowerResponse {
        power: conn.get_power(),
    }))
}

async fn set_power(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PowerRequest>,
) -> ApiResult<Json<PowerResponse>> {
    let state_name = match body.state {
        PowerState::On => "on",
        PowerState::Off => "off",
        PowerState::Toggle => "toggle",
    };
    info!("HTTP POST /power state={}", state_name);
    let mut conn = state.get_serial().await?;
    let power = match body.state {
        PowerState::On => conn.set_power(true).await?,
        PowerState::Off => conn.set_power(false).await?,
        PowerState::Toggle => conn.toggle_power().await?,
    };
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
    info!("HTTP GET /volume");
    let mut conn = state.get_serial().await?;
    Ok(Json(VolumeResponse {
        volume: conn.get_volume().await?,
    }))
}

async fn set_volume(
    State(state): State<Arc<AppState>>,
    Json(body): Json<VolumeRequest>,
) -> ApiResult<Json<VolumeResponse>> {
    info!("HTTP POST /volume level={:?} step={:?}", body.level, body.step);
    let mut conn = state.get_serial().await?;
    let volume = match (body.level, body.step) {
        (Some(level), _) => conn.set_volume(level).await?,
        (_, Some(step)) => conn.step_volume(step > 0).await?,
        _ => {
            return Err(AppError::InvalidParameter(
                "Provide either 'level' (0-79) or 'step' (+1/-1)".into(),
            ))
        }
    };
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
    info!("HTTP GET /input");
    let mut conn = state.get_serial().await?;
    Ok(Json(InputResponse {
        input: conn.get_input().await?,
    }))
}

async fn set_input(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InputRequest>,
) -> ApiResult<Json<InputResponse>> {
    info!("HTTP POST /input input={:?} step={:?}", body.input, body.step);
    let mut conn = state.get_serial().await?;
    let input = match (body.input, body.step) {
        (Some(i), _) => conn.set_input(i).await?,
        (_, Some(Direction::Up)) => conn.step_input(true).await?,
        (_, Some(Direction::Down)) => conn.step_input(false).await?,
        _ => {
            return Err(AppError::InvalidParameter(
                "Provide either 'input' (1-7) or 'step' (up/down)".into(),
            ))
        }
    };
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
    info!("HTTP GET /mute");
    let mut conn = state.get_serial().await?;
    Ok(Json(MuteResponse {
        mute: conn.get_mute().await?,
    }))
}

async fn set_mute(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MuteRequest>,
) -> ApiResult<Json<MuteResponse>> {
    info!("HTTP POST /mute state={:?}", body.state);
    let mut conn = state.get_serial().await?;
    let mute = match body.state {
        Some(v) => conn.set_mute(v).await?,
        None => conn.toggle_mute().await?,
    };
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
    info!("HTTP GET /balance");
    let mut conn = state.get_serial().await?;
    Ok(Json(BalanceResponse {
        balance: conn.get_balance().await?,
    }))
}

async fn set_balance(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BalanceRequest>,
) -> ApiResult<Json<BalanceResponse>> {
    info!("HTTP POST /balance value={:?} step={:?}", body.value, body.step);
    let mut conn = state.get_serial().await?;
    let balance = match (body.value, body.step) {
        (Some(v), _) => conn.set_balance(v).await?,
        (_, Some(step)) => conn.step_balance(step).await?,
        _ => {
            return Err(AppError::InvalidParameter(
                "Provide either 'value' (-9..9) or 'step'".into(),
            ))
        }
    };
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
    info!("HTTP GET /dim");
    let mut conn = state.get_serial().await?;
    Ok(Json(DimResponse {
        dim: conn.get_dim().await?,
    }))
}

async fn set_dim(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DimRequest>,
) -> ApiResult<Json<DimResponse>> {
    info!("HTTP POST /dim level={:?} step={}", body.level, body.step);
    let mut conn = state.get_serial().await?;
    let dim = match (body.level, body.step) {
        (Some(l), _) => conn.set_dim(l).await?,
        (None, true) => conn.step_dim().await?,
        _ => {
            return Err(AppError::InvalidParameter(
                "Provide either 'level' (0-3) or 'step': true".into(),
            ))
        }
    };
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
    info!("HTTP POST /menu action={:?}", body.action);
    let mut conn = state.get_serial().await?;
    match body.action {
        MenuAction::Enter => conn.menu_enter().await?,
        MenuAction::Exit => conn.menu_exit().await?,
        MenuAction::Up => conn.menu_nav(ir_remote::STEP_UP).await?,
        MenuAction::Down => conn.menu_nav(ir_remote::STEP_DOWN).await?,
        MenuAction::Right => conn.menu_nav(ir_remote::ARROW_RIGHT).await?,
        MenuAction::Left => conn.menu_nav(ir_remote::ARROW_LEFT).await?,
    }
    Ok(Json(OkResponse { ok: true }))
}

fn ir_source_str(back: bool) -> &'static str {
    if back {
        "back"
    } else {
        "front"
    }
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
    info!("HTTP GET /ir_input");
    let mut conn = state.get_serial().await?;
    Ok(Json(IrInputResponse {
        source: ir_source_str(conn.get_ir_input().await?).into(),
    }))
}

async fn set_ir_input(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IrInputRequest>,
) -> ApiResult<Json<IrInputResponse>> {
    info!("HTTP POST /ir_input source={:?}", body.source);
    let mut conn = state.get_serial().await?;
    let back = match body.source {
        IrSource::Back => conn.set_ir_input(true).await?,
        IrSource::Front => conn.set_ir_input(false).await?,
    };
    Ok(Json(IrInputResponse {
        source: ir_source_str(back).into(),
    }))
}

// ---- Info ----

#[derive(Serialize)]
pub struct InfoResponse {
    pub product_line: String,
    pub model: String,
    pub firmware: String,
}

async fn get_info(State(state): State<Arc<AppState>>) -> ApiResult<Json<InfoResponse>> {
    info!("HTTP GET /info");
    let mut conn = state.get_serial().await?;
    Ok(Json(InfoResponse {
        product_line: conn.get_product_line().await?,
        model: conn.get_model_name().await?,
        firmware: conn.get_version().await?,
    }))
}

// ---- Input names ----

#[derive(Serialize)]
pub struct InputNameResponse {
    pub name: String,
}

async fn get_current_input_name(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<InputNameResponse>> {
    info!("HTTP GET /input/current/name");
    let mut conn = state.get_serial().await?;
    Ok(Json(InputNameResponse {
        name: conn.get_input_name_current().await?,
    }))
}

async fn get_input_name(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u8>,
) -> ApiResult<Json<InputNameResponse>> {
    info!("HTTP GET /input/{}/name", id);
    let mut conn = state.get_serial().await?;
    Ok(Json(InputNameResponse {
        name: conn.get_input_name(id).await?,
    }))
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
    info!("HTTP POST /factory_reset confirm={}", body.confirm);
    if !body.confirm {
        return Err(AppError::InvalidParameter(
            "Set 'confirm': true to proceed with factory reset".into(),
        ));
    }
    let mut conn = state.get_serial().await?;
    conn.factory_reset().await?;
    drop(conn); // release mutex before disconnect
                // Always disconnect: verbose goes off after reset (spec), reconnect re-enables it.
    state.disconnect().await;
    Ok(Json(OkResponse { ok: true }))
}
