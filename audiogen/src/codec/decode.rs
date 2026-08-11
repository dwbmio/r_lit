//! Symphonia-backed decoder. Handles MP3 / OGG / FLAC through one trait impl.

use super::Pcm;
use anyhow::{anyhow, Result};
use symphonia::core::audio::{
    Audio, AudioBuffer, GenericAudioBufferRef,
};
use symphonia::core::codecs::{audio::AudioDecoderOptions, CodecParameters};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{probe::Hint, FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Decode any symphonia-supported format → interleaved i16 PCM.
pub fn decode_any(bytes: &[u8], hint_ext: Option<&str>) -> Result<Pcm> {
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = hint_ext {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| anyhow!("symphonia probe: {e:?}"))?;
    let mut format: Box<dyn symphonia::core::formats::FormatReader> = probed;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| anyhow!("no default audio track"))?
        .clone();
    let track_id = track.id;
    let params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(p)) => p.clone(),
        Some(_) => return Err(anyhow!("track is not audio")),
        None => return Err(anyhow!("track has no codec_params")),
    };
    let sample_rate = params
        .sample_rate
        .ok_or_else(|| anyhow!("unknown sample_rate"))?;
    let channels = params
        .channels
        .as_ref()
        .map(|c| c.count() as u16)
        .ok_or_else(|| anyhow!("unknown channels"))?;

    let dec_opts = AudioDecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &dec_opts)
        .map_err(|e| anyhow!("symphonia make_audio_decoder: {e:?}"))?;

    let mut samples: Vec<i16> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break, // EOF
            Err(SymError::IoError(_)) => break,
            Err(SymError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(anyhow!("symphonia next_packet: {e:?}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                append_interleaved_i16(&buf, &mut samples);
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(anyhow!("symphonia decode: {e:?}")),
        }
    }

    Ok(Pcm {
        samples,
        sample_rate,
        channels,
    })
}

fn append_interleaved_i16(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<i16>) {
    match buf {
        GenericAudioBufferRef::F32(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::S16(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::S32(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::S8(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::S24(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::U8(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::U16(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::U24(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::U32(b) => copy_i16_from(b, out),
        GenericAudioBufferRef::F64(b) => copy_i16_from(b, out),
    }
}

fn copy_i16_from<S>(buf: &AudioBuffer<S>, out: &mut Vec<i16>)
where
    S: symphonia::core::audio::sample::Sample + Copy,
    i16: symphonia::core::audio::conv::FromSample<S>,
{
    let frames = buf.frames();
    let chans = buf.spec().channels().count();
    let needed = frames * chans;
    let start = out.len();
    out.resize(start + needed, 0i16);
    buf.copy_to_slice_interleaved(&mut out[start..]);
}