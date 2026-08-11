//! Integration tests for audiogen.
//!
//! Coverage:
//! - Codec: WAV roundtrip (encode → decode → equal), MP3 lossy roundtrip
//! - Synth: 6 generators produce non-empty output with correct shape
//! - Jobs: load + validate the classroom.json + dry-run build_kwargs
//! - Pipeline: load_pending / save_pending roundtrip on disk
//! - Format conversion: WAV → MP3 via two codecs
//! - Decode: MP3 → PCM via symphonia

use audiogen::codec::{codec_for_ext, decode_any, EncodeCfg, Pcm};
use audiogen::jobs::{JobKind, JobsFile};
use audiogen::pipeline;
use audiogen::synth;
use std::path::PathBuf;

fn classroom_jobs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("jobs/classroom.json")
}

fn racing_jobs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("jobs/racing.json")
}

// ── Codec tests ──

#[test]
fn wav_roundtrip_lossless() {
    let original = synth::pluck(0.2, 22050, 1);
    let bytes = codec_for_ext("wav").unwrap().encode(&original, &EncodeCfg::wav(22050, 1)).unwrap();
    let decoded = codec_for_ext("wav").unwrap().decode(&bytes).unwrap();
    assert_eq!(decoded.samples, original.samples, "WAV must be bit-exact");
    assert_eq!(decoded.sample_rate, 22050);
    assert_eq!(decoded.channels, 1);
}

#[test]
fn mp3_encode_is_valid() {
    let pcm = synth::pluck(0.5, 22050, 1);
    let mp3 = codec_for_ext("mp3").unwrap().encode(&pcm, &EncodeCfg::mp3(22050, 1, 64_000)).unwrap();
    // MP3 sync word: first byte 0xFF, second 0xFB / 0xF3 / 0xF2 (varies by MPEG version).
    assert_eq!(mp3[0], 0xFF, "MP3 frame sync byte 0");
    assert!(mp3[1] & 0xE0 == 0xE0, "MP3 frame sync byte 1 (top 3 bits set)");
}

#[test]
fn mp3_decode_via_symphonia() {
    let pcm = synth::pluck(0.3, 22050, 2);
    let mp3 = codec_for_ext("mp3").unwrap().encode(&pcm, &EncodeCfg::mp3(22050, 2, 96_000)).unwrap();
    let back = decode_any(&mp3, Some("mp3")).unwrap();
    assert_eq!(back.channels, 2);
    assert_eq!(back.sample_rate, 22050);
    // MP3 is lossy: frame count differs by ≤ 1 frame (≈26ms @ 22050Hz / 1152 samples).
    let diff = (pcm.samples.len() as i64 - back.samples.len() as i64).abs();
    assert!(diff <= 2304, "frame count drift too large: {diff}");
    // Decoded should be mostly silence at start (pluck attack envelope).
    assert_eq!(back.samples[0], 0, "pluck first sample must be silent (attack ramp from 0)");
}

#[test]
fn format_conversion_wav_to_mp3() {
    // Make a WAV first.
    let pcm = synth::fanfare(0.5, 44100, 1);
    let wav = codec_for_ext("wav").unwrap().encode(&pcm, &EncodeCfg::wav(44100, 1)).unwrap();
    // Decode WAV → re-encode as MP3 (simulating transcoding).
    let from_wav = codec_for_ext("wav").unwrap().decode(&wav).unwrap();
    let to_mp3 = codec_for_ext("mp3").unwrap().encode(&from_wav, &EncodeCfg::mp3(44100, 1, 96_000)).unwrap();
    // Decode MP3 back → verify channels/rate preserved.
    let final_pcm = decode_any(&to_mp3, Some("mp3")).unwrap();
    assert_eq!(final_pcm.channels, 1);
    assert_eq!(final_pcm.sample_rate, 44100);
    assert!(!final_pcm.samples.is_empty());
    assert!(to_mp3.len() < wav.len(), "MP3 should be smaller than WAV");
}

#[test]
fn ogg_encode_returns_not_implemented() {
    let pcm = synth::woodblock(0.05, 22050, 1);
    let r = codec_for_ext("ogg").unwrap().encode(&pcm, &EncodeCfg::mp3(22050, 1, 64_000));
    assert!(r.is_err(), "OGG encode must error in v1");
    assert!(r.unwrap_err().to_string().contains("not implemented"));
}

// ── Synth tests ──

#[test]
fn all_synth_generators_produce_pcm() {
    let cases: &[(&str, f32, u32, u16)] = &[
        ("woodblock", 0.05, 22050, 1),
        ("ding",      1.0,  22050, 1),
        ("pluck",     0.3,  22050, 1),
        ("thud",      0.4,  22050, 1),
        ("fanfare",   0.8,  22050, 1),
        ("noise",     0.5,  22050, 1),
    ];
    for (name, dur, sr, ch) in cases {
        let p = synth::synthesize(name, *dur, *sr, *ch).unwrap();
        assert_eq!(p.samples.len(), (*dur * *sr as f32) as usize * *ch as usize, "{}: wrong sample count", name);
        assert_eq!(p.sample_rate, *sr, "{}: wrong sample_rate", name);
        assert_eq!(p.channels, *ch, "{}: wrong channels", name);
    }
}

#[test]
fn unknown_synth_returns_err() {
    let r = synth::synthesize("nonexistent", 0.1, 22050, 1);
    assert!(r.is_err());
}

// ── Jobs tests ──

#[test]
fn jobs_file_loads_classroom_json() {
    let jf = JobsFile::load(&classroom_jobs_path()).unwrap();
    assert_eq!(jf.game, "classroom");
    assert_eq!(jf.output_dir, "origin_res/classroom/audio");
    assert_eq!(jf.install_dir, Some("classroom/classroom/audio".to_string()));
    assert!(jf.style.is_some(), "must have style baseline");
    assert!(jf.voice_id.is_some(), "must have default voice_id");
    assert_eq!(jf.jobs.len(), 10, "10 standard items");
}

#[test]
fn jobs_kind_distribution() {
    let jf = JobsFile::load(&classroom_jobs_path()).unwrap();
    let mut music = 0;
    let mut voice = 0;
    let mut proc = 0;
    for j in &jf.jobs {
        match j.kind {
            JobKind::Music => music += 1,
            JobKind::Voice => voice += 1,
            JobKind::Procedural => proc += 1,
        }
    }
    assert_eq!(music, 7, "7 music (bgm_lobby, bgm_main, sfx_correct/wrong/fail/success, sfx_finish)");
    assert_eq!(voice, 1, "1 voice (voice_hint)");
    assert_eq!(proc, 2, "2 procedural (sfx_click, sfx_chain)");
}

#[test]
fn build_kwargs_music_prepends_style() {
    let jf = JobsFile::load(&classroom_jobs_path()).unwrap();
    let bgm = jf.jobs.iter().find(|j| j.name == "bgm_lobby").unwrap();
    let kw = pipeline::build_kwargs(&jf, bgm).unwrap();
    assert_eq!(kw["mode"], "music");
    let prompt = kw["prompt"].as_str().unwrap();
    assert!(prompt.contains("xianxia"), "style should be prepended: {prompt}");
    assert!(prompt.contains("calm lobby"), "job prompt should follow style: {prompt}");
    assert_eq!(kw["sample_rate"], 32000);
    assert_eq!(kw["bitrate"], 96000);
    assert_eq!(kw["channels"], 2);
}

#[test]
fn build_kwargs_voice_uses_text() {
    let jf = JobsFile::load(&classroom_jobs_path()).unwrap();
    let v = jf.jobs.iter().find(|j| j.name == "voice_hint").unwrap();
    let kw = pipeline::build_kwargs(&jf, v).unwrap();
    assert_eq!(kw["mode"], "voice");
    assert_eq!(kw["text"], "再想想看");
    assert_eq!(kw["voice_id"], "female-shaonv");
}

#[test]
fn build_kwargs_rejects_procedural() {
    let jf = JobsFile::load(&classroom_jobs_path()).unwrap();
    let p = jf.jobs.iter().find(|j| j.name == "sfx_click").unwrap();
    let r = pipeline::build_kwargs(&jf, p);
    assert!(r.is_err());
}

// ── R2 / S3 upload tests ──

use audiogen::r2::default_key;

#[test]
fn r2_default_key_pattern() {
    assert_eq!(default_key("classroom", "mp3", "bgm_lobby"),
               "gamesci-classroom/mp3/bgm_lobby.mp3");
    assert_eq!(default_key("racing", "ogg", "sfx_engine"),
               "gamesci-racing/ogg/sfx_engine.ogg");
}

#[test]
fn r2_credential_parsing() {
    // parse_s3_env is private; exercise via load_env_or_file with explicit path.
    use std::io::Write;
    let mut tmp = tempdir();
    let s3 = tmp.0.join(".s3");
    let mut f = std::fs::File::create(&s3).unwrap();
    writeln!(f, "# R2 prod credentials").unwrap();
    writeln!(f, "S3_BUCKET=test-bucket").unwrap();
    writeln!(f, "S3_ACCESS_KEY=AKIA...").unwrap();
    writeln!(f, "S3_SECRET_KEY=secret...").unwrap();
    writeln!(f, "S3_ENDPOINT=https://x.r2.cloudflarestorage.com").unwrap();
    writeln!(f, "S3_REGION=auto").unwrap();
    let (b, ak, sk, e, r) = audiogen::r2::R2Client::load_env_or_file(Some(&s3)).unwrap();
    assert_eq!(b, "test-bucket");
    assert_eq!(ak, "AKIA...");
    assert_eq!(sk, "secret...");
    assert_eq!(e, "https://x.r2.cloudflarestorage.com");
    assert_eq!(r, "auto");
    // Unset env so the file is the only source.
    // (Env vars are global — this test relies on absence of these in the env.)
}

#[tokio::test]
async fn r2_upload_produces_public_url() {
    // Only run if AUDIOGEN_TEST_R2=1 and creds are present.
    if std::env::var("AUDIOGEN_TEST_R2").ok().as_deref() != Some("1") {
        eprintln!("skip r2_upload_produces_public_url: set AUDIOGEN_TEST_R2=1 to enable");
        return;
    }
    let (bucket, ak, sk, endpoint, region) =
        audiogen::r2::R2Client::load_env_or_file(None).expect("R2 creds required");
    let client = audiogen::r2::R2Client::new(
        &bucket, &region, &endpoint, &ak, &sk,
        "https://r2.gamesci-lite.com",
    ).expect("client");
    // Write a small test file.
    let tmp = tempdir();
    let test_path = tmp.path().join("audiogen_test_marker.txt");
    let marker = format!("audiogen test {}", std::process::id());
    std::fs::write(&test_path, &marker).unwrap();
    let key = format!("gamesci-classroom/__tests__/audiogen_test_{}.txt",
                      std::process::id());
    let res = client.upload_file(&test_path, &key, "text/plain").await.expect("upload");
    assert!(res.url.contains(&key));
    // Verify via HTTP.
    let body = reqwest::get(&res.url).await.expect("get").text().await.expect("body");
    assert_eq!(body, marker, "uploaded content must round-trip");
}

#[test]
fn racing_jobs_file_loads() {
    let jf = JobsFile::load(&racing_jobs_path()).unwrap();
    assert_eq!(jf.game, "racing");
    assert_eq!(jf.install_dir.as_deref(), Some("racing/racing/audio"));
    // racing 清单含 BGM × 3 + voice × 3 + procedural × 5 = 11。
    assert_eq!(jf.jobs.len(), 11);
    let mut music = 0;
    let mut voice = 0;
    let mut proc = 0;
    for j in &jf.jobs {
        match j.kind {
            JobKind::Music => music += 1,
            JobKind::Voice => voice += 1,
            JobKind::Procedural => proc += 1,
        }
    }
    assert_eq!(music, 3, "bgm_menu/race/victory");
    assert_eq!(voice, 3, "voice_countdown/finish/newrecord");
    assert_eq!(proc, 5, "engine/accel/skid/crash/explode");
}

#[test]
fn racing_synth_generators_produce_mp3() {
    let names = ["engine", "accel", "skid", "crash", "explode"];
    for name in names {
        let pcm = synth::synthesize(name, 1.0, 22050, 1).unwrap();
        let mp3 = codec_for_ext("mp3").unwrap()
            .encode(&pcm, &EncodeCfg::mp3(22050, 1, 32_000)).unwrap();
        assert!(mp3.len() > 100, "{name}: MP3 too small ({}B)", mp3.len());
        assert_eq!(mp3[0], 0xFF, "{name}: invalid MP3 header");
    }
}

#[test]
fn engine_loop_seamless() {
    // engine_loop 频率为整数 → 重复拼接不该有 pop。
    let p = synth::engine_loop(2.0, 22050, 1);
    let tail_amp = p.samples[p.samples.len() - 100..].iter().map(|s| s.abs()).max().unwrap();
    let head_amp = p.samples[..100].iter().map(|s| s.abs()).max().unwrap();
    let diff = (tail_amp as i32 - head_amp as i32).abs();
    assert!(diff < (head_amp.max(tail_amp) as f32 * 0.3) as i32,
            "engine_loop seam: tail={tail_amp} head={head_amp}");
}

// ── Pipeline state tests ──

#[test]
fn pending_state_roundtrip() {
    let tmp = tempdir();
    let dir = tmp.path();
    let game = "test_game";
    // Empty initial state.
    let st = pipeline::load_pending(dir, game).unwrap();
    assert_eq!(st.jobs.len(), 0);
    // Save + reload should preserve.
    let mut st2 = st;
    use audiogen::pipeline::{PendingJob, PendingState};
    use std::collections::HashMap;
    st2.jobs.insert(
        "test_game/audio/x".into(),
        PendingJob {
            cache_key: "test_game/audio/x".into(),
            kind: JobKind::Music,
            format: "mp3".into(),
            submitted_at: 1234567890,
        },
    );
    pipeline::save_pending(dir, &st2).unwrap();
    let st3 = pipeline::load_pending(dir, game).unwrap();
    assert_eq!(st3.jobs.len(), 1);
    assert_eq!(st3.jobs["test_game/audio/x"].submitted_at, 1234567890);
}

// Minimal tempdir helper (avoids extra crate dep).
struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path { &self.0 }
}
impl Drop for TempDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
fn tempdir() -> TempDir {
    let mut p = std::env::temp_dir();
    p.push(format!("audiogen-test-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&p).unwrap();
    TempDir(p)
}
#[test]
fn flac_roundtrip_via_symphonia() {
    // Encode → file → decode (symphonia) → verify
    let pcm = synth::pluck(0.3, 22050, 1);
    let flac_bytes = codec_for_ext("flac").unwrap()
        .encode(&pcm, &EncodeCfg::wav(22050, 1)).unwrap();
    // FLAC magic: "fLaC"
    assert_eq!(&flac_bytes[0..4], b"fLaC", "FLAC must start with fLaC magic");
    assert!(flac_bytes.len() > 100, "FLAC should be reasonably sized");
    // Decode back
    let back = decode_any(&flac_bytes, Some("flac")).unwrap();
    assert_eq!(back.channels, pcm.channels);
    assert_eq!(back.sample_rate, pcm.sample_rate);
    // FLAC is lossless: amplitudes should match (within rounding)
    // First sample must be silent (pluck envelope starts at 0)
    assert_eq!(back.samples[0], 0);
    // Mid should have non-zero
    let mid_amp = back.samples[back.samples.len() / 2].abs();
    assert!(mid_amp > 0, "decoded FLAC should have non-zero mid amplitude");
}

#[test]
fn flac_rejects_bad_sample_rate() {
    let pcm = Pcm {
        samples: vec![0; 22050],
        sample_rate: 19000, // not in FLAC's allowed list
        channels: 1,
    };
    let r = codec_for_ext("flac").unwrap().encode(&pcm, &EncodeCfg::wav(19000, 1));
    assert!(r.is_err());
    assert!(r.unwrap_err().to_string().contains("unsupported sample rate"));
}

#[test]
fn aac_encode_returns_not_implemented() {
    let pcm = Pcm {
        samples: vec![0; 1000],
        sample_rate: 22050,
        channels: 1,
    };
    let r = codec_for_ext("aac").unwrap().encode(&pcm, &EncodeCfg::mp3(22050, 1, 64000));
    assert!(r.is_err(), "AAC encode must error in v1");
    assert!(r.unwrap_err().to_string().contains("not implemented"));
}
