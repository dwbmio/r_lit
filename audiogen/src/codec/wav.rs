//! WAV codec — hand-rolled RIFF/WAVE writer + reader, zero dependencies.
//!
//! Supports: PCM 16-bit little-endian, mono or stereo.

use super::{Codec, EncodeCfg, Pcm};
use anyhow::{anyhow, Result};
use std::io::{Cursor, Read};

pub struct WavCodec;

const PCM_FORMAT: u16 = 0x0001;
const BITS_PER_SAMPLE: u16 = 16;

impl Codec for WavCodec {
    fn ext(&self) -> &'static str { "wav" }
    fn name(&self) -> &'static str { "wav (RIFF/WAVE PCM 16-bit)" }

    fn encode(&self, pcm: &Pcm, cfg: &EncodeCfg) -> Result<Vec<u8>> {
        let data_bytes = (pcm.samples.len() as u32) * 2;
        let byte_rate = cfg.sample_rate * cfg.channels as u32 * BITS_PER_SAMPLE as u32 / 8;
        let block_align = cfg.channels * BITS_PER_SAMPLE / 8;

        let mut buf: Vec<u8> = Vec::with_capacity(44 + data_bytes as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&PCM_FORMAT.to_le_bytes());
        buf.extend_from_slice(&cfg.channels.to_le_bytes());
        buf.extend_from_slice(&cfg.sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_bytes.to_le_bytes());
        for s in &pcm.samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        Ok(buf)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Pcm> {
        let mut cur = Cursor::new(bytes);
        let mut riff = [0u8; 4]; cur.read_exact(&mut riff)?;
        if &riff != b"RIFF" { return Err(anyhow!("wav: missing RIFF header")); }
        let _chunk_size = read_u32_le(&mut cur)?;
        let mut wave = [0u8; 4]; cur.read_exact(&mut wave)?;
        if &wave != b"WAVE" { return Err(anyhow!("wav: missing WAVE marker")); }

        let mut sample_rate = 0u32;
        let mut channels = 0u16;
        let mut bits_per_sample = 0u16;
        let mut data_chunk: Option<(u64, u32)> = None;

        loop {
            let mut id = [0u8; 4];
            match cur.read_exact(&mut id) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(anyhow!("wav: read chunk id: {e}")),
            }
            let size = read_u32_le(&mut cur)?;
            let chunk_start = cur.position();
            match &id {
                b"fmt " => {
                    let fmt_code = read_u16_le(&mut cur)?;
                    if fmt_code != PCM_FORMAT {
                        return Err(anyhow!("wav: only PCM (format=1) supported, got {fmt_code}"));
                    }
                    channels = read_u16_le(&mut cur)?;
                    sample_rate = read_u32_le(&mut cur)?;
                    let _byte_rate = read_u32_le(&mut cur)?;
                    let _block_align = read_u16_le(&mut cur)?;
                    bits_per_sample = read_u16_le(&mut cur)?;
                }
                b"data" => {
                    data_chunk = Some((chunk_start, size));
                }
                _ => {}
            }
            let aligned = (size + 1) & !1;
            cur.set_position(chunk_start + aligned as u64);
        }

        if bits_per_sample != BITS_PER_SAMPLE {
            return Err(anyhow!("wav: only 16-bit PCM supported, got {bits_per_sample}"));
        }
        let (data_off, data_size) = data_chunk.ok_or_else(|| anyhow!("wav: no data chunk"))?;

        let sample_count = (data_size / 2) as usize;
        let mut samples = Vec::with_capacity(sample_count);
        cur.set_position(data_off);
        for _ in 0..sample_count {
            samples.push(read_i16_le(&mut cur)?);
        }
        Ok(Pcm { samples, sample_rate, channels })
    }
}

fn read_u32_le(cur: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut b = [0u8; 4]; cur.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn read_u16_le(cur: &mut Cursor<&[u8]>) -> Result<u16> {
    let mut b = [0u8; 2]; cur.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}
fn read_i16_le(cur: &mut Cursor<&[u8]>) -> Result<i16> {
    let mut b = [0u8; 2]; cur.read_exact(&mut b)?;
    Ok(i16::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let pcm = Pcm {
            samples: vec![0, 1000, -1000, i16::MAX, i16::MIN, 0, 500, -500],
            sample_rate: 22050,
            channels: 1,
        };
        let bytes = WavCodec.encode(&pcm, &EncodeCfg::wav(22050, 1)).unwrap();
        let back = WavCodec.decode(&bytes).unwrap();
        assert_eq!(back.samples, pcm.samples);
        assert_eq!(back.sample_rate, pcm.sample_rate);
        assert_eq!(back.channels, pcm.channels);
    }
}