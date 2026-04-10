# miniflac-sys

Rust bindings for [miniflac](https://github.com/jprjr/miniflac) v1.1.3, a single-header C FLAC decoder.

`no_std` compatible. Enable the `std` feature for host-side use (tests, examples).

## Usage

```toml
[dependencies]
miniflac-sys = { path = "..." }
```

The decoder is **streaming-friendly**: `decode` consumes however many bytes it needs and
returns the count. You can feed it any size chunk - a full file, a network buffer, or a
single byte at a time - and call it repeatedly as more data arrives.

```rust
use miniflac_sys::FlacDecoder;

let mut decoder = FlacDecoder::new();
decoder.init();

// `buf` can be any slice — the full file, a partially-received network chunk, etc.
// Call decode() again with remaining data (or new incoming data) each iteration.
let mut buf: &[u8] = /* your FLAC bytes */;
while !buf.is_empty() {
    match decoder.decode(buf) {
        Ok((consumed, Some(frame))) => {
            let samples: &[i16] = frame.samples();
            // process samples...
            buf = &buf[consumed..];
        }
        Ok((consumed, None)) => buf = &buf[consumed.max(1)..],
        Err(e) => break,
    }
}
```

## Examples

Play a FLAC file through the default audio output:

```sh
cargo run --example play --features std -- path/to/file.flac
cargo run --example play --features std -- path/to/file.flac --volume 0.5
```

## Development

Requires [just](https://github.com/casey/just).

```sh
just          # run tests
just test     # run tests
just check    # cargo check
just clippy   # run clippy
just play tests/test_440hz.flac
```
