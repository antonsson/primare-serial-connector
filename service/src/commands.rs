use crate::protocol::{self, var, CMD_READ, CMD_WRITE};

#[derive(Clone, Copy)]
pub struct CommandSpec {
    pub cmd: u8,
    pub variable: u8,
    pub default_value: Option<u8>,
}

impl CommandSpec {
    pub fn frame(self, value: Option<u8>) -> Vec<u8> {
        let value = value.or(self.default_value);
        match self.cmd {
            CMD_WRITE => protocol::build_write(self.variable, value),
            CMD_READ => match value {
                Some(0x00) => protocol::build_read(self.variable),
                _ => protocol::build_frame(self.cmd, self.variable, value),
            },
            _ => protocol::build_frame(self.cmd, self.variable, value),
        }
    }
}

// ---- Verbose ----
// VERBOSE_OFF uses the non-direct toggle (0x0D). The direct variant (0x8D 0x00) is
// treated as a query when verbose is on, so it would not turn verbose off.
pub const VERBOSE_ON: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VERBOSE_DIRECT,
    default_value: Some(0x01),
};
pub const VERBOSE_OFF: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VERBOSE,
    default_value: Some(0x00),
};

// ---- Power ----
// POWER_GET: direct 0x00 — per spec this is a query when verbose is on.
// POWER_ON / POWER_OFF: direct set; must be sent with verbose off (set_power disables it first).
// POWER_TOGGLE: non-direct, spec example W 0x01 0x00.
pub const POWER_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY_DIRECT,
    default_value: Some(0x00),
};
pub const POWER_ON: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY_DIRECT,
    default_value: Some(0x01),
};
pub const POWER_OFF: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY_DIRECT,
    default_value: Some(0x00),
};
pub const POWER_TOGGLE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY,
    default_value: Some(0x00),
};

// ---- Volume ----
pub const VOLUME_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VOLUME_DIRECT,
    default_value: Some(0x00),
};
pub const VOLUME_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VOLUME_DIRECT,
    default_value: None,
};
pub const VOLUME_STEP: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VOLUME,
    default_value: None,
};

// ---- Input ----
pub const INPUT_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::INPUT_DIRECT,
    default_value: Some(0x00),
};
pub const INPUT_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::INPUT_DIRECT,
    default_value: None,
};
pub const INPUT_STEP: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::INPUT,
    default_value: None,
};

// ---- Mute ----
pub const MUTE_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MUTE_DIRECT,
    default_value: Some(0x00),
};
pub const MUTE_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MUTE_DIRECT,
    default_value: None,
};
pub const MUTE_TOGGLE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MUTE,
    default_value: Some(0x00),
};

// ---- Balance ----
pub const BALANCE_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::BALANCE_DIRECT,
    default_value: Some(0x00),
};
pub const BALANCE_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::BALANCE_DIRECT,
    default_value: None,
};
pub const BALANCE_STEP: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::BALANCE,
    default_value: None,
};

// ---- Dim ----
pub const DIM_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::DIM_DIRECT,
    default_value: Some(0x00),
};
pub const DIM_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::DIM_DIRECT,
    default_value: None,
};
pub const DIM_STEP: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::DIM,
    default_value: Some(0x00),
};

// ---- Menu ----
// MENU_ENTER: direct 0x01 (enter) — value is non-zero, so works with verbose on.
// MENU_EXIT: non-direct toggle (0x0E 0x01), per spec example. Direct 0x00 would be a
// query when verbose is on and would not exit the menu.
pub const MENU_ENTER: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MENU_DIRECT,
    default_value: Some(0x01),
};
pub const MENU_EXIT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MENU,
    default_value: Some(0x01),
};
pub const MENU_NAV: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::REMOTE,
    default_value: None,
};

// ---- IR Input ----
pub const IR_INPUT_GET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::IR_INPUT_DIRECT,
    default_value: Some(0x00),
};
pub const IR_INPUT_SET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::IR_INPUT_DIRECT,
    default_value: None,
};

// ---- Device info (read commands) ----
pub const PRODUCT_LINE_READ: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::PRODUCT_LINE,
    default_value: Some(0x00),
};
pub const MODEL_NAME_READ: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::MODEL_NAME,
    default_value: Some(0x00),
};
pub const VERSION_READ: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::VERSION,
    default_value: Some(0x00),
};
pub const INPUT_NAME_READ: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::INPUT_NAME,
    default_value: Some(0x00),
};
pub const INPUT_NAME_BY_ID_READ: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::INPUT_NAME_DIRECT,
    default_value: None,
};

// ---- Factory reset ----
pub const FACTORY_RESET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::FACTORY_RESET,
    default_value: Some(0x00),
};
