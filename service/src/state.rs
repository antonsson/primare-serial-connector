use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::error::ApiResult;
use crate::serial::SerialConnection;

pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
    pub timeout_ms: u64,
}

pub struct AppState {
    pub config: SerialConfig,
    pub connection: Mutex<Option<SerialConnection>>,
}

impl AppState {
    pub fn new(port: String, baud: u32, timeout_ms: u64) -> Self {
        Self {
            config: SerialConfig { port, baud, timeout_ms },
            connection: Mutex::new(None),
        }
    }

    /// Check if currently connected (without trying to connect)
    pub async fn is_connected(&self) -> bool {
        self.connection.lock().await.is_some()
    }

    /// Get the serial connection, connecting if necessary.
    /// Returns a guard that holds the mutex lock.
    pub async fn get_serial(&self) -> ApiResult<SerialGuard<'_>> {
        let mut guard = self.connection.lock().await;

        if guard.is_none() {
            info!("Connecting to serial port {}...", self.config.port);
            match SerialConnection::open(&self.config.port, self.config.baud, self.config.timeout_ms) {
                Ok(mut serial) => {
                    // Enable verbose mode so commands return state
                    if let Err(e) = serial.enable_verbose().await {
                        warn!("Failed to enable verbose mode: {}", e);
                    }
                    info!("Serial connection established");
                    *guard = Some(serial);
                }
                Err(e) => {
                    warn!("Failed to connect to serial port: {}", e);
                    return Err(e);
                }
            }
        }

        Ok(SerialGuard(guard))
    }

    /// Mark connection as failed (will reconnect on next request)
    pub async fn disconnect(&self) {
        let mut conn = self.connection.lock().await;
        if conn.is_some() {
            warn!("Disconnecting serial port");
            *conn = None;
        }
    }
}

/// A wrapper that provides DerefMut to the inner SerialConnection
pub struct SerialGuard<'a>(tokio::sync::MutexGuard<'a, Option<SerialConnection>>);

impl<'a> std::ops::Deref for SerialGuard<'a> {
    type Target = SerialConnection;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("SerialGuard created without connection")
    }
}

impl<'a> std::ops::DerefMut for SerialGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("SerialGuard created without connection")
    }
}
