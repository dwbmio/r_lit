//! Smoke test: synthesize a click → encode to MP3 → write to /tmp.
//! Run: cargo run --example smoke

use audiogen::codec::{codec_for_ext, EncodeCfg};
use audiogen::synth;

fn main() -> anyhow::Result<()> {
    let pcm = synth::woodblock(0.05, 22050, 1);
    println!("woodblock: {} samples, {}Hz {}ch, {:.3}s",
             pcm.samples.len(), pcm.sample_rate, pcm.channels, pcm.duration_secs());

    let codec = codec_for_ext("mp3")?;
    let cfg = EncodeCfg::mp3(22050, 1, 64_000);
    let mp3 = codec.encode(&pcm, &cfg)?;
    println!("mp3: {} bytes ({:.1} KB)", mp3.len(), mp3.len() as f64 / 1024.0);

    std::fs::write("/tmp/audiogen_smoke.mp3", &mp3)?;
    println!("written → /tmp/audiogen_smoke.mp3");
    Ok(())
}