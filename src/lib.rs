//! `miniflac-rust-sys` — no_std FLAC decoder crate for ESP32-S3 audio player.
//!
//! Wraps miniflac v1.1.3 via hand-written FFI. Zero heap allocation.
//! Output is always interleaved i16 PCM regardless of source bit depth.
//!
//! # Usage
//! ```ignore
//! let mut dec = FlacDecoder::new();
//! dec.init();
//! loop {
//!     let (consumed, frame) = dec.decode(&buf[pos..])?;
//!     pos += consumed;
//!     if let Some(f) = frame {
//!         i2s.write(f.samples());
//!     }
//! }
//! ```
#![no_std]

mod flac;

/// Reference Core 1 playback loop (not a binary; documents integration pattern).
pub mod audio_loop;

pub use flac::{DecodedFrame, FlacDecoder, FlacError, StreamInfo};
pub use flac::{MAX_BLOCK_SIZE, MAX_CHANNELS, MAX_SAMPLES_PER_FRAME};
