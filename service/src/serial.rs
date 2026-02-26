use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, trace, warn};

use crate::commands::{
    CommandSpec, FACTORY_RESET, GET_BALANCE, GET_DIM, GET_INPUT, GET_INPUT_NAME_BY_ID,
    GET_INPUT_NAME_CURRENT, GET_IR_INPUT, GET_MODEL_NAME, GET_MUTE, GET_POWER, GET_PRODUCT_LINE,
    GET_VERSION, GET_VOLUME, MENU_ENTER, MENU_EXIT, MENU_NAV, SET_BALANCE, SET_DIM, SET_INPUT,
    SET_IR_INPUT, SET_MUTE, SET_POWER_OFF, SET_POWER_ON, SET_VOLUME, STEP_BALANCE, STEP_DIM,
    STEP_INPUT, STEP_VOLUME, TOGGLE_MUTE, TOGGLE_POWER, VERBOSE_OFF, VERBOSE_ON,
};
use crate::error::{AppError, ApiResult};
use crate::protocol::{self, Reply};

pub struct SerialConnection {
    port: tokio_serial::SerialStream,
    timeout: Duration,
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
        })
    }

    /// Send a raw frame and read back one reply frame.
    pub async fn send_recv(&mut self, frame: &[u8]) -> ApiResult<Reply> {
        trace!("TX: {:02X?}", frame);
        self.port.write_all(frame).await?;

        let reply_bytes = timeout(self.timeout, self.read_frame())
            .await
            .map_err(|_| AppError::Timeout)??;

        trace!("RX: {:02X?}", reply_bytes);

        let reply = protocol::parse_reply(&reply_bytes).ok_or(AppError::InvalidReply)?;
        debug!("RX var=0x{:02X} values={:02X?}", reply.variable, reply.values);
        Ok(reply)
    }

    /// Send a frame and discard the reply (e.g. factory reset).
    pub async fn send_only(&mut self, frame: &[u8]) -> ApiResult<()> {
        trace!("TX (no reply): {:02X?}", frame);
        self.port.write_all(frame).await?;
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
        if up { 0x01 } else { 0xFF }
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
        self.enable_verbose().await?;
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

    pub async fn enable_verbose(&mut self) -> ApiResult<()> {
        match self.send_command(VERBOSE_ON).await {
            Ok(reply) => {
                debug!("Verbose mode active (confirmed 0x{:02X})", reply.value().unwrap_or(0));
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

    pub async fn disable_verbose(&mut self) -> ApiResult<()> {
        match self.send_command(VERBOSE_OFF).await {
            Ok(_) | Err(AppError::Timeout) => Ok(()),
            Err(e) => Err(e),
        }
    }

    // --- Power ---

    pub async fn get_power(&mut self) -> ApiResult<bool> {
        // Standby var: 0x00 = standby (off), 0x01 = operate (on)
        match self.read_value(GET_POWER).await {
            Ok(v) => Ok(v == 0x01),
            Err(AppError::Timeout) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn set_power(&mut self, on: bool) -> ApiResult<bool> {
        // Power set should be sent with verbose disabled.
        self.disable_verbose().await?;
        let reply = self
            .send_command(if on { SET_POWER_ON } else { SET_POWER_OFF })
            .await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    pub async fn toggle_power(&mut self) -> ApiResult<bool> {
        let reply = self.send_command(TOGGLE_POWER).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Volume ---

    pub async fn get_volume(&mut self) -> ApiResult<u8> {
        self.read_value(GET_VOLUME).await
    }

    pub async fn set_volume(&mut self, level: u8) -> ApiResult<u8> {
        if level > 79 {
            return Err(AppError::InvalidParameter("Volume must be 0-79".into()));
        }
        self.write_value(SET_VOLUME, level).await
    }

    pub async fn step_volume(&mut self, up: bool) -> ApiResult<u8> {
        let reply = self.send_command_value(STEP_VOLUME, Self::step_byte(up)).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Input ---

    pub async fn get_input(&mut self) -> ApiResult<u8> {
        self.read_value(GET_INPUT).await
    }

    pub async fn set_input(&mut self, input: u8) -> ApiResult<u8> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        self.write_value(SET_INPUT, input).await
    }

    pub async fn step_input(&mut self, up: bool) -> ApiResult<u8> {
        let reply = self.send_command_value(STEP_INPUT, Self::step_byte(up)).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Mute ---

    pub async fn get_mute(&mut self) -> ApiResult<bool> {
        Ok(self.read_value(GET_MUTE).await? == 0x01)
    }

    pub async fn set_mute(&mut self, muted: bool) -> ApiResult<bool> {
        let value = if muted { 0x01 } else { 0x00 };
        Ok(self.write_value(SET_MUTE, value).await? == 0x01)
    }

    pub async fn toggle_mute(&mut self) -> ApiResult<bool> {
        let reply = self.send_command(TOGGLE_MUTE).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Balance ---

    /// Returns balance as i8 (-9 to +9).
    pub async fn get_balance(&mut self) -> ApiResult<i8> {
        Ok(self.read_value(GET_BALANCE).await? as i8)
    }

    pub async fn set_balance(&mut self, value: i8) -> ApiResult<i8> {
        if !(-9..=9).contains(&value) {
            return Err(AppError::InvalidParameter("Balance must be -9 to 9".into()));
        }
        Ok(self.write_value(SET_BALANCE, value as u8).await? as i8)
    }

    pub async fn step_balance(&mut self, steps: i8) -> ApiResult<i8> {
        let reply = self.send_command_value(STEP_BALANCE, steps as u8).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? as i8)
    }

    // --- Dim ---

    pub async fn get_dim(&mut self) -> ApiResult<u8> {
        self.read_value(GET_DIM).await
    }

    pub async fn set_dim(&mut self, level: u8) -> ApiResult<u8> {
        if level > 3 {
            return Err(AppError::InvalidParameter("Dim level must be 0-3".into()));
        }
        self.write_value(SET_DIM, level).await
    }

    pub async fn step_dim(&mut self) -> ApiResult<u8> {
        let reply = self.send_command(STEP_DIM).await?;
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
        Ok(self.read_value(GET_IR_INPUT).await? != 0x00)
    }

    pub async fn set_ir_input(&mut self, back: bool) -> ApiResult<bool> {
        let value = if back { 0x01 } else { 0x00 };
        Ok(self.write_value(SET_IR_INPUT, value).await? != 0x00)
    }

    // --- Info ---

    pub async fn get_product_line(&mut self) -> ApiResult<String> {
        self.read_text(GET_PRODUCT_LINE).await
    }

    pub async fn get_model_name(&mut self) -> ApiResult<String> {
        self.read_text(GET_MODEL_NAME).await
    }

    pub async fn get_version(&mut self) -> ApiResult<String> {
        self.read_text(GET_VERSION).await
    }

    pub async fn get_input_name_current(&mut self) -> ApiResult<String> {
        self.read_text(GET_INPUT_NAME_CURRENT).await
    }

    pub async fn get_input_name(&mut self, input: u8) -> ApiResult<String> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        let reply = self.send_command_value(GET_INPUT_NAME_BY_ID, input).await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // --- Factory reset ---

    pub async fn factory_reset(&mut self) -> ApiResult<()> {
        let frame = FACTORY_RESET.frame(None);
        // No reply expected; device turns verbose off after reset.
        self.send_only(&frame).await
    }
}
