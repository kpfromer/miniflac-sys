use clap::Parser;
use miniflac_sys::FlacDecoder;
use rodio::Source;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Decode and play a FLAC file")]
struct Args {
    file: std::path::PathBuf,
    #[arg(long, default_value_t = 1.0)]
    volume: f32,
}

struct FlacSource {
    data: Vec<u8>,
    pos: usize,
    decoder: FlacDecoder,
    frame_buf: Vec<i16>,
    frame_pos: usize,
    sample_rate: u32,
    channels: u16,
}

impl FlacSource {
    fn new(data: Vec<u8>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut decoder = FlacDecoder::new();
        decoder.init();
        let mut pos = 0usize;
        loop {
            if pos >= data.len() {
                return Err("no audio frames found".into());
            }
            match decoder.decode(&data[pos..]).map_err(|e| format!("{e:?}"))? {
                (consumed, Some(frame)) => {
                    let sr = frame.sample_rate;
                    let ch = frame.channels as u16;
                    let buf = frame.samples().to_vec();
                    pos += consumed;
                    return Ok(Self {
                        data,
                        pos,
                        decoder,
                        frame_buf: buf,
                        frame_pos: 0,
                        sample_rate: sr,
                        channels: ch,
                    });
                }
                (consumed, None) => {
                    pos += if consumed == 0 { 1 } else { consumed };
                }
            }
        }
    }

    fn fill_next_frame(&mut self) -> bool {
        loop {
            if self.pos >= self.data.len() {
                return false;
            }
            match self.decoder.decode(&self.data[self.pos..]) {
                Ok((consumed, Some(frame))) => {
                    self.frame_buf = frame.samples().to_vec();
                    self.frame_pos = 0;
                    self.pos += consumed;
                    return true;
                }
                Ok((consumed, None)) => {
                    self.pos += if consumed == 0 { 1 } else { consumed };
                }
                Err(_) => {
                    self.pos += 1;
                }
            }
        }
    }
}

impl Iterator for FlacSource {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        if self.frame_pos < self.frame_buf.len() {
            let s = self.frame_buf[self.frame_pos];
            self.frame_pos += 1;
            return Some(s);
        }
        if self.fill_next_frame() {
            let s = self.frame_buf[self.frame_pos];
            self.frame_pos += 1;
            Some(s)
        } else {
            None
        }
    }
}

impl Source for FlacSource {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.frame_buf.len() - self.frame_pos)
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let data = std::fs::read(&args.file)
        .map_err(|e| format!("cannot read {:?}: {e}", args.file))?;
    let source = FlacSource::new(data)?;
    let (_stream, stream_handle) = rodio::OutputStream::try_default()?;
    let sink = rodio::Sink::try_new(&stream_handle)?;
    sink.set_volume(args.volume.clamp(0.0, 1.0));
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
