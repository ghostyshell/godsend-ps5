//! Ghostpad client: the PS5 virtual controller payload.
//!
//! Ghostpad listens on TCP 6967 and takes a fixed 16-byte frame at 60 Hz:
//!
//! ```text
//! 0  "GPAD"           magic
//! 4  u32 big-endian   ScePad button bitmask
//! 8  u8 L-stick X     0..255, 128 centred
//! 9  u8 L-stick Y
//! 10 u8 R-stick X
//! 11 u8 R-stick Y
//! 12 u8 L2            0..255
//! 13 u8 R2
//! 14 2 bytes          reserved
//! ```
//!
//! The payload registers the pad through `/dev/hid`, so the console treats it
//! as a real DualSense. Nothing in this module needs privileges beyond a
//! reachable TCP port; without the payload every call reports that clearly.

use crate::config::Host;
use crate::error::{Error, Result};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MAGIC: [u8; 4] = *b"GPAD";
pub const FRAME_BYTES: usize = 16;
pub const CENTRE: u8 = 128;
/// 60 frames per second, the rate the payload expects.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u32,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    pub l2: u8,
    pub r2: u8,
}

impl Default for PadState {
    fn default() -> Self {
        Self {
            buttons: 0,
            lx: CENTRE,
            ly: CENTRE,
            rx: CENTRE,
            ry: CENTRE,
            l2: 0,
            r2: 0,
        }
    }
}

impl PadState {
    pub fn frame(&self) -> [u8; FRAME_BYTES] {
        let mut frame = [0u8; FRAME_BYTES];
        frame[0..4].copy_from_slice(&MAGIC);
        frame[4..8].copy_from_slice(&self.buttons.to_be_bytes());
        frame[8] = self.lx;
        frame[9] = self.ly;
        frame[10] = self.rx;
        frame[11] = self.ry;
        frame[12] = self.l2;
        frame[13] = self.r2;
        frame
    }
}

pub struct Ghostpad {
    state: Arc<Mutex<PadState>>,
    alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
    addr: String,
}

impl Ghostpad {
    /// Connect to the payload, which must already be running on the console.
    pub fn connect(host: &Host) -> Result<Ghostpad> {
        let addr = host.socket(host.pad_port);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| Error::DeviceRequired(format!("bad Ghostpad address {addr}: {e}")))?
            .next()
            .ok_or_else(|| Error::DeviceRequired(format!("no address for {addr}")))?;
        let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2)).map_err(|e| {
            Error::DeviceRequired(format!(
                "Ghostpad is not answering on {addr} ({e}). Load the Ghostpad payload on the console."
            ))
        })?;
        stream.set_nodelay(true)?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        let mut writer = stream.try_clone()?;
        let state = Arc::new(Mutex::new(PadState::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_state = Arc::clone(&state);
        let thread_alive = Arc::clone(&alive);
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let frame = match thread_state.lock() {
                    Ok(state) => state.frame(),
                    Err(_) => break,
                };
                if writer.write_all(&frame).is_err() {
                    thread_alive.store(false, Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(FRAME_INTERVAL);
            }
        });

        Ok(Ghostpad {
            state,
            alive,
            stop,
            writer: Mutex::new(Some(handle)),
            addr,
        })
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    fn check(&self) -> Result<()> {
        if self.alive() {
            Ok(())
        } else {
            Err(Error::device(format!(
                "Ghostpad closed the connection on {}",
                self.addr
            )))
        }
    }

    pub fn state(&self) -> PadState {
        self.state.lock().map(|s| *s).unwrap_or_default()
    }

    fn update(&self, change: impl FnOnce(&mut PadState)) -> Result<()> {
        self.check()?;
        if let Ok(mut state) = self.state.lock() {
            change(&mut state);
        }
        Ok(())
    }

    /// Replace the whole button mask.
    pub fn set_mask(&self, mask: u32) -> Result<()> {
        self.update(|state| state.buttons = mask)
    }

    /// Add buttons to the held mask.
    pub fn hold(&self, mask: u32) -> Result<()> {
        self.update(|state| state.buttons |= mask)
    }

    /// Lift buttons, keeping the others held.
    pub fn release(&self, mask: u32) -> Result<()> {
        self.update(|state| state.buttons &= !mask)
    }

    pub fn release_all(&self) -> Result<()> {
        self.update(|state| {
            *state = PadState::default();
        })
    }

    /// Stick position as 0..255 on each axis (128 is centred).
    pub fn stick(&self, right: bool, x: u8, y: u8) -> Result<()> {
        self.update(|state| {
            if right {
                state.rx = x;
                state.ry = y;
            } else {
                state.lx = x;
                state.ly = y;
            }
        })
    }

    /// Analog trigger pressure 0..255.
    pub fn trigger(&self, right: bool, value: u8) -> Result<()> {
        self.update(|state| {
            if right {
                state.r2 = value;
            } else {
                state.l2 = value;
            }
        })
    }
}

impl Drop for Ghostpad {
    fn drop(&mut self) {
        // The payload keeps the last frame it received. Send one neutral frame
        // before closing so a held button never stays pressed on the console.
        if let Ok(mut state) = self.state.lock() {
            *state = PadState::default();
        }
        std::thread::sleep(FRAME_INTERVAL * 2);
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut handle) = self.writer.lock() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_layout_matches_the_payload() {
        let state = PadState {
            buttons: 0x0000_4000,
            lx: 0,
            ly: 255,
            rx: 128,
            ry: 64,
            l2: 10,
            r2: 20,
        };
        let frame = state.frame();
        assert_eq!(&frame[0..4], b"GPAD");
        assert_eq!(&frame[4..8], &[0x00, 0x00, 0x40, 0x00]);
        assert_eq!(frame[8], 0);
        assert_eq!(frame[9], 255);
        assert_eq!(frame[10], 128);
        assert_eq!(frame[11], 64);
        assert_eq!(frame[12], 10);
        assert_eq!(frame[13], 20);
        assert_eq!(frame.len(), FRAME_BYTES);
    }

    #[test]
    fn default_state_is_centred() {
        let state = PadState::default();
        assert_eq!(state.lx, CENTRE);
        assert_eq!(state.ry, CENTRE);
        assert_eq!(state.buttons, 0);
    }
}
