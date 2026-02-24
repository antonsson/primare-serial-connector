mod error;
mod protocol;
mod serial;
mod api;
mod state;

use std::sync::Arc;
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "primare-i22-rs232")]
#[command(about = "REST API bridge for Primare I22 amplifier via RS232")]
struct Args {
    /// Serial port device (e.g. /dev/ttyUSB0)
    #[arg(short, long, env = "PRIMARE_PORT", default_value = "/dev/ttyUSB0")]
    port: String,

    /// Baud rate (I22 uses 4800)
    #[arg(short, long, env = "PRIMARE_BAUD", default_value_t = 4800)]
    baud: u32,

    /// Listen address
    #[arg(short, long, env = "PRIMARE_LISTEN", default_value = "0.0.0.0:3000")]
    listen: String,

    /// Response timeout in milliseconds
    #[arg(short, long, env = "PRIMARE_TIMEOUT_MS", default_value_t = 500)]
    timeout_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "primare_i22_rs232=info".into()),
        )
        .init();

    let args = Args::parse();

    info!(
        "Starting server (serial port: {}, baud: {})",
        args.port, args.baud
    );

    // Create state with connection config - connection will be established on first request
    let state = Arc::new(AppState::new(args.port, args.baud, args.timeout_ms));

    let app = Router::new()
        .merge(api::routes())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    info!("Listening on http://{}", args.listen);
    info!("Serial connection will be established on first request");

    axum::serve(listener, app).await?;
    Ok(())
}
