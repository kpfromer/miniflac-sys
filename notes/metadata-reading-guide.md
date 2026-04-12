# Reading FLAC Metadata + Streaming Audio

## Block Order

FLAC files have metadata blocks before audio frames. STREAMINFO is always first, but the remaining blocks (VORBIS_COMMENT, PICTURE, PADDING, SEEKTABLE, etc.) can appear in **any order** depending on the encoder.

## Key Rules

1. **STREAMINFO is always the first block** -- read it unconditionally
2. **Other metadata blocks can appear in any order** -- try each reader and handle the `Miniflac(-1)` error as "wrong block type, skip it"
3. **`read_picture_data()` must be called immediately after `read_picture_info()`** -- they are sequential reads of the same block
4. **Once `decode()` returns a frame, you've left metadata territory** -- all metadata reading must happen before the first audio frame
5. On ESP32 with streaming from SD card, feed chunks incrementally and handle `None`/CONTINUE returns by reading more data, rather than having the whole file in memory

## Example: Read Tags + Album Art, Then Stream

```rust
use miniflac_sys::{FlacDecoder, StreamInfo, VorbisComments, PictureInfo};

let mut dec = FlacDecoder::new();
dec.init();

let data: &[u8] = /* your full file or buffered data */;
let mut pos = 0usize;

// 1. STREAMINFO is always first
let (consumed, info) = dec.read_streaminfo(&data[pos..]).unwrap();
let info = info.unwrap();
pos += consumed;

// 2. Walk remaining metadata blocks in file order.
//    sync() advances to the next block boundary.
//    Then try each reader -- if it returns Miniflac(-1), that block
//    isn't the type you called, so skip it by syncing again.

let mut comments: Option<VorbisComments<128, 256, 16>> = None;
let mut picture_data: Option<Vec<u8>> = None;
let mut picture_info: Option<PictureInfo<32, 64>> = None;

loop {
    // Sync to next metadata/frame boundary
    let (consumed, ready) = dec.sync(&data[pos..]).unwrap();
    pos += consumed;
    if !ready { break; }

    // Try reading as vorbis comments
    if comments.is_none() {
        match dec.read_vorbis_comments::<128, 256, 16>(&data[pos..]) {
            Ok((consumed, Some(vc))) => {
                pos += consumed;
                comments = Some(vc);
                continue;
            }
            Ok((consumed, None)) => { pos += consumed; continue; }
            Err(_) => {} // not a vorbis comment block, try picture
        }
    }

    // Try reading as picture
    if picture_info.is_none() {
        match dec.read_picture_info::<32, 64>(&data[pos..]) {
            Ok((consumed, Some(pi))) => {
                pos += consumed;
                // Read the image data immediately after info
                let mut buf = vec![0u8; pi.data_length as usize];
                let (consumed, _written) = dec
                    .read_picture_data(&data[pos..], &mut buf)
                    .unwrap();
                pos += consumed;
                picture_data = Some(buf);
                picture_info = Some(pi);
                continue;
            }
            Ok((consumed, None)) => { pos += consumed; continue; }
            Err(_) => {} // not a picture block (padding, seektable, etc.)
        }
    }

    // Unknown/unhandled block type -- try to decode as audio.
    // If decode returns a frame, we've hit audio data.
    match dec.decode(&data[pos..]) {
        Ok((consumed, Some(_frame))) => {
            // Reached audio frames -- metadata is done.
            break;
        }
        Ok((consumed, None)) => { pos += consumed; }
        Err(_) => break,
    }
}

// 3. Stream audio frames
loop {
    match dec.decode(&data[pos..]).unwrap() {
        (consumed, Some(frame)) => {
            pos += consumed;
            let samples = frame.samples(); // interleaved i16 PCM
            // send to I2S / DAC / ringbuffer
        }
        (consumed, None) => {
            if consumed == 0 { break; }
            pos += consumed;
        }
    }
}
```

## Accessing Parsed Metadata

```rust
// Vorbis comments (tags)
if let Some(ref vc) = comments {
    // vc.vendor -- encoder string (e.g. "Lavf62.3.100")
    // vc.comments -- Vec of "KEY=value" byte strings
    // vc.total_in_file -- total comment count (may exceed buffer capacity)
    for comment in &vc.comments {
        if let Ok(s) = core::str::from_utf8(comment) {
            // s is e.g. "artist=Test Artist" or "title=Test Title"
        }
    }
}

// Picture info
if let Some(ref pi) = picture_info {
    // pi.picture_type -- 3 = front cover (see FLAC spec)
    // pi.mime -- e.g. "image/png"
    // pi.width, pi.height
    // pi.data_length -- size of image data in bytes
}

// Picture data is in picture_data: Option<Vec<u8>>
```

## Const Generic Buffer Sizes

The structs use const generics to control stack allocation:

- `VorbisComments<V, C, N>` -- V=vendor bytes, C=bytes per comment, N=max comments
- `PictureInfo<M, D>` -- M=MIME bytes, D=description bytes

Default aliases are provided: `DefaultVorbisComments` (128/256/16) and `DefaultPictureInfo` (32/64).

For ESP32 with limited stack, keep these small or allocate the decoder in a static.
