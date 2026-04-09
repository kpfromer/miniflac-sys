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
//! 2. Feed the front of the buffer into `AudioDecoder::decode()`.
//! 3. Write returned PCM to I2S (reconfigure if sample_rate/channels changed).
//! 4. Shift consumed bytes out of the buffer and refill from SD.
//! 5. On decode error, skip one byte (corrupt-data recovery).
//! 6. Poll a stop channel between frames.

#![allow(dead_code, unused_variables)]

use crate::{AudioDecoder, DecodeError, DecodeOutput};

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

/// Reconfigure I2S for a new sample_rate / channel count.
fn i2s_reconfigure(i2s: &mut I2s, sample_rate: u32, channels: u8) {
    unimplemented!("replace with real I2S reconfigure")
}

/// Returns `true` if a stop command has arrived on the Embassy channel.
fn should_stop() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Extension detection
// ---------------------------------------------------------------------------

fn is_mp3(filename: &str) -> bool {
    filename.ends_with(".mp3") || filename.ends_with(".MP3")
}

fn is_flac(filename: &str) -> bool {
    filename.ends_with(".flac") || filename.ends_with(".FLAC")
}

// ---------------------------------------------------------------------------
// Playback loop
// ---------------------------------------------------------------------------

/// Decode and play one audio file. Called by the Core 1 Embassy task.
///
/// `filename` — used only to select the decoder type.
/// `i2s`      — I2S peripheral handle (already configured to a default rate).
pub fn play_file(filename: &str, i2s: &mut I2s) {
    // Choose decoder based on file extension.
    let mut decoder = if is_mp3(filename) {
        AudioDecoder::new_mp3()
    } else if is_flac(filename) {
        AudioDecoder::new_flac()
    } else {
        return; // unsupported format
    };

    // -------------------------------------------------------------------
    // Read buffer: 4 KB keeps SD latency manageable while fitting on stack.
    // For FLAC, a single max-size frame can be ~20 KB compressed; the
    // push-style API handles partial frames via MINIFLAC_CONTINUE so a
    // smaller window is fine — just call decode() again after refilling.
    // -------------------------------------------------------------------
    const BUF_SIZE: usize = 4096;
    let mut buf = [0u8; BUF_SIZE];
    let mut filled: usize = 0; // bytes currently valid in buf[0..filled]

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

        // Try to decode one frame from the front of the buffer.
        match decoder.decode(&buf[..filled]) {
            Ok(DecodeOutput { consumed, frame }) => {
                // Advance the buffer by consumed bytes.
                if consumed > 0 {
                    buf.copy_within(consumed..filled, 0);
                    filled -= consumed;
                    // Refill from SD card.
                    filled += sd_read(&mut buf[filled..]);
                }

                if let Some(f) = frame {
                    // Reconfigure I2S if format changed (e.g. first frame, or
                    // mixed-format playlist — rare but handled gracefully).
                    if f.sample_rate != current_sample_rate || f.channels != current_channels {
                        current_sample_rate = f.sample_rate;
                        current_channels = f.channels;
                        i2s_reconfigure(i2s, current_sample_rate, current_channels);
                    }

                    i2s_write(i2s, f.samples());
                } else if consumed == 0 {
                    // No progress and no frame: need more data but buffer is full.
                    // This shouldn't happen with BUF_SIZE >= max compressed frame,
                    // but guard against it anyway.
                    if filled == BUF_SIZE {
                        // Buffer full, still can't decode — skip a byte and retry.
                        buf.copy_within(1..filled, 0);
                        filled -= 1;
                        filled += sd_read(&mut buf[filled..]);
                    } else {
                        // Just need more data — refill and retry.
                        let n = sd_read(&mut buf[filled..]);
                        if n == 0 {
                            break 'outer; // EOF with no complete frame
                        }
                        filled += n;
                    }
                }
            }

            Err(DecodeError::Flac(e)) => {
                // FLAC decode error — skip one byte and try to resync.
                // miniflac_sync() will re-lock onto the next valid frame header.
                if filled > 0 {
                    buf.copy_within(1..filled, 0);
                    filled -= 1;
                    filled += sd_read(&mut buf[filled..]);
                }
            }

            Err(DecodeError::Mp3Corrupt) => {
                // rmp3 returned None — skip one byte to advance past garbage.
                if filled > 0 {
                    buf.copy_within(1..filled, 0);
                    filled -= 1;
                    filled += sd_read(&mut buf[filled..]);
                }
            }
        }
    }
}
