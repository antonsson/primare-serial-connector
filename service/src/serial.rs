use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, trace, warn};

use crate::error::{AppError, ApiResult};
use crate::protocol::{self, Reply, var, direct, build_write, build_read, build_frame, CMD_READ};

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

    /// Read a variable's current value via direct mode.
    /// Sends value 0x00 which, per protocol spec, replies with the current
    /// value without changing state when verbose mode is active.
    async fn read_direct(&mut self, var: u8) -> ApiResult<u8> {
        let frame = build_write(direct(var), Some(0x00));
        let reply = self.send_recv(&frame).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    /// Write a value to a variable in direct mode, returning the confirmed value.
    async fn write_direct(&mut self, var: u8, value: u8) -> ApiResult<u8> {
        let frame = build_write(direct(var), Some(value));
        let reply = self.send_recv(&frame).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    /// Read a text (ASCII) variable using CMD_READ, returning the decoded string.
    async fn read_text(&mut self, variable: u8) -> ApiResult<String> {
        let frame = build_read(variable);
        let reply = self.send_recv(&frame).await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // ---- Higher-level commands ----

    pub async fn enable_verbose(&mut self) -> ApiResult<()> {
        let frame = build_write(direct(var::VERBOSE), Some(0x01));
        match self.send_recv(&frame).await {
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

    // --- Power ---

    pub async fn get_power(&mut self) -> ApiResult<bool> {
        // Standby var: 0x00 = standby (off), 0x01 = operate (on)
        Ok(self.read_direct(var::STANDBY).await? == 0x01)
    }

    pub async fn set_power(&mut self, on: bool) -> ApiResult<bool> {
        // Use IR "Operate/Standby (unique)" commands to unambiguously force
        // state — avoids the 0x00 collision between "set standby" and the
        // protocol's "reply current value without change" sentinel.
        // IR value 0x02 = Operate only, 0x01 = Standby only (Part 1 of spec).
        let ir_value = if on { 0x02u8 } else { 0x01u8 };
        let frame = build_write(var::REMOTE, Some(ir_value));
        let reply = self.send_recv(&frame).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    pub async fn toggle_power(&mut self) -> ApiResult<bool> {
        let frame = build_write(var::STANDBY, Some(0x00));
        let reply = self.send_recv(&frame).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Volume ---

    pub async fn get_volume(&mut self) -> ApiResult<u8> {
        self.read_direct(var::VOLUME).await
    }

    pub async fn set_volume(&mut self, level: u8) -> ApiResult<u8> {
        if level > 79 {
            return Err(AppError::InvalidParameter("Volume must be 0-79".into()));
        }
        self.write_direct(var::VOLUME, level).await
    }

    pub async fn step_volume(&mut self, up: bool) -> ApiResult<u8> {
        let frame = build_write(var::VOLUME, Some(Self::step_byte(up)));
        let reply = self.send_recv(&frame).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Input ---

    pub async fn get_input(&mut self) -> ApiResult<u8> {
        self.read_direct(var::INPUT).await
    }

    pub async fn set_input(&mut self, input: u8) -> ApiResult<u8> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        self.write_direct(var::INPUT, input).await
    }

    pub async fn step_input(&mut self, up: bool) -> ApiResult<u8> {
        let frame = build_write(var::INPUT, Some(Self::step_byte(up)));
        let reply = self.send_recv(&frame).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Mute ---

    pub async fn get_mute(&mut self) -> ApiResult<bool> {
        Ok(self.read_direct(var::MUTE).await? == 0x01)
    }

    pub async fn set_mute(&mut self, muted: bool) -> ApiResult<bool> {
        let value = if muted { 0x01 } else { 0x00 };
        Ok(self.write_direct(var::MUTE, value).await? == 0x01)
    }

    pub async fn toggle_mute(&mut self) -> ApiResult<bool> {
        let frame = build_write(var::MUTE, Some(0x00));
        let reply = self.send_recv(&frame).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? == 0x01)
    }

    // --- Balance ---

    /// Returns balance as i8 (-9 to +9).
    pub async fn get_balance(&mut self) -> ApiResult<i8> {
        Ok(self.read_direct(var::BALANCE).await? as i8)
    }

    pub async fn set_balance(&mut self, value: i8) -> ApiResult<i8> {
        if !(-9..=9).contains(&value) {
            return Err(AppError::InvalidParameter("Balance must be -9 to 9".into()));
        }
        Ok(self.write_direct(var::BALANCE, value as u8).await? as i8)
    }

    pub async fn step_balance(&mut self, steps: i8) -> ApiResult<i8> {
        let frame = build_write(var::BALANCE, Some(steps as u8));
        let reply = self.send_recv(&frame).await?;
        Ok(reply.value().ok_or(AppError::InvalidReply)? as i8)
    }

    // --- Dim ---

    pub async fn get_dim(&mut self) -> ApiResult<u8> {
        self.read_direct(var::DIM).await
    }

    pub async fn set_dim(&mut self, level: u8) -> ApiResult<u8> {
        if level > 3 {
            return Err(AppError::InvalidParameter("Dim level must be 0-3".into()));
        }
        self.write_direct(var::DIM, level).await
    }

    pub async fn step_dim(&mut self) -> ApiResult<u8> {
        let frame = build_write(var::DIM, Some(0x00));
        let reply = self.send_recv(&frame).await?;
        reply.value().ok_or(AppError::InvalidReply)
    }

    // --- Menu ---

    pub async fn menu_enter(&mut self) -> ApiResult<()> {
        self.write_direct(var::MENU, 0x01).await?;
        Ok(())
    }

    pub async fn menu_exit(&mut self) -> ApiResult<()> {
        self.write_direct(var::MENU, 0x00).await?;
        Ok(())
    }

    pub async fn menu_nav(&mut self, remote_value: u8) -> ApiResult<()> {
        let frame = build_write(var::REMOTE, Some(remote_value));
        self.send_recv(&frame).await?;
        Ok(())
    }

    // --- IR Input ---

    pub async fn get_ir_input(&mut self) -> ApiResult<bool> {
        // 0x00 = front, any other = back
        Ok(self.read_direct(var::IR_INPUT).await? != 0x00)
    }

    pub async fn set_ir_input(&mut self, back: bool) -> ApiResult<bool> {
        let value = if back { 0x01 } else { 0x00 };
        Ok(self.write_direct(var::IR_INPUT, value).await? != 0x00)
    }

    // --- Info ---

    pub async fn get_product_line(&mut self) -> ApiResult<String> {
        self.read_text(var::PRODUCT_LINE).await
    }

    pub async fn get_model_name(&mut self) -> ApiResult<String> {
        self.read_text(var::MODEL_NAME).await
    }

    pub async fn get_version(&mut self) -> ApiResult<String> {
        self.read_text(var::VERSION).await
    }

    pub async fn get_input_name_current(&mut self) -> ApiResult<String> {
        self.read_text(var::INPUT_NAME).await
    }

    pub async fn get_input_name(&mut self, input: u8) -> ApiResult<String> {
        if !(1..=7).contains(&input) {
            return Err(AppError::InvalidParameter("Input must be 1-7".into()));
        }
        // Spec requires CMD_READ (0x52) with the direct-mode variable and
        // the input number as the value byte.
        let frame = build_frame(CMD_READ, direct(var::INPUT_NAME), Some(input));
        let reply = self.send_recv(&frame).await?;
        Ok(reply.as_text().ok_or(AppError::InvalidReply)?.to_owned())
    }

    // --- Factory reset ---

    pub async fn factory_reset(&mut self) -> ApiResult<()> {
        let frame = build_write(var::FACTORY_RESET, Some(0x00));
        // No reply expected; device turns verbose off after reset.
        self.send_only(&frame).await
    }
}
