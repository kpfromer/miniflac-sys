//! `audio-decoder` — no_std MP3 + FLAC decoder crate for ESP32-S3 audio player.
//!
//! # Design
//! - Single `AudioDecoder` enum — no trait objects, no dynamic dispatch.
//! - Zero heap allocation; all state is inline in the enum variants.
//! - Output is always interleaved i16 PCM regardless of source bit depth.
//!
//! # Usage
//! ```ignore
//! let mut decoder = AudioDecoder::new_flac();
//! loop {
//!     let out = decoder.decode(&buf[pos..])?;
//!     pos += out.consumed;
//!     if let Some(frame) = out.frame {
//!         i2s.write(frame.samples());
//!     }
//! }
//! ```
#![no_std]

mod flac;

/// Reference Core 1 playback loop (not a binary; documents integration pattern).
pub mod audio_loop;

pub use flac::{DecodedFrame, FlacDecoder, FlacError, StreamInfo};
pub use flac::{MAX_BLOCK_SIZE, MAX_CHANNELS, MAX_SAMPLES_PER_FRAME};

use rmp3::RawDecoder;

// ---------------------------------------------------------------------------
// DecodeError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Flac(FlacError),
    /// rmp3 returned None even though data was present (corrupt MP3 stream).
    Mp3Corrupt,
}

impl From<FlacError> for DecodeError {
    fn from(e: FlacError) -> Self {
        DecodeError::Flac(e)
    }
}

// ---------------------------------------------------------------------------
// DecodeOutput
// ---------------------------------------------------------------------------

/// Result of one `AudioDecoder::decode()` call.
pub struct DecodeOutput {
    /// Bytes consumed from the front of the input slice.
    pub consumed: usize,
    /// Decoded audio frame, or `None` if the consumed bytes were non-audio
    /// (ID3 tags, FLAC metadata blocks, or incomplete frame needing more data).
    pub frame: Option<DecodedFrame>,
}

// ---------------------------------------------------------------------------
// AudioDecoder
// ---------------------------------------------------------------------------

/// Unified audio decoder for MP3 and FLAC, dispatched via an enum.
///
/// - `Mp3`: holds an `rmp3::RawDecoder` + 2304-sample scratch buffer.
///   The scratch buffer is passed to `rmp3::RawDecoder::next()` and samples
///   are then copied into the returned `DecodedFrame`.
/// - `Flac`: holds a `FlacDecoder<16384>`.
pub enum AudioDecoder {
    Mp3 {
        decoder: RawDecoder,
        scratch: [rmp3::Sample; 2304],
    },
    Flac(FlacDecoder<16384>),
}

impl AudioDecoder {
    /// Create an MP3 decoder, ready to use immediately.
    pub fn new_mp3() -> Self {
        AudioDecoder::Mp3 {
            decoder: RawDecoder::new(),
            scratch: [0i16; 2304],
        }
    }

    /// Create a FLAC decoder and run `miniflac_init`.
    pub fn new_flac() -> Self {
        let mut dec = FlacDecoder::new();
        dec.init();
        AudioDecoder::Flac(dec)
    }

    /// Reset the decoder state (e.g. when starting a new file of the same type).
    pub fn reset(&mut self) {
        match self {
            AudioDecoder::Mp3 { decoder, scratch } => {
                *decoder = RawDecoder::new();
                *scratch = [0i16; 2304];
            }
            AudioDecoder::Flac(dec) => dec.reset(),
        }
    }

    /// Decode one unit from `input`.
    ///
    /// Advances `consumed` bytes into the stream. Returns `frame: Some(_)` when
    /// a complete audio frame is available. Returns `frame: None` for non-audio
    /// content (ID3, FLAC metadata) or when more data is needed.
    ///
    /// On a corrupt-data error the caller should discard one byte and retry,
    /// see `audio_loop.rs` for the recovery pattern.
    pub fn decode(&mut self, input: &[u8]) -> Result<DecodeOutput, DecodeError> {
        match self {
            AudioDecoder::Mp3 { decoder, scratch } => decode_mp3(decoder, scratch, input),
            AudioDecoder::Flac(dec) => decode_flac(dec, input),
        }
    }
}

// ---------------------------------------------------------------------------
// MP3 decode path
// ---------------------------------------------------------------------------

fn decode_mp3(
    decoder: &mut RawDecoder,
    scratch: &mut [rmp3::Sample; 2304],
    input: &[u8],
) -> Result<DecodeOutput, DecodeError> {
    match decoder.next(input, scratch) {
        None => {
            // No frame found in the provided data. If input is non-empty this
            // usually means we need more data; treat as consumed=0, no frame.
            Ok(DecodeOutput { consumed: 0, frame: None })
        }
        Some((rmp3::Frame::Other(_raw), consumed)) => {
            // ID3 tag or other non-audio frame — skip it.
            Ok(DecodeOutput { consumed, frame: None })
        }
        Some((rmp3::Frame::Audio(audio), consumed)) => {
            let src = audio.samples(); // &[i16] borrowing from scratch
            let frame = DecodedFrame::from_pcm(
                audio.sample_rate(),
                audio.channels() as u8,
                16,
                audio.sample_count() as u16, // samples per channel
                src,
            );
            Ok(DecodeOutput { consumed, frame: Some(frame) })
        }
    }
}

// ---------------------------------------------------------------------------
// FLAC decode path
// ---------------------------------------------------------------------------

fn decode_flac(dec: &mut FlacDecoder<16384>, input: &[u8]) -> Result<DecodeOutput, DecodeError> {
    let (consumed, frame) = dec.decode(input)?;
    Ok(DecodeOutput { consumed, frame })
}
