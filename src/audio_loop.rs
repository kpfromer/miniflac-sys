//! Reference Core 1 playback loop for the ESP32-S3 digital audio player.
//!
//! This file is NOT compiled as a binary — it documents the intended
//! integration pattern for the Embassy async task that drives I2S output.
//!
//! Stubs replace hardware functions (`sd_read`, `i2s_write`, etc.) so the
//! file compiles on host for documentation / review purposes.
//!
//! # Pattern
//! 1. Fill a ~4 KB read buffer from the SD card.
//! 2. Feed the front of the buffer into `FlacDecoder::decode()`.
//! 3. Write returned PCM to I2S (reconfigure if sample_rate/channels changed).
//! 4. Shift consumed bytes out of the buffer and refill from SD.
//! 5. On decode error, skip one byte (corrupt-data recovery).
//! 6. Poll a stop channel between frames.

#![allow(dead_code, unused_variables)]

use crate::FlacDecoder;

// ---------------------------------------------------------------------------
// Hardware stubs (replace with real Embassy / esp-hal calls)
// ---------------------------------------------------------------------------

/// Opaque I2S handle placeholder.
pub struct I2s;

/// Read up to `buf.len()` bytes from the SD card into `buf`.
/// Returns the number of bytes actually read (0 = EOF).
fn sd_read(buf: &mut [u8]) -> usize {
    unimplemented!("replace with real SD card read")
}

/// Write interleaved i16 PCM to I2S DMA.
fn i2s_write(i2s: &mut I2s, samples: &[i16]) {
    unimplemented!("replace with real I2S write")
}

/// Reconfigure I2S for a new sample rate / channel count.
fn i2s_reconfigure(i2s: &mut I2s, sample_rate: u32, channels: u8) {
    unimplemented!("replace with real I2S reconfigure")
}

/// Returns `true` if a stop command has arrived on the Embassy channel.
fn should_stop() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Playback loop
// ---------------------------------------------------------------------------

/// Decode and play one FLAC file. Called by the Core 1 Embassy task.
pub fn play_file(decoder: &mut FlacDecoder, i2s: &mut I2s) {
    // -------------------------------------------------------------------
    // Read buffer: 4 KB keeps SD latency manageable while fitting on stack.
    // The push-style API handles partial frames via MINIFLAC_CONTINUE so a
    // smaller window is fine — just call decode() again after refilling.
    // -------------------------------------------------------------------
    const BUF_SIZE: usize = 4096;
    let mut buf = [0u8; BUF_SIZE];
    let mut filled: usize = 0;

    // I2S parameters — detect changes to reconfigure.
    let mut current_sample_rate: u32 = 0;
    let mut current_channels: u8 = 0;

    // Initial fill
    filled += sd_read(&mut buf[filled..]);

    'outer: loop {
        if should_stop() {
            break;
        }

        if filled == 0 {
            break; // EOF
        }

        match decoder.decode(&buf[..filled]) {
            Ok((consumed, frame)) => {
                if consumed > 0 {
                    buf.copy_within(consumed..filled, 0);
                    filled -= consumed;
                    filled += sd_read(&mut buf[filled..]);
                }

                if let Some(f) = frame {
                    if f.sample_rate != current_sample_rate || f.channels != current_channels {
                        current_sample_rate = f.sample_rate;
                        current_channels = f.channels;
                        i2s_reconfigure(i2s, current_sample_rate, current_channels);
                    }
                    i2s_write(i2s, f.samples());
                } else if consumed == 0 {
                    // No progress — buffer full but still can't decode.
                    if filled == BUF_SIZE {
                        // Skip one byte to resync past corrupt data.
                        buf.copy_within(1..filled, 0);
                        filled -= 1;
                        filled += sd_read(&mut buf[filled..]);
                    } else {
                        let n = sd_read(&mut buf[filled..]);
                        if n == 0 {
                            break 'outer; // EOF with no complete frame
                        }
                        filled += n;
                    }
                }
            }

            Err(_) => {
                // Decode error — skip one byte and resync.
                if filled > 0 {
                    buf.copy_within(1..filled, 0);
                    filled -= 1;
                    filled += sd_read(&mut buf[filled..]);
                }
            }
        }
    }
}
