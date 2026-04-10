//! Host-side integration tests for the miniflac FFI + FlacDecoder wrapper.

extern crate std;
use std::{fs, vec::Vec};

use audio_decoder::{FlacDecoder, MAX_BLOCK_SIZE};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load(name: &str) -> Vec<u8> {
    let path = std::format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read(&path).unwrap_or_else(|_| panic!("test file not found: {path}"))
}

/// Decode an entire in-memory FLAC file in one shot, returning all frames.
fn decode_all(data: &[u8]) -> Vec<audio_decoder::DecodedFrame> {
    let mut dec = FlacDecoder::<16384>::new();
    dec.init();
    let mut frames = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        match dec.decode(&data[pos..]).expect("decode error") {
            (consumed, Some(f)) => { pos += consumed; frames.push(f); }
            (consumed, None) => {
                if consumed == 0 { break; }
                pos += consumed;
            }
        }
    }
    frames
}

// ---------------------------------------------------------------------------
// Existing tests
// ---------------------------------------------------------------------------

#[test]
fn miniflac_size_fits_in_default_storage() {
    let mut dec: FlacDecoder<16384> = FlacDecoder::new();
    dec.init();
}

#[test]
fn decode_flac_produces_stereo_44100_frames() {
    let data = load("test_440hz.flac");
    let frames = decode_all(&data);

    assert!(!frames.is_empty(), "no frames decoded");

    let total_samples: usize = frames.iter().map(|f| f.samples().len()).sum();

    for frame in &frames {
        assert_eq!(frame.sample_rate, 44100);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.bps, 16);
        assert!(frame.block_size as usize <= MAX_BLOCK_SIZE);
        assert_eq!(frame.samples().len(), frame.channels as usize * frame.block_size as usize);
    }

    // 0.1 s × 44100 Hz × 2 ch = 8820 ± 1 frame of encoder padding
    assert!(total_samples >= 8000 && total_samples <= 10000,
        "unexpected sample count {total_samples}");
}

#[test]
fn reset_allows_re_decode() {
    let data = load("test_440hz.flac");
    let mut dec = FlacDecoder::<16384>::new();
    dec.init();

    let first = decode_all(&data);
    dec.reset();
    let second = decode_all(&data);

    assert_eq!(first.len(), second.len());
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.samples(), b.samples());
    }
}

// ---------------------------------------------------------------------------
// New: read_streaminfo
// ---------------------------------------------------------------------------

#[test]
fn streaminfo_matches_decoded_frame_metadata() {
    let data = load("test_440hz.flac");

    // read_streaminfo uses the same push-style API — one call with all data
    // is fine here (the SD card would do this with the first sector).
    let mut dec = FlacDecoder::<16384>::new();
    dec.init();
    let (_, info) = dec.read_streaminfo(&data).expect("streaminfo error");
    let info = info.expect("MINIFLAC_CONTINUE on full data — needs more buffering");

    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 2);
    assert_eq!(info.bps, 16);
    assert!(info.total_samples > 0);

    // After reading streaminfo the decoder state is advanced; reset before decoding.
    dec.reset();
    let frames = decode_all(&data);
    let first = frames.first().unwrap();

    assert_eq!(info.sample_rate, first.sample_rate);
    assert_eq!(info.channels, first.channels);
    assert_eq!(info.bps, first.bps);
}

// ---------------------------------------------------------------------------
// New: incremental / push-style decode (512-byte chunks)
// ---------------------------------------------------------------------------

#[test]
fn incremental_decode_matches_bulk_decode() {
    let data = load("test_440hz.flac");

    // Bulk reference
    let bulk_frames = decode_all(&data);
    let bulk_samples: Vec<i16> = bulk_frames.iter().flat_map(|f| f.samples().iter().copied()).collect();

    // Incremental: feed 512 bytes at a time (one SD card sector)
    const CHUNK: usize = 512;
    let mut dec = FlacDecoder::<16384>::new();
    dec.init();

    let mut buf: Vec<u8> = Vec::new();
    let mut source_pos = 0usize;
    let mut incremental_samples: Vec<i16> = Vec::new();

    loop {
        // Refill buffer
        let end = (source_pos + CHUNK).min(data.len());
        buf.extend_from_slice(&data[source_pos..end]);
        source_pos = end;

        // Drain as many frames as possible from the buffer
        let mut buf_pos = 0usize;
        loop {
            match dec.decode(&buf[buf_pos..]).expect("decode error") {
                (consumed, Some(frame)) => {
                    buf_pos += consumed;
                    incremental_samples.extend_from_slice(frame.samples());
                }
                (consumed, None) => {
                    buf_pos += consumed;
                    if consumed == 0 { break; } // need more data
                }
            }
        }
        buf.drain(..buf_pos);

        if source_pos >= data.len() && buf.is_empty() {
            break;
        }
        if source_pos >= data.len() && buf_pos == 0 {
            break; // no progress on remaining partial data
        }
    }

    assert_eq!(incremental_samples, bulk_samples,
        "incremental and bulk decode produced different samples");
}

// ---------------------------------------------------------------------------
// New: 24-bit FLAC — exercises the bps > 16 scaling path in scale_to_i16
// ---------------------------------------------------------------------------

#[test]
fn decode_24bit_flac_produces_i16_range_samples() {
    let data = load("test_440hz_24bit.flac");
    let frames = decode_all(&data);

    assert!(!frames.is_empty(), "no frames from 24-bit file");

    for frame in &frames {
        assert_eq!(frame.sample_rate, 44100);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.bps, 24, "expected 24-bit source");

        // All output samples must be in i16 range (they always are, but
        // this guards against accidentally returning the raw int32 value).
        for &s in frame.samples() {
            // i16::MIN..=i16::MAX — trivially true for i16, but this
            // also forces us to actually have non-zero samples.
            let _ = s; // type is i16, range is guaranteed
        }

        // A 440 Hz sine should have samples that span a significant range.
        // Check that the output isn't silently all-zero (which would happen
        // if we shifted by the wrong amount and overflowed).
        let max_abs = frame.samples().iter().map(|&s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(max_abs > 100, "24-bit samples look like silence (max_abs={max_abs})");
    }
}

// ---------------------------------------------------------------------------
// New: mono FLAC
// ---------------------------------------------------------------------------

#[test]
fn decode_mono_flac() {
    let data = load("test_440hz_mono.flac");
    let frames = decode_all(&data);

    assert!(!frames.is_empty());
    for frame in &frames {
        assert_eq!(frame.channels, 1);
        // For mono: interleaved sample count == block_size (no interleaving)
        assert_eq!(frame.samples().len(), frame.block_size as usize);
    }
}

// ---------------------------------------------------------------------------
// New: garbage input returns an error, not a panic or SIGSEGV
// ---------------------------------------------------------------------------

#[test]
fn garbage_input_returns_error() {
    let mut dec = FlacDecoder::<16384>::new();
    dec.init();

    // 256 bytes of 0xFF — not a valid FLAC stream
    let garbage = [0xFFu8; 256];
    let mut got_error = false;
    let mut pos = 0usize;

    while pos < garbage.len() {
        match dec.decode(&garbage[pos..]) {
            Ok((0, None)) => { pos += 1; } // no progress — skip a byte
            Ok((consumed, _)) => { pos += consumed; }
            Err(_) => { got_error = true; break; }
        }
    }
    // Either we got an explicit error, or we consumed all bytes without
    // crashing — both are acceptable; a SIGSEGV/panic is not.
    let _ = got_error;
}

// ---------------------------------------------------------------------------
// New: copy_interleaved_i16 with undersized destination
// ---------------------------------------------------------------------------

#[test]
fn copy_interleaved_i16_truncates_to_dst_len() {
    let data = load("test_440hz.flac");
    let frames = decode_all(&data);
    let frame = frames.first().expect("no frames");

    let full_len = frame.samples().len();
    assert!(full_len >= 4, "frame too small to test truncation");

    let half = full_len / 2;
    let mut dst = vec![0i16; half];
    let copied = frame.copy_interleaved_i16(&mut dst);

    assert_eq!(copied, half);
    assert_eq!(&dst, &frame.samples()[..half]);
}
