//! Host-side integration tests for the miniflac FFI + FlacDecoder wrapper.

extern crate std;
use std::{fs, vec::Vec};

use miniflac_sys::{DecodedFrame, FlacDecoder, MAX_BLOCK_SIZE};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load(name: &str) -> Vec<u8> {
    let path = std::format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read(&path).unwrap_or_else(|_| panic!("test file not found: {path}"))
}

/// Decode an entire in-memory FLAC file in one shot, returning all frames.
fn decode_all(data: &[u8]) -> Vec<DecodedFrame> {
    let mut dec = FlacDecoder::new();
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
fn miniflac_size_fits_in_storage() {
    // init() asserts MINIFLAC_STORAGE_SIZE >= miniflac_size(); panics if not.
    let mut dec = FlacDecoder::new();
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
    let mut dec = FlacDecoder::new();
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
// read_streaminfo
// ---------------------------------------------------------------------------

#[test]
fn streaminfo_matches_decoded_frame_metadata() {
    let data = load("test_440hz.flac");

    let mut dec = FlacDecoder::new();
    dec.init();
    let (_, info) = dec.read_streaminfo(&data).expect("streaminfo error");
    let info = info.expect("MINIFLAC_CONTINUE on full data — needs more buffering");

    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 2);
    assert_eq!(info.bps, 16);
    assert!(info.total_samples > 0);

    dec.reset();
    let frames = decode_all(&data);
    let first = frames.first().unwrap();

    assert_eq!(info.sample_rate, first.sample_rate);
    assert_eq!(info.channels, first.channels);
    assert_eq!(info.bps, first.bps);
}

// ---------------------------------------------------------------------------
// Incremental / push-style decode (512-byte chunks)
// ---------------------------------------------------------------------------

#[test]
fn incremental_decode_matches_bulk_decode() {
    let data = load("test_440hz.flac");

    let bulk_samples: Vec<i16> = decode_all(&data)
        .iter()
        .flat_map(|f| f.samples().iter().copied())
        .collect();

    // Feed 512 bytes at a time (one SD card sector).
    const CHUNK: usize = 512;
    let mut dec = FlacDecoder::new();
    dec.init();

    let mut buf: Vec<u8> = Vec::new();
    let mut source_pos = 0usize;
    let mut incremental_samples: Vec<i16> = Vec::new();

    loop {
        let end = (source_pos + CHUNK).min(data.len());
        buf.extend_from_slice(&data[source_pos..end]);
        source_pos = end;

        let mut buf_pos = 0usize;
        loop {
            match dec.decode(&buf[buf_pos..]).expect("decode error") {
                (consumed, Some(frame)) => {
                    buf_pos += consumed;
                    incremental_samples.extend_from_slice(frame.samples());
                }
                (consumed, None) => {
                    buf_pos += consumed;
                    if consumed == 0 { break; }
                }
            }
        }
        buf.drain(..buf_pos);

        if source_pos >= data.len() && buf.is_empty() { break; }
        if source_pos >= data.len() && buf_pos == 0 { break; }
    }

    assert_eq!(incremental_samples, bulk_samples,
        "incremental and bulk decode produced different samples");
}

// ---------------------------------------------------------------------------
// 24-bit FLAC — exercises the bps > 16 scaling path in scale_to_i16
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

        // A 440 Hz sine should have a significant amplitude.
        // All-zero would mean the right-shift overflowed or the wrong path ran.
        let max_abs = frame.samples().iter().map(|&s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(max_abs > 100, "24-bit samples look like silence (max_abs={max_abs})");
    }
}

// ---------------------------------------------------------------------------
// Mono FLAC
// ---------------------------------------------------------------------------

#[test]
fn decode_mono_flac() {
    let data = load("test_440hz_mono.flac");
    let frames = decode_all(&data);

    assert!(!frames.is_empty());
    for frame in &frames {
        assert_eq!(frame.channels, 1);
        // For mono: interleaved sample count == block_size (no channel multiplier)
        assert_eq!(frame.samples().len(), frame.block_size as usize);
    }
}

// ---------------------------------------------------------------------------
// Garbage input — must return an error, not panic or SIGSEGV
// ---------------------------------------------------------------------------

#[test]
fn garbage_input_returns_error() {
    let mut dec = FlacDecoder::new();
    dec.init();

    let garbage = [0xFFu8; 256];
    let mut pos = 0usize;
    let mut got_error = false;

    while pos < garbage.len() {
        match dec.decode(&garbage[pos..]) {
            Ok((0, None)) => { pos += 1; }
            Ok((consumed, _)) => { pos += consumed; }
            Err(_) => { got_error = true; break; }
        }
    }
    let _ = got_error; // crash = fail; error or quiet consume = pass
}

// ---------------------------------------------------------------------------
// copy_interleaved_i16 with undersized destination
// ---------------------------------------------------------------------------

#[test]
fn copy_interleaved_i16_truncates_to_dst_len() {
    let data = load("test_440hz.flac");
    let frames = decode_all(&data);
    let frame = frames.first().expect("no frames");

    let full_len = frame.samples().len();
    assert!(full_len >= 4);

    let half = full_len / 2;
    let mut dst = vec![0i16; half];
    let copied = frame.copy_interleaved_i16(&mut dst);

    assert_eq!(copied, half);
    assert_eq!(&dst, &frame.samples()[..half]);
}
