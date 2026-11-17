use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, WriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_serial::{SerialPortBuilderExt, SerialStream};
use tracing::{debug, info, trace, warn};

use crate::commands::{
    CommandSpec, BALANCE_GET, BALANCE_SET, BALANCE_STEP, DIM_GET, DIM_SET, DIM_STEP, FACTORY_RESET,
    INPUT_GET, INPUT_NAME_BY_ID_READ, INPUT_NAME_READ, INPUT_SET, INPUT_STEP, IR_INPUT_GET,
    IR_INPUT_SET, MENU_ENTER, MENU_EXIT, MENU_NAV, MODEL_NAME_READ, MUTE_GET, MUTE_SET,
    MUTE_TOGGLE, POWER_OFF, POWER_ON, POWER_TOGGLE, PRODUCT_LINE_READ, VERBOSE_ON, VERSION_READ,
    VOLUME_GET, VOLUME_SET, VOLUME_STEP,
};
use crate::error::{ApiResult, AppError};
use crate::protocol::{self, var, Reply};

/// Last known amp state, built from every frame the amp sends: replies to our
/// requests and the verbose frames it pushes on IR remote / front-panel changes.
/// None = not seen since ampd started.
#[derive(Debug, Default, Clone)]
pub struct AmpCache {
    pub power: Option<bool>,
    pub volume: Option<u8>,
    pub input: Option<u8>,
    pub mute: Option<bool>,
    pub balance: Option<i8>,
    pub dim: Option<u8>,
}

/// Shared between the reader task and the API; outlives reconnects.
pub type SharedCache = Arc<Mutex<AmpCache>>;

fn update<T: PartialEq + std::fmt::Debug>(slot: &mut Option<T>, value: T, name: &str) {
    if slot.as_ref() != Some(&value) {
        info!("Amp {}: {:?}", name, value);
        *slot = Some(value);
    }
}

impl AmpCache {
    fn apply(&mut self, reply: &Reply) {
        let Some(v) = reply.value() else { return };
        // In standby the amp only answers power commands, so any other frame
        // means it's on.
        if reply.variable != var::STANDBY {
            update(&mut self.power, true, "power");
        }
        match reply.variable {
            var::STANDBY => update(&mut self.power, v == 0x01, "power"),
            var::VOLUME => update(&mut self.volume, v, "volume"),
            var::INPUT => update(&mut self.input, v, "input"),
            var::MUTE => update(&mut self.mute, v == 0x01, "mute"),
            var::BALANCE => update(&mut self.balance, v as i8, "balance"),
            var::DIM => update(&mut self.dim, v, "dim"),
            _ => {}
        }
    }
}

pub struct SerialConnection {
    writer: WriteHalf<SerialStream>,
    /// Every parsed frame from the reader task, in arrival order.
    frames: mpsc::UnboundedReceiver<Reply>,
    reader: JoinHandle<()>,
    timeout: Duration,
    dead: Arc<AtomicBool>,
    cache: SharedCache,
    /// Verbose confirmed by the amp. Not answered in standby, so retried.
    verbose_ok: bool,
}

impl Drop for SerialConnection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// Read bytes until we see an unescaped DLE ETX (0x10 0x03).
async fn read_frame<R: AsyncRead + Unpin>(port: &mut R) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut prev_was_dle = false;

    loop {
        let b = port.read_u8().await?;
        buf.push(b);

        if prev_was_dle {
            if b == protocol::ETX {
                return Ok(buf);
            }
            // DLE DLE is an escaped DLE data byte — reset state so the
            // next byte is treated fresh, not as "after a DLE".
            prev_was_dle = false;
        } else {
            prev_was_dle = b == protocol::DLE;
        }
    }
}

/// Reads the port continuously: updates the cache from every frame and hands
/// frames to whoever is waiting for a reply. Exits (marking the connection
/// dead) on I/O error.
async fn reader_task<R: AsyncRead + Unpin>(
    mut port: R,
    tx: mpsc::UnboundedSender<Reply>,
    cache: SharedCache,
    dead: Arc<AtomicBool>,
) {
    loop {
        let raw = match read_frame(&mut port).await {
            Ok(raw) => raw,
            Err(e) => {
                warn!("Serial read failed: {}", e);
                dead.store(true, Ordering::Relaxed);
                return;
            }
        };
        trace!("RX: {:02X?}", raw);
        let Some(reply) = protocol::parse_reply(&raw) else {
            debug!("Unparseable frame: {:02X?}", raw);
            continue;
        };
        debug!(
            "RX var=0x{:02X} values={:02X?}",
            reply.variable, reply.values
        );
        cache.lock().unwrap().apply(&reply);
        if tx.send(reply).is_err() {
            return; // connection dropped
        }
    }
}

impl SerialConnection {
    pub fn open(path: &str, baud: u32, timeout_ms: u64, cache: SharedCache) -> ApiResult<Self> {
        let port = tokio_serial::new(path, baud)
            .data_bits(tokio_serial::DataBits::Eight)
            .stop_bits(tokio_serial::StopBits::One)
            .parity(tokio_serial::Parity::None)
            .open_native_async()?;

        let (rd, writer) = tokio::io::split(port);
        let (tx, frames) = mpsc::unbounded_channel();
        let dead = Arc::new(AtomicBool::new(false));
        let reader = tokio::spawn(reader_task(rd, tx, cache.clone(), dead.clone()));

        Ok(Self {
            writer,
            frames,
            reader,
            timeout: Duration::from_millis(timeout_ms),
            dead,
            cache,
            verbose_ok: false,
        })
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }

    fn set_power_cache(&self, on: bool) {
        update(&mut self.cache.lock().unwrap().power, on, "power");
    }

    async fn write(&mut self, frame: &[u8]) -> ApiResult<()> {
        trace!("TX: {:02X?}", frame);
        if let Err(e) = self.writer.write_all(frame).await {
            self.dead.store(true, Ordering::Relaxed);
            return Err(e.into());
        }
        Ok(())
    }

    /// Send a raw frame and wait for its reply. Unsolicited frames that arrive
    /// meanwhile are skipped (the reader task has already cached them).
    pub async fn send_recv(&mut self, frame: &[u8]) -> ApiResult<Reply> {
        // Drop frames nobody waited for (already applied to the cache).
        while self.frames.try_recv().is_ok() {}

        self.write(frame).await?;

        let request_var = protocol::request_variable(frame);
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            let reply = match tokio::time::timeout_at(deadline, self.frames.recv()).await {
                Ok(Some(reply)) => reply,
                Ok(None) => {
                    self.dead.store(true, Ordering::Relaxed);
                    return Err(AppError::Io(std::io::Error::other("serial reader stopped")));
                }
                Err(_) => {
                    // No reply: the amp is in standby (it only answers power
                    // commands there). set_power overrides this for its own case.
                    self.set_power_cache(false);
                    return Err(AppError::Timeout);
                }
            };
            match request_var {
                Some(v) if !protocol::reply_matches(v, &reply) => {
                    debug!("Skipping unsolicited frame var=0x{:02X}", reply.variable);
                }
                _ => return Ok(reply),
            }
        }
    }

    /// Send a frame and discard the reply (e.g. factory reset).
    pub async fn send_only(&mut self, frame: &[u8]) -> ApiResult<()> {
        self.write(frame).await
    }

    // ---- Private helpers ----

    /// Encode the step direction as the relative value the protocol expects:
    /// up = 0x01 (increase by 1), down = 0xFF (-1 as u8).
    fn step_byte(up: bool) -> u8 {
        if up {
            0x01
        } else {
            0xFF
        }
    }

    async fn send_command(&mut self, command: CommandSpec) -> ApiResult<Reply> {
        let frame = command.frame(None);
        self.send_recv(&frame).await
    }

    async fn send_command_value(&mut self, command: CommandSpec, value: u8) -> ApiResult<Reply> {
        let frame = command.frame(Some(value));
        self.send_recv(&frame).await
    }

    /// Read a variable's current value via command list.
    async fn read_value(&mut self, command: CommandSpec) -> ApiResult<u8> {
        let reply = self.send_command(command).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    /// Write a value via command list, returning the confirmed value.
    async fn write_value(&mut self, command: CommandSpec, value: u8) -> ApiResult<u8> {
        let reply = self.send_command_value(command, value).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    /// Read a text (ASCII) variable using CMD_READ, returning the decoded string.
    async fn read_text(&mut self, command: CommandSpec) -> ApiResult<String> {
        let reply = self.send_command(command).await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // ---- Higher-level commands ----

    pub fn verbose_ok(&self) -> bool {
        self.verbose_ok
    }

    pub async fn enable_verbose(&mut self) -> ApiResult<()> {
        match self.send_command(VERBOSE_ON).await {
            Ok(reply) => {
                self.verbose_ok = true;
                debug!(
                    "Verbose mode active (confirmed 0x{:02X})",
                    reply.value().unwrap_or(0)
                );
                Ok(())
            }
            Err(AppError::Timeout) => {
                // No reply: amp in standby. AppState::get_serial retries
                // until confirmed.
                warn!("Verbose enable timed out (amp in standby or unresponsive)");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    // --- Power ---
    // Power can't be read without toggling it. It's inferred: any non-power
    // reply means on, a timeout means standby (see AmpCache::apply).

    /// Cached power state (power replies, standby pushes, reply/timeout inference).
    /// `false` if not seen since ampd started.
    pub fn get_power(&self) -> bool {
        self.cache.lock().unwrap().power.unwrap_or(false)
    }

    pub async fn set_power(&mut self, on: bool) -> ApiResult<bool> {
        match self
            .send_command(if on { POWER_ON } else { POWER_OFF })
            .await
        {
            Ok(reply) => Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01),
            Err(AppError::Timeout) => {
                // Timeout likely means device is already in the requested state
                self.set_power_cache(on);
                Ok(on)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn toggle_power(&mut self) -> ApiResult<bool> {
        let reply = self.send_command(POWER_TOGGLE).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Volume ---

    pub async fn get_volume(&mut self) -> ApiResult<u8> {
        self.read_value(VOLUME_GET).await
    }

    pub async fn set_volume(&mut self, level: u8) -> ApiResult<u8> {
        if level > 79 {
            return Err(AppError::InvalidParameter("Volume must be 0-79".into()));
        }
        self.write_value(VOLUME_SET, level).await
    }

    pub async fn step_volume(&mut self, up: bool) -> ApiResult<u8> {
        let reply = self
            .send_command_value(VOLUME_STEP, Self::step_byte(up))
            .await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Input ---

    pub async fn get_input(&mut self) -> ApiResult<u8> {
        self.read_value(INPUT_GET).await
    }

    pub async fn set_input(&mut self, input: u8) -> ApiResult<u8> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        self.write_value(INPUT_SET, input).await
    }

    pub async fn step_input(&mut self, up: bool) -> ApiResult<u8> {
        let reply = self
            .send_command_value(INPUT_STEP, Self::step_byte(up))
            .await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Mute ---

    pub async fn get_mute(&mut self) -> ApiResult<bool> {
        Ok(self.read_value(MUTE_GET).await? == 0x01)
    }

    pub async fn set_mute(&mut self, muted: bool) -> ApiResult<bool> {
        let value = if muted { 0x01 } else { 0x00 };
        Ok(self.write_value(MUTE_SET, value).await? == 0x01)
    }

    pub async fn toggle_mute(&mut self) -> ApiResult<bool> {
        let reply = self.send_command(MUTE_TOGGLE).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Balance ---

    /// Returns balance as i8 (-9 to +9).
    pub async fn get_balance(&mut self) -> ApiResult<i8> {
        Ok(self.read_value(BALANCE_GET).await? as i8)
    }

    pub async fn set_balance(&mut self, value: i8) -> ApiResult<i8> {
        if !(-9..=9).contains(&value) {
            return Err(AppError::InvalidParameter("Balance must be -9 to 9".into()));
        }
        Ok(self.write_value(BALANCE_SET, value as u8).await? as i8)
    }

    pub async fn step_balance(&mut self, steps: i8) -> ApiResult<i8> {
        let reply = self.send_command_value(BALANCE_STEP, steps as u8).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? as i8)
    }

    // --- Dim ---

    pub async fn get_dim(&mut self) -> ApiResult<u8> {
        self.read_value(DIM_GET).await
    }

    pub async fn set_dim(&mut self, level: u8) -> ApiResult<u8> {
        if level > 3 {
            return Err(AppError::InvalidParameter("Dim level must be 0-3".into()));
        }
        self.write_value(DIM_SET, level).await
    }

    pub async fn step_dim(&mut self) -> ApiResult<u8> {
        let reply = self.send_command(DIM_STEP).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Menu ---

    pub async fn menu_enter(&mut self) -> ApiResult<()> {
        self.send_command(MENU_ENTER).await?;
        Ok(())
    }

    pub async fn menu_exit(&mut self) -> ApiResult<()> {
        self.send_command(MENU_EXIT).await?;
        Ok(())
    }

    pub async fn menu_nav(&mut self, remote_value: u8) -> ApiResult<()> {
        self.send_command_value(MENU_NAV, remote_value).await?;
        Ok(())
    }

    // --- IR Input ---

    pub async fn get_ir_input(&mut self) -> ApiResult<bool> {
        // 0x00 = front, any other = back
        Ok(self.read_value(IR_INPUT_GET).await? != 0x00)
    }

    pub async fn set_ir_input(&mut self, back: bool) -> ApiResult<bool> {
        let value = if back { 0x01 } else { 0x00 };
        Ok(self.write_value(IR_INPUT_SET, value).await? != 0x00)
    }

    // --- Info ---

    pub async fn get_product_line(&mut self) -> ApiResult<String> {
        self.read_text(PRODUCT_LINE_READ).await
    }

    pub async fn get_model_name(&mut self) -> ApiResult<String> {
        self.read_text(MODEL_NAME_READ).await
    }

    pub async fn get_version(&mut self) -> ApiResult<String> {
        self.read_text(VERSION_READ).await
    }

    pub async fn get_input_name_current(&mut self) -> ApiResult<String> {
        self.read_text(INPUT_NAME_READ).await
    }

    pub async fn get_input_name(&mut self, input: u8) -> ApiResult<String> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        // Firmware quirk: the device returns the PREVIOUSLY queried input's name,
        // not the current one. Query twice and use the second response.
        // Note: uses 1-indexed values (0x01-0x07), as 0x00 means "current input".
        let _ = self
            .send_command_value(INPUT_NAME_BY_ID_READ, input)
            .await?;
        let reply = self
            .send_command_value(INPUT_NAME_BY_ID_READ, input)
            .await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // --- Factory reset ---

    pub async fn factory_reset(&mut self) -> ApiResult<()> {
        let frame = FACTORY_RESET.frame(None);
        // No reply expected; device turns verbose off after reset.
        self.send_only(&frame).await
    }
}
