//! Primare I22 RS232 protocol implementation.
//!
//! Frame format:  <STX> <CMD> <VAR> [<VALUE>] <DLE> <ETX>
//! Reply format:  <STX> <VAR> <VALUE> [<VALUE2>...] <DLE> <ETX>
//!
//! Special bytes:
//!   STX = 0x02
//!   DLE = 0x10  (must be doubled/escaped when appearing as data)
//!   ETX = 0x03
//!
//! CMD: 0x57 = Write ('W'), 0x52 = Read ('R')

pub const STX: u8 = 0x02;
pub const DLE: u8 = 0x10;
pub const ETX: u8 = 0x03;
pub const CMD_WRITE: u8 = 0x57; // 'W'
pub const CMD_READ: u8 = 0x52;  // 'R'

/// IR remote command values for menu navigation
pub mod ir_remote {
    pub const STEP_UP: u8 = 0x1E;
    pub const STEP_DOWN: u8 = 0x1F;
    pub const ARROW_RIGHT: u8 = 0x08;
    pub const ARROW_LEFT: u8 = 0x09;
}

/// Variable IDs (direct)
pub mod var {
    pub const STANDBY:       u8 = 0x01;
    pub const INPUT:         u8 = 0x02;
    pub const VOLUME:        u8 = 0x03;
    pub const BALANCE:       u8 = 0x04;
    pub const MUTE:          u8 = 0x09;
    pub const DIM:           u8 = 0x0A;
    pub const VERBOSE:       u8 = 0x0D;
    pub const MENU:          u8 = 0x0E;
    pub const REMOTE:        u8 = 0x0F;
    pub const IR_INPUT:      u8 = 0x12;
    pub const FACTORY_RESET: u8 = 0x13;
    pub const INPUT_NAME:    u8 = 0x14;
    pub const PRODUCT_LINE:  u8 = 0x15;
    pub const MODEL_NAME:    u8 = 0x16;
    pub const VERSION:       u8 = 0x17;
}

/// Direct-mode variables use var | 0x80
pub const fn direct(var: u8) -> u8 {
    var | 0x80
}

/// Escape a data byte: if it equals DLE, double it.
fn escape(byte: u8, buf: &mut Vec<u8>) {
    buf.push(byte);
    if byte == DLE {
        buf.push(DLE);
    }
}

/// Build a command frame: `<STX> <cmd> <variable> [<value>] <DLE> <ETX>`.
/// All bytes are DLE-escaped as required by the protocol.
pub fn build_frame(cmd: u8, variable: u8, value: Option<u8>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.push(STX);
    escape(cmd, &mut buf);
    escape(variable, &mut buf);
    if let Some(v) = value {
        escape(v, &mut buf);
    }
    buf.push(DLE);
    buf.push(ETX);
    buf
}

/// Build a write (CMD_WRITE) frame.
pub fn build_write(variable: u8, value: Option<u8>) -> Vec<u8> {
    build_frame(CMD_WRITE, variable, value)
}

/// Build a read (CMD_READ) frame. Sends 0x00 as value, which per protocol
/// replies with the current value when verbose mode is active.
pub fn build_read(variable: u8) -> Vec<u8> {
    build_frame(CMD_READ, variable, Some(0x00))
}

/// Parsed reply from the I22.
#[derive(Debug, Clone)]
pub struct Reply {
    pub variable: u8,
    pub values: Vec<u8>,
}

impl Reply {
    /// First value byte, if present
    pub fn value(&self) -> Option<u8> {
        self.values.first().copied()
    }

    /// All value bytes decoded as UTF-8 string (for text replies)
    pub fn as_text(&self) -> Option<&str> {
        std::str::from_utf8(&self.values).ok()
    }
}

/// Parse a raw reply frame (including STX/DLE/ETX delimiters).
/// Handles DLE un-escaping.
pub fn parse_reply(raw: &[u8]) -> Option<Reply> {
    // Expect: STX ... DLE ETX
    if raw.len() < 4 {
        return None;
    }
    if raw[0] != STX {
        return None;
    }

    // Find terminal DLE ETX (un-escaped)
    // Walk the payload, un-doubling DLE bytes
    let mut payload: Vec<u8> = Vec::new();
    let mut i = 1usize;
    while i < raw.len() {
        let b = raw[i];
        if b == DLE {
            if i + 1 < raw.len() && raw[i + 1] == ETX {
                break; // end of frame
            } else if i + 1 < raw.len() && raw[i + 1] == DLE {
                // escaped DLE
                payload.push(DLE);
                i += 2;
                continue;
            }
        }
        payload.push(b);
        i += 1;
    }

    if payload.is_empty() {
        return None;
    }

    let variable = payload[0];
    let values = payload[1..].to_vec();

    Some(Reply { variable, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_write_volume_direct() {
        // Set volume to 10 (0x0A): variable = 0x83, value = 0x0A
        let frame = build_write(direct(var::VOLUME), Some(0x0A));
        assert_eq!(frame, vec![0x02, 0x57, 0x83, 0x0A, 0x10, 0x03]);
    }

    #[test]
    fn test_build_write_dle_escaping() {
        // If value is 0x10 (DLE), it must be doubled
        let frame = build_write(var::VOLUME, Some(DLE));
        assert_eq!(frame[4], DLE);
        assert_eq!(frame[5], DLE); // doubled
    }

    #[test]
    fn test_parse_reply_volume() {
        // Reply: 0x02 0x03 0x28 0x10 0x03  (volume = 40)
        let raw = vec![0x02, 0x03, 0x28, 0x10, 0x03];
        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.variable, var::VOLUME);
        assert_eq!(reply.value(), Some(0x28));
    }

    #[test]
    fn test_parse_reply_dle_unescaping() {
        // Reply containing escaped DLE in value
        let raw = vec![0x02, 0x03, 0x10, 0x10, 0x10, 0x03];
        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.value(), Some(0x10));
    }
}
