//! Virtual controller: button vocabulary and press semantics over Ghostpad.
//!
//! `ghostpad` owns the TCP connection and the 60 Hz frame sender. This module
//! owns the names the UI and CLI use, the ScePad bit each name maps to, and
//! the tap, hold, combo, macro and stick helpers.

use crate::error::{Error, Result};
use crate::ghostpad::{Ghostpad, CENTRE};
use std::time::Duration;

/// ScePad button bits as the payload inserts them into ScePadData.
pub const BIT_L3: u32 = 0x0000_0001;
pub const BIT_R3: u32 = 0x0000_0002;
pub const BIT_CREATE: u32 = 0x0000_0004;
pub const BIT_OPTIONS: u32 = 0x0000_0008;
pub const BIT_UP: u32 = 0x0000_0010;
pub const BIT_RIGHT: u32 = 0x0000_0020;
pub const BIT_DOWN: u32 = 0x0000_0040;
pub const BIT_LEFT: u32 = 0x0000_0080;
pub const BIT_L2: u32 = 0x0000_0100;
pub const BIT_R2: u32 = 0x0000_0200;
pub const BIT_L1: u32 = 0x0000_0400;
pub const BIT_R1: u32 = 0x0000_0800;
pub const BIT_TRIANGLE: u32 = 0x0000_1000;
pub const BIT_CIRCLE: u32 = 0x0000_2000;
pub const BIT_CROSS: u32 = 0x0000_4000;
pub const BIT_SQUARE: u32 = 0x0000_8000;
pub const BIT_PS: u32 = 0x0001_0000;
pub const BIT_TOUCHPAD: u32 = 0x0010_0000;

/// Every button the UI and CLI accept by name.
pub const BUTTONS: &[&str] = &[
    "cross", "circle", "square", "triangle", "up", "down", "left", "right", "l1", "l2", "l3", "r1",
    "r2", "r3", "create", "options", "ps", "touchpad",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Cross,
    Circle,
    Square,
    Triangle,
    Up,
    Down,
    Left,
    Right,
    L1,
    L2,
    L3,
    R1,
    R2,
    R3,
    Create,
    Options,
    Ps,
    Touchpad,
}

impl Button {
    pub fn parse(name: &str) -> Result<Button> {
        Ok(match name.trim().to_ascii_lowercase().as_str() {
            "cross" | "x" => Button::Cross,
            "circle" | "o" => Button::Circle,
            "square" => Button::Square,
            "triangle" => Button::Triangle,
            "up" => Button::Up,
            "down" => Button::Down,
            "left" => Button::Left,
            "right" => Button::Right,
            "l1" => Button::L1,
            "l2" => Button::L2,
            "l3" => Button::L3,
            "r1" => Button::R1,
            "r2" => Button::R2,
            "r3" => Button::R3,
            "select" | "share" | "create" => Button::Create,
            "start" | "options" => Button::Options,
            "ps" => Button::Ps,
            "touchpad" | "touch" => Button::Touchpad,
            other => return Err(Error::Invalid(format!("unknown button '{other}'"))),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Button::Cross => "cross",
            Button::Circle => "circle",
            Button::Square => "square",
            Button::Triangle => "triangle",
            Button::Up => "up",
            Button::Down => "down",
            Button::Left => "left",
            Button::Right => "right",
            Button::L1 => "l1",
            Button::L2 => "l2",
            Button::L3 => "l3",
            Button::R1 => "r1",
            Button::R2 => "r2",
            Button::R3 => "r3",
            Button::Create => "create",
            Button::Options => "options",
            Button::Ps => "ps",
            Button::Touchpad => "touchpad",
        }
    }

    pub fn bit(self) -> u32 {
        match self {
            Button::Cross => BIT_CROSS,
            Button::Circle => BIT_CIRCLE,
            Button::Square => BIT_SQUARE,
            Button::Triangle => BIT_TRIANGLE,
            Button::Up => BIT_UP,
            Button::Down => BIT_DOWN,
            Button::Left => BIT_LEFT,
            Button::Right => BIT_RIGHT,
            Button::L1 => BIT_L1,
            Button::L2 => BIT_L2,
            Button::L3 => BIT_L3,
            Button::R1 => BIT_R1,
            Button::R2 => BIT_R2,
            Button::R3 => BIT_R3,
            Button::Create => BIT_CREATE,
            Button::Options => BIT_OPTIONS,
            Button::Ps => BIT_PS,
            Button::Touchpad => BIT_TOUCHPAD,
        }
    }
}

fn mask_for(names: &[&str]) -> Result<u32> {
    let mut mask = 0;
    for name in names {
        mask |= Button::parse(name)?.bit();
    }
    Ok(mask)
}

/// Tap one button: hold for 80 ms, then lift. Held buttons stay held.
pub fn press(pad: &Ghostpad, name: &str) -> Result<()> {
    let bit = Button::parse(name)?.bit();
    pad.hold(bit)?;
    std::thread::sleep(Duration::from_millis(80));
    pad.release(bit)
}

/// Hold a button until `release` is called.
pub fn hold(pad: &Ghostpad, name: &str) -> Result<()> {
    pad.hold(Button::parse(name)?.bit())
}

pub fn release(pad: &Ghostpad) -> Result<()> {
    pad.release_all()
}

/// Lift one button, keeping the others held.
pub fn release_button(pad: &Ghostpad, name: &str) -> Result<()> {
    pad.release(Button::parse(name)?.bit())
}

/// Press buttons together: hold all of them for `hold_ms`, then lift them.
pub fn combo(pad: &Ghostpad, names: &[&str], hold_ms: u64) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let mask = mask_for(names)?;
    pad.hold(mask)?;
    std::thread::sleep(Duration::from_millis(hold_ms.clamp(40, 5_000)));
    pad.release(mask)
}

/// Run a macro: each step is a button name or an `a+b` combo, separated by
/// commas.
pub fn macro_run(pad: &Ghostpad, script: &str, step_ms: u64) -> Result<()> {
    let step = Duration::from_millis(step_ms.clamp(20, 5_000));
    for step_text in script.split(',') {
        let parts: Vec<&str> = step_text
            .split('+')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        let mask = mask_for(&parts)?;
        pad.hold(mask)?;
        std::thread::sleep(step);
        pad.release(mask)?;
        std::thread::sleep(step);
    }
    Ok(())
}

/// Stick direction as 0..255 on each axis, 128 centred.
pub fn analog_axes(direction: &str) -> Result<(u8, u8)> {
    let low = 0u8;
    let high = 255u8;
    Ok(match direction.trim().to_ascii_lowercase().as_str() {
        "centre" | "center" | "neutral" => (CENTRE, CENTRE),
        "up" => (CENTRE, low),
        "down" => (CENTRE, high),
        "left" => (low, CENTRE),
        "right" => (high, CENTRE),
        "up_left" => (low, low),
        "up_right" => (high, low),
        "down_left" => (low, high),
        "down_right" => (high, high),
        other => {
            return Err(Error::Invalid(format!(
                "unknown stick direction {other}"
            )))
        }
    })
}

/// Tilt a stick: side `L`/`R`, direction `up`, `down_left`, `centre`, ...
pub fn analog(pad: &Ghostpad, side: &str, direction: &str) -> Result<()> {
    let right = match side.to_ascii_uppercase().as_str() {
        "R" | "RIGHT" => true,
        "L" | "LEFT" => false,
        other => return Err(Error::Invalid(format!("unknown stick {other}"))),
    };
    let (x, y) = analog_axes(direction)?;
    pad.stick(right, x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases_and_names() {
        assert_eq!(Button::parse("X").unwrap(), Button::Cross);
        assert_eq!(Button::parse("Circle").unwrap(), Button::Circle);
        assert_eq!(Button::parse("Share").unwrap(), Button::Create);
        assert_eq!(Button::parse("Start").unwrap(), Button::Options);
        assert_eq!(Button::parse("l3").unwrap().name(), "l3");
        assert!(Button::parse("jump").is_err());
    }

    #[test]
    fn every_button_round_trips() {
        for name in BUTTONS {
            assert_eq!(Button::parse(name).unwrap().name(), *name);
        }
    }

    #[test]
    fn bits_are_distinct_and_nonzero() {
        let mut seen = 0u32;
        for name in BUTTONS {
            let bit = Button::parse(name).unwrap().bit();
            assert_ne!(bit, 0);
            assert_eq!(seen & bit, 0, "{name} shares a bit");
            seen |= bit;
        }
    }

    #[test]
    fn stick_directions_use_the_full_range() {
        assert_eq!(analog_axes("up").unwrap(), (128, 0));
        assert_eq!(analog_axes("down_right").unwrap(), (255, 255));
        assert_eq!(analog_axes("centre").unwrap(), (128, 128));
        assert!(analog_axes("sideways").is_err());
    }
}
