use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, trace, warn};

use crate::commands::{
    CommandSpec, BALANCE_GET, BALANCE_SET, BALANCE_STEP, DIM_GET, DIM_SET, DIM_STEP, FACTORY_RESET,
    INPUT_GET, INPUT_NAME_BY_ID_READ, INPUT_NAME_READ, INPUT_SET, INPUT_STEP, IR_INPUT_GET,
    IR_INPUT_SET, MENU_ENTER, MENU_EXIT, MENU_NAV, MODEL_NAME_READ, MUTE_GET, MUTE_SET,
    MUTE_TOGGLE, POWER_OFF, POWER_ON, POWER_TOGGLE, PRODUCT_LINE_READ,
    VERBOSE_ON, VERSION_READ, VOLUME_GET, VOLUME_SET, VOLUME_STEP,
};
use crate::error::{ApiResult, AppError};
use crate::protocol::{self, Reply};

pub struct SerialConnection {
    port: tokio_serial::SerialStream,
    timeout: Duration,
    dead: bool,
    /// Cached power state. None = unknown (not yet determined via toggle).
    power_state: Option<bool>,
}

impl SerialConnection {
    pub fn open(path: &str, baud: u32, timeout_ms: u64) -> ApiResult<Self> {
        let port = tokio_serial::new(path, baud)
            .data_bits(tokio_serial::DataBits::Eight)
            .stop_bits(tokio_serial::StopBits::One)
            .parity(tokio_serial::Parity::None)
            .open_native_async()?;

        Ok(Self {
            port,
            timeout: Duration::from_millis(timeout_ms),
            dead: false,
            power_state: None,
        })
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// Drain any pending data from the serial buffer.
    async fn drain_buffer(&mut self) {
        let mut buf = [0u8; 64];
        loop {
            match timeout(Duration::from_millis(10), self.port.read(&mut buf)).await {
                Ok(Ok(0)) | Err(_) => break, // No data or timeout
                Ok(Ok(n)) => {
                    trace!("Drained {} bytes: {:02X?}", n, &buf[..n]);
                }
                Ok(Err(_)) => break,
            }
        }
    }

    /// Send a raw frame and read back one reply frame.
    pub async fn send_recv(&mut self, frame: &[u8]) -> ApiResult<Reply> {
        // Drain any stale data from buffer first
        self.drain_buffer().await;

        trace!("TX: {:02X?}", frame);

        if let Err(e) = self.port.write_all(frame).await {
            self.dead = true;
            return Err(e.into());
        }

        let reply_bytes = match timeout(self.timeout, self.read_frame()).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                self.dead = true;
                return Err(e);
            }
            Err(_) => return Err(AppError::Timeout),
        };

        trace!("RX: {:02X?}", reply_bytes);

        let reply = protocol::parse_reply(&reply_bytes).ok_or(AppError::InvalidReply)?;
        debug!(
            "RX var=0x{:02X} values={:02X?}",
            reply.variable, reply.values
        );
        Ok(reply)
    }

    /// Send a frame and discard the reply (e.g. factory reset).
    pub async fn send_only(&mut self, frame: &[u8]) -> ApiResult<()> {
        trace!("TX (no reply): {:02X?}", frame);
        if let Err(e) = self.port.write_all(frame).await {
            self.dead = true;
            return Err(e.into());
        }
        Ok(())
    }

    /// Read bytes until we see an unescaped DLE ETX (0x10 0x03).
    async fn read_frame(&mut self) -> ApiResult<Vec<u8>> {
        let mut buf = Vec::new();
        let mut prev_was_dle = false;

        loop {
            let mut byte = [0u8; 1];
            self.port.read_exact(&mut byte).await?;
            let b = byte[0];

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
    /// Timeout indicates the amp is likely off.
    async fn read_value(&mut self, command: CommandSpec) -> ApiResult<u8> {
        match self.send_command(command).await {
            Ok(reply) => reply.value().ok_or(AppError::InvalidReply),
            Err(AppError::Timeout) => {
                debug!("Read timed out, assuming amp is off");
                self.power_state = Some(false);
                Err(AppError::Timeout)
            }
            Err(e) => Err(e),
        }
    }

    /// Write a value via command list, returning the confirmed value.
    /// Timeout indicates the amp is likely off.
    async fn write_value(&mut self, command: CommandSpec, value: u8) -> ApiResult<u8> {
        match self.send_command_value(command, value).await {
            Ok(reply) => reply.value().ok_or(AppError::InvalidReply),
            Err(AppError::Timeout) => {
                debug!("Write timed out, assuming amp is off");
                self.power_state = Some(false);
                Err(AppError::Timeout)
            }
            Err(e) => Err(e),
        }
    }

    /// Read a text (ASCII) variable using CMD_READ, returning the decoded string.
    /// Timeout indicates the amp is likely off.
    async fn read_text(&mut self, command: CommandSpec) -> ApiResult<String> {
        match self.send_command(command).await {
            Ok(reply) => Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned()),
            Err(AppError::Timeout) => {
                debug!("Read text timed out, assuming amp is off");
                self.power_state = Some(false);
                Err(AppError::Timeout)
            }
            Err(e) => Err(e),
        }
    }

    // ---- Higher-level commands ----

    pub async fn enable_verbose(&mut self) -> ApiResult<()> {
        match self.send_command(VERBOSE_ON).await {
            Ok(reply) => {
                debug!(
                    "Verbose mode active (confirmed 0x{:02X})",
                    reply.value().unwrap_or(0)
                );
                Ok(())
            }
            Err(AppError::Timeout) => {
                // Device did not reply — likely in standby or not yet ready.
                // All state-read commands require verbose=1 to get a reply,
                // so subsequent requests will also timeout, triggering a
                // reconnect that will re-attempt enable_verbose.
                warn!("Verbose enable timed out (device in standby or unresponsive)");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    // --- Power ---
    // Power state is cached from the last power operation response.

    /// Returns cached power state. Returns `false` if unknown (no power operation yet).
    pub fn get_power(&self) -> bool {
        self.power_state.unwrap_or(false)
    }

    pub async fn set_power(&mut self, on: bool) -> ApiResult<bool> {
        match self.send_command(if on { POWER_ON } else { POWER_OFF }).await {
            Ok(reply) => {
                let state = reply.value().ok_or(AppError::InvalidReply)? == 0x01;
                self.power_state = Some(state);
                Ok(state)
            }
            Err(AppError::Timeout) => {
                // Timeout likely means device is already in the requested state
                self.power_state = Some(on);
                Ok(on)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn toggle_power(&mut self) -> ApiResult<bool> {
        let reply = self.send_command(POWER_TOGGLE).await?;
        let state = reply.value().ok_or(AppError::InvalidReply)? == 0x01;
        self.power_state = Some(state);
        Ok(state)
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
        let _ = self.send_command_value(INPUT_NAME_BY_ID_READ, input).await?;
        let reply = self.send_command_value(INPUT_NAME_BY_ID_READ, input).await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // --- Factory reset ---

    pub async fn factory_reset(&mut self) -> ApiResult<()> {
        let frame = FACTORY_RESET.frame(None);
        // No reply expected; device turns verbose off after reset.
        self.send_only(&frame).await
    }
}
