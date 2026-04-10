/// Hand-written FFI bindings for miniflac v1.1.3 + safe FlacDecoder wrapper.
///
/// # Corrected API model
/// `miniflac_decode`'s `samples` parameter is an array of **caller-provided**
/// `int32_t*` output buffers (one per channel). miniflac writes decoded int32
/// samples into those buffers; it does NOT maintain internal sample buffers.
/// Our `FlacDecoder<S>` therefore holds its own per-channel `[i32; MAX_BLOCK_SIZE]`
/// storage alongside the opaque `miniflac_t` state.
///
/// All `unsafe` FFI is contained inside `FlacDecoder::decode()` and
/// `FlacDecoder::sync()`. The public `DecodedFrame` owns its (copied) i16 samples,
/// so everything above that boundary is fully safe.

// ---------------------------------------------------------------------------
// FFI — cross-checked against miniflac/miniflac.h v1.1.3
// ---------------------------------------------------------------------------
mod ffi {
    use core::ffi::c_int;

    /// Opaque placeholder for `miniflac_t`. We allocate real storage in
    /// `AlignedStorage<S>` and cast a raw pointer to this type for FFI calls.
    #[repr(C)]
    pub(super) struct Miniflac {
        _private: [u8; 0],
    }

    /// Return type of miniflac_sync / miniflac_decode.
    /// MINIFLAC_OK = 1, MINIFLAC_CONTINUE = 0, errors are negative.
    pub(super) type MiniflacResult = i32;
    pub(super) const MINIFLAC_OK: MiniflacResult = 1;
    pub(super) const MINIFLAC_CONTINUE: MiniflacResult = 0;

    /// MINIFLAC_CONTAINER enum values (underlying type is C `int`).
    pub(super) const MINIFLAC_CONTAINER_NATIVE: c_int = 1;

    extern "C" {
        /// Returns sizeof(miniflac_t). Used to verify our storage is large enough.
        pub(super) fn miniflac_size() -> usize;

        /// Initialise a miniflac_t in caller-allocated storage.
        /// `container`: MINIFLAC_CONTAINER_NATIVE (1) for native .flac files.
        pub(super) fn miniflac_init(flac: *mut Miniflac, container: c_int);

        /// Advance the decoder to the next metadata or audio frame boundary.
        /// Returns MINIFLAC_OK on success, MINIFLAC_CONTINUE if more data needed.
        pub(super) fn miniflac_sync(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
        ) -> MiniflacResult;

        /// Decode one audio frame.
        ///
        /// `samples` is an array of caller-provided `int32_t*` output buffers,
        /// one per channel. miniflac WRITES int32 PCM samples into each buffer.
        /// Each buffer must hold at least `block_size` elements (query with
        /// `miniflac_frame_block_size` after a successful decode).
        ///
        /// Returns MINIFLAC_OK on a complete frame, MINIFLAC_CONTINUE if more
        /// data is needed, or a negative error code.
        pub(super) fn miniflac_decode(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
            samples: *mut *mut i32,
        ) -> MiniflacResult;

        // --- Frame info accessors (valid after MINIFLAC_OK from miniflac_decode) ---
        pub(super) fn miniflac_frame_block_size(flac: *mut Miniflac) -> u16;
        pub(super) fn miniflac_frame_sample_rate(flac: *mut Miniflac) -> u32;
        pub(super) fn miniflac_frame_channels(flac: *mut Miniflac) -> u8;
        pub(super) fn miniflac_frame_bps(flac: *mut Miniflac) -> u8;

        // --- STREAMINFO metadata readers (push-style, callable before audio frames) ---
        pub(super) fn miniflac_streaminfo_sample_rate(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
            sample_rate: *mut u32,
        ) -> MiniflacResult;

        pub(super) fn miniflac_streaminfo_channels(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
            channels: *mut u8,
        ) -> MiniflacResult;

        pub(super) fn miniflac_streaminfo_bps(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
            bps: *mut u8,
        ) -> MiniflacResult;

        pub(super) fn miniflac_streaminfo_total_samples(
            flac: *mut Miniflac,
            data: *const u8,
            length: u32,
            out_length: *mut u32,
            total_samples: *mut u64,
        ) -> MiniflacResult;
    }
}

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Maximum FLAC block size for the streamable subset at ≤48 kHz.
pub const MAX_BLOCK_SIZE: usize = 4608;

/// Maximum supported channel count (stereo files only).
pub const MAX_CHANNELS: usize = 2;

/// Maximum interleaved i16 samples in one decoded frame.
pub const MAX_SAMPLES_PER_FRAME: usize = MAX_BLOCK_SIZE * MAX_CHANNELS;

// ---------------------------------------------------------------------------
// DecodedFrame
// ---------------------------------------------------------------------------

/// One decoded FLAC audio frame — owns its interleaved i16 PCM data.
///
/// Source bit depths other than 16 are scaled to fill the i16 range:
/// bit depths > 16 are right-shifted; < 16 are left-shifted.
pub struct DecodedFrame {
    pub sample_rate: u32,
    pub channels: u8,
    pub bps: u8,
    pub block_size: u16,
    sample_count: usize,
    samples: [i16; MAX_SAMPLES_PER_FRAME],
}

impl DecodedFrame {
    /// Interleaved i16 PCM (length = channels × block_size).
    #[inline]
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.sample_count]
    }

    /// Copy interleaved i16 samples into `dst`. Returns the count copied.
    pub fn copy_interleaved_i16(&self, dst: &mut [i16]) -> usize {
        let n = self.sample_count.min(dst.len());
        dst[..n].copy_from_slice(&self.samples[..n]);
        n
    }

}

// ---------------------------------------------------------------------------
// FlacError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlacError {
    /// Raw miniflac error code (always negative).
    Miniflac(i32),
    /// Input slice length exceeds u32::MAX.
    InputTooLong,
    /// Decoded frame had more channels than MAX_CHANNELS.
    TooManyChannels(u8),
    /// Decoded block size exceeded MAX_BLOCK_SIZE.
    BlockSizeTooLarge(u16),
}

// ---------------------------------------------------------------------------
// Internal storage size for miniflac_t
// ---------------------------------------------------------------------------

/// miniflac_size() == 560 bytes for v1.1.3. We round up to the next power of
/// two (1024) so there is comfortable headroom for future miniflac versions
/// without exposing the size as a public API surface.
const MINIFLAC_STORAGE_SIZE: usize = 1024;

/// 8-byte aligned opaque storage for miniflac_t (contains uint64_t members).
#[repr(C, align(8))]
struct MiniflacStorage([u8; MINIFLAC_STORAGE_SIZE]);

// ---------------------------------------------------------------------------
// FlacDecoder
// ---------------------------------------------------------------------------

/// Zero-allocation FLAC decoder backed by miniflac v1.1.3.
///
/// Call `init()` before any other method. `new()` is `const` so the decoder
/// can live in a `static` (e.g. via `static_cell::StaticCell`).
pub struct FlacDecoder {
    /// Opaque miniflac_t state (560 bytes measured; 1024 allocated).
    storage: MiniflacStorage,
    /// Per-channel int32 output buffers that miniflac writes into.
    channel_bufs: [[i32; MAX_BLOCK_SIZE]; MAX_CHANNELS],
    initialized: bool,
}

impl FlacDecoder {
    /// Construct an uninitialised decoder. Call `init()` before use.
    pub const fn new() -> Self {
        Self {
            storage: MiniflacStorage([0u8; MINIFLAC_STORAGE_SIZE]),
            channel_bufs: [[0i32; MAX_BLOCK_SIZE]; MAX_CHANNELS],
            initialized: false,
        }
    }

    /// Initialise (or re-initialise) the miniflac decoder for native FLAC.
    /// Asserts at runtime that MINIFLAC_STORAGE_SIZE >= miniflac_size().
    pub fn init(&mut self) {
        let required = unsafe { ffi::miniflac_size() };
        assert!(
            MINIFLAC_STORAGE_SIZE >= required,
            "MINIFLAC_STORAGE_SIZE={MINIFLAC_STORAGE_SIZE} < miniflac_size()={required}; bump the constant"
        );
        unsafe {
            ffi::miniflac_init(self.flac_ptr(), ffi::MINIFLAC_CONTAINER_NATIVE);
        }
        self.initialized = true;
    }

    /// Reset decoder to initial state (use when starting a new file).
    #[inline]
    pub fn reset(&mut self) {
        self.init();
    }

    #[inline]
    fn flac_ptr(&mut self) -> *mut ffi::Miniflac {
        self.storage.0.as_mut_ptr() as *mut ffi::Miniflac
    }

    // -----------------------------------------------------------------------
    // sync
    // -----------------------------------------------------------------------

    /// Advance the decoder to the next metadata or frame boundary.
    ///
    /// Returns `(bytes_consumed, true)` on success, `(bytes_consumed, false)`
    /// when more data is needed (MINIFLAC_CONTINUE).
    pub fn sync(&mut self, data: &[u8]) -> Result<(usize, bool), FlacError> {
        debug_assert!(self.initialized, "FlacDecoder::sync called before init()");
        if data.len() > u32::MAX as usize {
            return Err(FlacError::InputTooLong);
        }
        let mut out_len: u32 = 0;
        let r = unsafe {
            ffi::miniflac_sync(
                self.flac_ptr(),
                data.as_ptr(),
                data.len() as u32,
                &mut out_len,
            )
        };
        match r {
            ffi::MINIFLAC_OK => Ok((out_len as usize, true)),
            ffi::MINIFLAC_CONTINUE => Ok((out_len as usize, false)),
            e => Err(FlacError::Miniflac(e)),
        }
    }

    // -----------------------------------------------------------------------
    // decode
    // -----------------------------------------------------------------------

    /// Decode one FLAC audio frame from `data`.
    ///
    /// Returns `(bytes_consumed, Some(frame))` on a complete frame, or
    /// `(bytes_consumed, None)` when more data is needed (MINIFLAC_CONTINUE).
    ///
    /// Samples are converted from miniflac's int32 output to interleaved i16
    /// before returning, so the `DecodedFrame` is fully owned and safe.
    pub fn decode(&mut self, data: &[u8]) -> Result<(usize, Option<DecodedFrame>), FlacError> {
        debug_assert!(self.initialized, "FlacDecoder::decode called before init()");
        if data.len() > u32::MAX as usize {
            return Err(FlacError::InputTooLong);
        }

        // Build the array of per-channel output pointers for miniflac.
        // miniflac writes int32 samples into channel_bufs[c] for channel c.
        let channel_ptrs: [*mut i32; MAX_CHANNELS] = [
            self.channel_bufs[0].as_mut_ptr(),
            self.channel_bufs[1].as_mut_ptr(),
        ];

        let mut out_len: u32 = 0;

        // SAFETY: channel_ptrs[c] are valid for MAX_BLOCK_SIZE elements.
        // miniflac will write at most block_size (≤ MAX_BLOCK_SIZE) samples
        // per channel. The storage is exclusively borrowed for this call.
        let r = unsafe {
            ffi::miniflac_decode(
                self.flac_ptr(),
                data.as_ptr(),
                data.len() as u32,
                &mut out_len,
                channel_ptrs.as_ptr() as *mut *mut i32,
            )
        };

        let consumed = out_len as usize;

        match r {
            ffi::MINIFLAC_CONTINUE => Ok((consumed, None)),
            ffi::MINIFLAC_OK => {
                // Query frame metadata — valid immediately after MINIFLAC_OK.
                let channels = unsafe { ffi::miniflac_frame_channels(self.flac_ptr()) };
                let block_size = unsafe { ffi::miniflac_frame_block_size(self.flac_ptr()) };
                let sample_rate = unsafe { ffi::miniflac_frame_sample_rate(self.flac_ptr()) };
                let bps = unsafe { ffi::miniflac_frame_bps(self.flac_ptr()) };

                if channels as usize > MAX_CHANNELS {
                    return Err(FlacError::TooManyChannels(channels));
                }
                if block_size as usize > MAX_BLOCK_SIZE {
                    return Err(FlacError::BlockSizeTooLarge(block_size));
                }

                // Convert int32 → interleaved i16.
                let mut frame = DecodedFrame {
                    sample_rate,
                    channels,
                    bps,
                    block_size,
                    sample_count: 0,
                    samples: [0i16; MAX_SAMPLES_PER_FRAME],
                };

                let mut idx = 0usize;
                for s in 0..block_size as usize {
                    for c in 0..channels as usize {
                        // SAFETY: miniflac has written block_size samples per channel.
                        let raw = self.channel_bufs[c][s];
                        frame.samples[idx] = scale_to_i16(raw, bps);
                        idx += 1;
                    }
                }
                frame.sample_count = idx;

                Ok((consumed, Some(frame)))
            }
            e => Err(FlacError::Miniflac(e)),
        }
    }

    // -----------------------------------------------------------------------
    // read_streaminfo
    // -----------------------------------------------------------------------

    /// Read STREAMINFO fields from the beginning of the stream.
    ///
    /// The caller should buffer at least ~42 bytes (4-byte "fLaC" marker +
    /// 4-byte metadata header + 34-byte STREAMINFO body). In practice a
    /// single 512-byte SD card sector always contains the full STREAMINFO.
    ///
    /// Returns `(bytes_consumed, Some(info))` on success, or
    /// `(bytes_consumed, None)` if more data is needed (MINIFLAC_CONTINUE).
    pub fn read_streaminfo(&mut self, data: &[u8]) -> Result<(usize, Option<StreamInfo>), FlacError> {
        debug_assert!(self.initialized);
        if data.len() > u32::MAX as usize {
            return Err(FlacError::InputTooLong);
        }

        // Each miniflac_streaminfo_* function resets br.pos = 0 internally, so
        // every call must receive a slice starting at the current byte offset.
        // Leftover bits in br.val/br.bits carry across calls automatically.
        let mut offset = 0usize;
        let flac = self.flac_ptr();

        macro_rules! read_field {
            ($fn:ident, $ty:ty) => {{
                let slice = &data[offset..];
                let mut val: $ty = 0;
                let mut consumed: u32 = 0;
                let r = unsafe {
                    ffi::$fn(flac, slice.as_ptr(), slice.len() as u32, &mut consumed, &mut val)
                };
                match r {
                    ffi::MINIFLAC_OK => { offset += consumed as usize; val }
                    ffi::MINIFLAC_CONTINUE => return Ok((offset + consumed as usize, None)),
                    e => return Err(FlacError::Miniflac(e)),
                }
            }};
        }

        let sample_rate   = read_field!(miniflac_streaminfo_sample_rate, u32);
        let channels      = read_field!(miniflac_streaminfo_channels, u8);
        let bps           = read_field!(miniflac_streaminfo_bps, u8);
        let total_samples = read_field!(miniflac_streaminfo_total_samples, u64);

        Ok((offset, Some(StreamInfo { sample_rate, channels, bps, total_samples })))
    }
}

// ---------------------------------------------------------------------------
// StreamInfo
// ---------------------------------------------------------------------------

/// Fields from the FLAC STREAMINFO metadata block.
#[derive(Debug, Clone, Copy)]
pub struct StreamInfo {
    pub sample_rate: u32,
    pub channels: u8,
    pub bps: u8,
    pub total_samples: u64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Scale a raw int32 FLAC sample (range ±2^(bps−1)) to i16.
#[inline]
fn scale_to_i16(sample: i32, bps: u8) -> i16 {
    if bps == 16 {
        sample as i16
    } else if bps > 16 {
        (sample >> (bps - 16)) as i16
    } else {
        // bps < 16: shift left to fill the i16 dynamic range
        (sample << (16 - bps)) as i16
    }
}
