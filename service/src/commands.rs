use crate::protocol::{self, CMD_READ, CMD_WRITE, direct, var};

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

pub const VERBOSE_ON: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::VERBOSE),
    default_value: Some(0x01),
};

pub const VERBOSE_OFF: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::VERBOSE),
    default_value: Some(0x00),
};

pub const GET_POWER: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::STANDBY),
    default_value: Some(0x00),
};

pub const SET_POWER_ON: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY,
    default_value: Some(0x01),
};

pub const SET_POWER_OFF: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY,
    default_value: Some(0x00),
};

pub const TOGGLE_POWER: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::STANDBY,
    default_value: Some(0x00),
};

pub const GET_VOLUME: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::VOLUME),
    default_value: Some(0x00),
};

pub const SET_VOLUME: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::VOLUME),
    default_value: None,
};

pub const STEP_VOLUME: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::VOLUME,
    default_value: None,
};

pub const GET_INPUT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::INPUT),
    default_value: Some(0x00),
};

pub const SET_INPUT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::INPUT),
    default_value: None,
};

pub const STEP_INPUT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::INPUT,
    default_value: None,
};

pub const GET_MUTE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::MUTE),
    default_value: Some(0x00),
};

pub const SET_MUTE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::MUTE),
    default_value: None,
};

pub const TOGGLE_MUTE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::MUTE,
    default_value: Some(0x00),
};

pub const GET_BALANCE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::BALANCE),
    default_value: Some(0x00),
};

pub const SET_BALANCE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::BALANCE),
    default_value: None,
};

pub const STEP_BALANCE: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::BALANCE,
    default_value: None,
};

pub const GET_DIM: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::DIM),
    default_value: Some(0x00),
};

pub const SET_DIM: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::DIM),
    default_value: None,
};

pub const STEP_DIM: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::DIM,
    default_value: Some(0x00),
};

pub const MENU_ENTER: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::MENU),
    default_value: Some(0x01),
};

pub const MENU_EXIT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::MENU),
    default_value: Some(0x00),
};

pub const MENU_NAV: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::REMOTE,
    default_value: None,
};

pub const GET_IR_INPUT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::IR_INPUT),
    default_value: Some(0x00),
};

pub const SET_IR_INPUT: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: direct(var::IR_INPUT),
    default_value: None,
};

pub const GET_PRODUCT_LINE: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::PRODUCT_LINE,
    default_value: Some(0x00),
};

pub const GET_MODEL_NAME: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::MODEL_NAME,
    default_value: Some(0x00),
};

pub const GET_VERSION: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::VERSION,
    default_value: Some(0x00),
};

pub const GET_INPUT_NAME_CURRENT: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: var::INPUT_NAME,
    default_value: Some(0x00),
};

pub const GET_INPUT_NAME_BY_ID: CommandSpec = CommandSpec {
    cmd: CMD_READ,
    variable: direct(var::INPUT_NAME),
    default_value: None,
};

pub const FACTORY_RESET: CommandSpec = CommandSpec {
    cmd: CMD_WRITE,
    variable: var::FACTORY_RESET,
    default_value: Some(0x00),
};
