//! Host-side integration test for the miniflac FFI + FlacDecoder wrapper.
//! Decodes a known FLAC file and validates structural properties of the output.

extern crate std;
use std::{fs, vec::Vec};

use audio_decoder::{AudioDecoder, DecodeOutput, FlacDecoder};

/// Load the entire test FLAC file into a Vec<u8>.
fn load_test_flac() -> Vec<u8> {
    fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/test_440hz.flac"))
        .expect("test FLAC file not found — run `cargo build` in the crate root first")
}

// ---------------------------------------------------------------------------
// FlacDecoder unit tests
// ---------------------------------------------------------------------------

#[test]
fn miniflac_size_fits_in_default_storage() {
    // The default storage S=16384 must be >= miniflac_size().
    // FlacDecoder::init() asserts this; if it doesn't panic we're good.
    let mut dec: FlacDecoder<16384> = FlacDecoder::new();
    dec.init(); // panics if S < miniflac_size()
}

#[test]
fn decode_flac_produces_stereo_44100_frames() {
    let data = load_test_flac();

    let mut dec: FlacDecoder = FlacDecoder::new();
    dec.init();

    let mut pos = 0usize;
    let mut total_samples = 0usize;
    let mut frames = 0u32;

    while pos < data.len() {
        match dec.decode(&data[pos..]) {
            Ok((consumed, Some(frame))) => {
                pos += consumed;
                frames += 1;

                // The test file is stereo 44100 Hz 16-bit.
                assert_eq!(frame.sample_rate, 44100, "unexpected sample rate");
                assert_eq!(frame.channels, 2, "expected stereo");
                assert_eq!(frame.bps, 16, "expected 16-bit");
                assert!(frame.block_size > 0 && frame.block_size as usize <= audio_decoder::MAX_BLOCK_SIZE);

                // samples() length == channels × block_size
                let expected_len = frame.channels as usize * frame.block_size as usize;
                assert_eq!(frame.samples().len(), expected_len);

                // copy_interleaved_i16 copies the right count
                let mut out = vec![0i16; frame.samples().len()];
                let copied = frame.copy_interleaved_i16(&mut out);
                assert_eq!(copied, expected_len);
                assert_eq!(&out, frame.samples());

                total_samples += expected_len;
            }
            Ok((consumed, None)) => {
                if consumed == 0 {
                    break; // need more data but we've given it all — EOF
                }
                pos += consumed; // metadata block, skip
            }
            Err(e) => panic!("decode error at offset {pos}: {e:?}"),
        }
    }

    assert!(frames > 0, "no frames decoded");
    assert!(total_samples > 0, "no samples produced");

    // 0.1 s × 44100 Hz × 2 channels = 8820 interleaved samples.
    // Allow ±1 frame of tolerance for encoder padding.
    assert!(
        total_samples >= 8000 && total_samples <= 10000,
        "unexpected sample count {total_samples}"
    );
}

#[test]
fn reset_allows_re_decode() {
    let data = load_test_flac();

    let mut dec: FlacDecoder = FlacDecoder::new();
    dec.init();

    // Decode one frame
    let mut pos = 0usize;
    let first_frame = loop {
        match dec.decode(&data[pos..]).expect("decode error") {
            (consumed, Some(f)) => { let _ = consumed; break f; }
            (consumed, None) => {
                if consumed == 0 { panic!("no frame found") }
                pos += consumed;
            }
        }
    };

    // Reset and decode again from the start — should produce the same first frame.
    dec.reset();
    pos = 0;
    let second_frame = loop {
        match dec.decode(&data[pos..]).expect("decode error") {
            (consumed, Some(f)) => { let _ = consumed; break f; }
            (consumed, None) => {
                if consumed == 0 { panic!("no frame found after reset") }
                pos += consumed;
            }
        }
    };

    assert_eq!(first_frame.sample_rate, second_frame.sample_rate);
    assert_eq!(first_frame.channels, second_frame.channels);
    assert_eq!(first_frame.block_size, second_frame.block_size);
    assert_eq!(first_frame.samples(), second_frame.samples());
}

// ---------------------------------------------------------------------------
// AudioDecoder enum dispatch test
// ---------------------------------------------------------------------------

#[test]
fn audio_decoder_flac_variant() {
    let data = load_test_flac();

    let mut decoder = AudioDecoder::new_flac();
    let mut pos = 0usize;
    let mut frames = 0u32;

    while pos < data.len() {
        match decoder.decode(&data[pos..]) {
            Ok(DecodeOutput { consumed, frame }) => {
                if consumed == 0 && frame.is_none() {
                    break;
                }
                pos += consumed;
                if let Some(f) = frame {
                    frames += 1;
                    assert_eq!(f.sample_rate, 44100);
                    assert_eq!(f.channels, 2);
                }
            }
            Err(e) => panic!("AudioDecoder::decode error: {e:?}"),
        }
    }

    assert!(frames > 0, "AudioDecoder produced no frames");
}
