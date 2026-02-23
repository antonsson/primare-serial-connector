#!/usr/bin/env python3
"""Mock Primare amplifier over a serial port for service-level testing."""

from __future__ import annotations

import argparse
import logging
import time
from dataclasses import dataclass, field

import serial

STX = 0x02
DLE = 0x10
ETX = 0x03
CMD_WRITE = 0x57
CMD_READ = 0x52
DIRECT_BIT = 0x80

VAR_STANDBY = 0x01
VAR_INPUT = 0x02
VAR_VOLUME = 0x03
VAR_BALANCE = 0x04
VAR_MUTE = 0x09
VAR_DIM = 0x0A
VAR_VERBOSE = 0x0D
VAR_MENU = 0x0E
VAR_REMOTE = 0x0F
VAR_IR_INPUT = 0x12
VAR_FACTORY_RESET = 0x13
VAR_INPUT_NAME = 0x14
VAR_PRODUCT_LINE = 0x15
VAR_MODEL_NAME = 0x16
VAR_VERSION = 0x17


@dataclass
class AmpState:
    power: int = 1
    input: int = 1
    volume: int = 25
    balance: int = 0
    mute: int = 0
    dim: int = 2
    verbose: int = 1
    ir_input_back: int = 0
    product_line: str = "PRIMARE"
    model: str = "I22-MOCK"
    version: str = "V0.0.1"
    input_names: dict[int, str] = field(
        default_factory=lambda: {
            1: "CD",
            2: "TV",
            3: "AUX",
            4: "BT",
            5: "TUNER",
            6: "PHONO",
            7: "STREAM",
        }
    )


def clamp(value: int, low: int, high: int) -> int:
    return max(low, min(high, value))


def to_i8(raw: int) -> int:
    return raw - 256 if raw >= 128 else raw


def from_i8(value: int) -> int:
    return value & 0xFF


def escape_payload(payload: bytes) -> bytes:
    out = bytearray()
    for b in payload:
        out.append(b)
        if b == DLE:
            out.append(DLE)
    return bytes(out)


def build_reply(variable: int, values: bytes) -> bytes:
    payload = bytes([variable]) + values
    return bytes([STX]) + escape_payload(payload) + bytes([DLE, ETX])


def unescape_payload(raw: bytes) -> bytes | None:
    if not raw or raw[0] != STX:
        return None

    out = bytearray()
    i = 1
    while i < len(raw):
        b = raw[i]
        if b == DLE:
            if i + 1 >= len(raw):
                return None
            nxt = raw[i + 1]
            if nxt == ETX:
                return bytes(out)
            if nxt == DLE:
                out.append(DLE)
                i += 2
                continue
            return None
        out.append(b)
        i += 1

    return None


def read_frame(port: serial.Serial) -> bytes:
    buf = bytearray()
    while True:
        b = port.read(1)
        if not b:
            raise TimeoutError("serial read timeout")
        val = b[0]
        buf.append(val)
        if len(buf) >= 2 and buf[-2] == DLE and buf[-1] == ETX:
            return bytes(buf)


def current_value(state: AmpState, var_id: int) -> int:
    if var_id == VAR_STANDBY:
        return state.power
    if var_id == VAR_INPUT:
        return state.input
    if var_id == VAR_VOLUME:
        return state.volume
    if var_id == VAR_BALANCE:
        return from_i8(state.balance)
    if var_id == VAR_MUTE:
        return state.mute
    if var_id == VAR_DIM:
        return state.dim
    if var_id == VAR_VERBOSE:
        return state.verbose
    if var_id == VAR_IR_INPUT:
        return state.ir_input_back
    return 0


def apply_direct_write(state: AmpState, var_id: int, value: int) -> int:
    # Service uses direct var + value=0 for reads. We treat value=0 as read to avoid
    # mutating state during /status polling.
    if value == 0:
        return current_value(state, var_id)

    if var_id == VAR_STANDBY:
        state.power = 1 if value else 0
    elif var_id == VAR_INPUT:
        state.input = clamp(value, 1, 7)
    elif var_id == VAR_VOLUME:
        state.volume = clamp(value, 0, 79)
    elif var_id == VAR_BALANCE:
        state.balance = clamp(to_i8(value), -9, 9)
    elif var_id == VAR_MUTE:
        state.mute = 1 if value else 0
    elif var_id == VAR_DIM:
        state.dim = clamp(value, 0, 3)
    elif var_id == VAR_VERBOSE:
        state.verbose = 1 if value else 0
    elif var_id == VAR_IR_INPUT:
        state.ir_input_back = 1 if value else 0

    return current_value(state, var_id)


def apply_step_write(state: AmpState, var_id: int, value: int) -> int:
    if var_id == VAR_STANDBY:
        state.power = 0 if state.power else 1
        return state.power
    if var_id == VAR_INPUT:
        step = 1 if value == 0x01 else -1
        state.input = clamp(state.input + step, 1, 7)
        return state.input
    if var_id == VAR_VOLUME:
        step = 1 if value == 0x01 else -1
        state.volume = clamp(state.volume + step, 0, 79)
        return state.volume
    if var_id == VAR_BALANCE:
        state.balance = clamp(state.balance + to_i8(value), -9, 9)
        return from_i8(state.balance)
    if var_id == VAR_MUTE:
        state.mute = 0 if state.mute else 1
        return state.mute
    if var_id == VAR_DIM:
        state.dim = (state.dim + 1) % 4
        return state.dim
    if var_id == VAR_MENU:
        return 1
    if var_id == VAR_REMOTE:
        return value
    if var_id == VAR_FACTORY_RESET:
        # Real device usually replies nothing here.
        raise BrokenPipeError("no reply for factory reset")
    return current_value(state, var_id)


def handle_request(state: AmpState, frame: bytes) -> bytes | None:
    payload = unescape_payload(frame)
    if payload is None or len(payload) < 2:
        return None

    cmd = payload[0]
    var_raw = payload[1]
    values = payload[2:]

    is_direct = (var_raw & DIRECT_BIT) != 0
    var_id = var_raw & 0x7F
    value = values[0] if values else 0

    if cmd == CMD_READ:
        if var_id == VAR_PRODUCT_LINE:
            return build_reply(var_id, state.product_line.encode("utf-8"))
        if var_id == VAR_MODEL_NAME:
            return build_reply(var_id, state.model.encode("utf-8"))
        if var_id == VAR_VERSION:
            return build_reply(var_id, state.version.encode("utf-8"))
        if var_id == VAR_INPUT_NAME:
            return build_reply(var_id, state.input_names.get(state.input, f"Input {state.input}").encode("utf-8"))
        return build_reply(var_id, bytes([current_value(state, var_id)]))

    if cmd != CMD_WRITE:
        return None

    if is_direct and var_id == VAR_INPUT_NAME:
        idx = clamp(value, 1, 7)
        return build_reply(var_id, state.input_names.get(idx, f"Input {idx}").encode("utf-8"))

    if is_direct:
        result = apply_direct_write(state, var_id, value)
        return build_reply(var_id, bytes([result]))

    try:
        result = apply_step_write(state, var_id, value)
    except BrokenPipeError:
        return None
    return build_reply(var_id, bytes([result]))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Mock Primare amplifier over serial")
    parser.add_argument("--port", required=True, help="Serial port path, e.g. /tmp/ttyPRIMARE_MOCK")
    parser.add_argument("--baud", type=int, default=4800)
    parser.add_argument("--wait-seconds", type=int, default=20, help="Wait for serial port to appear")
    parser.add_argument("--log-level", default="INFO", choices=["DEBUG", "INFO", "WARNING", "ERROR"])
    return parser.parse_args()


def open_serial_with_wait(path: str, baud: int, wait_seconds: int) -> serial.Serial:
    deadline = time.time() + wait_seconds
    last_error: Exception | None = None
    while time.time() < deadline:
        try:
            return serial.Serial(path, baudrate=baud, timeout=1)
        except serial.SerialException as exc:
            last_error = exc
            time.sleep(0.2)
    raise serial.SerialException(f"could not open port {path}: {last_error}")


def main() -> int:
    args = parse_args()
    logging.basicConfig(level=getattr(logging, args.log_level), format="%(asctime)s %(levelname)s %(message)s")

    state = AmpState()
    logging.info("Starting mock amp on %s @ %d", args.port, args.baud)

    with open_serial_with_wait(args.port, args.baud, args.wait_seconds) as port:
        while True:
            try:
                frame = read_frame(port)
            except TimeoutError:
                continue
            except serial.SerialException as exc:
                logging.error("Serial error: %s", exc)
                return 1

            logging.debug("RX raw: %s", frame.hex(" "))
            reply = handle_request(state, frame)
            if reply is None:
                continue

            logging.debug("TX raw: %s", reply.hex(" "))
            port.write(reply)
            port.flush()


if __name__ == "__main__":
    raise SystemExit(main())
