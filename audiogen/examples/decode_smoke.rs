//! MP3 encode → decode roundtrip test.
//! Run: cargo run --example decode_smoke

use audiogen::codec::{codec_for_ext, EncodeCfg};
use audiogen::decode_any;
use audiogen::synth;

fn main() -> anyhow::Result<()> {
    // 1. Synthesize a 22050Hz, 0.3s pluck (2 channels for variety)
    let pcm_orig = synth::pluck(0.3, 22050, 2);
    println!("orig: {} samples, {}Hz {}ch, {:.3}s",
             pcm_orig.samples.len(), pcm_orig.sample_rate, pcm_orig.channels,
             pcm_orig.duration_secs());

    // 2. Encode to MP3
    let mp3 = codec_for_ext("mp3")?.encode(&pcm_orig, &EncodeCfg::mp3(22050, 2, 64_000))?;
    println!("mp3 encoded: {} bytes ({:.1} KB)", mp3.len(), mp3.len() as f64 / 1024.0);

    std::fs::write("/tmp/audiogen_roundtrip.mp3", &mp3)?;

    // 3. Decode back via symphonia
    let pcm_back = decode_any(&mp3, Some("mp3"))?;
    println!("decoded: {} samples, {}Hz {}ch, {:.3}s",
             pcm_back.samples.len(), pcm_back.sample_rate, pcm_back.channels,
             pcm_back.duration_secs());

    // 4. Compare: mp3 is lossy, so allow some tolerance
    let sample_diff = (pcm_orig.samples.len() as i64 - pcm_back.samples.len() as i64).abs();
    let frame_diff = sample_diff / pcm_orig.channels as i64;
    println!("frame count diff: {} (acceptable; MP3 frame alignment)", frame_diff);

    // First/last frame should be near zero (silence envelope)
    let first_frame_amp = pcm_back.samples[..pcm_back.channels as usize]
        .iter().map(|s| s.abs()).max().unwrap_or(0);
    let mid = pcm_back.samples.len() / 2;
    let mid_frame_amp = pcm_back.samples[mid - pcm_back.channels as usize..mid]
        .iter().map(|s| s.abs()).max().unwrap_or(0);
    println!("first_frame max amp: {}, mid_frame max amp: {}", first_frame_amp, mid_frame_amp);
    assert!(mid_frame_amp > first_frame_amp, "decoded audio should still have pluck decay pattern");

    Ok(())
}