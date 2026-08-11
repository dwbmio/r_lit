//! Procedural SFX synthesis (zero network, deterministic, tiny footprint).
//!
//! Each generator returns a `Pcm` at its declared sample rate / channels.
//! Encoding to MP3 is the caller's responsibility (see [`crate::codec::Codec::encode`]).

use crate::codec::Pcm;
use anyhow::{anyhow, Result};
use std::f32::consts::PI;

/// Dispatch a procedural SFX by name. Add new generators here.
pub fn synthesize(name: &str, duration_secs: f32, sample_rate: u32, channels: u16) -> Result<Pcm> {
    match name {
        // ── UI ──
        "woodblock" | "click"      => Ok(woodblock(duration_secs, sample_rate, channels)),
        "ding"      | "chime"      => Ok(ding(duration_secs, sample_rate, channels)),
        // ── game events ──
        "pluck"     | "string"     => Ok(pluck(duration_secs, sample_rate, channels)),
        "thud"      | "lowhit"     => Ok(thud(duration_secs, sample_rate, channels)),
        "fanfare"   | "victory"    => Ok(fanfare(duration_secs, sample_rate, channels)),
        // ── racing (FF / arcade) ──
        "engine"    | "loop"       => Ok(engine_loop(duration_secs, sample_rate, channels)),
        "accel"                    => Ok(accel(duration_secs, sample_rate, channels)),
        "skid"                     => Ok(skid(duration_secs, sample_rate, channels)),
        "crash"                    => Ok(crash(duration_secs, sample_rate, channels)),
        "explode"                  => Ok(explode(duration_secs, sample_rate, channels)),
        // ── generic ──
        "noise"                    => Ok(noise(duration_secs, sample_rate, channels)),
        other => Err(anyhow!("unknown procedural synth: {other}")),
    }
}

/// 木鱼/铃铛 — 短促高频一击 + 指数衰减。UI 点击用。
pub fn woodblock(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let freq = 1800.0;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = (-t * 60.0).exp(); // sharp decay
        let s = (2.0 * PI * freq * t).sin() * env * 0.8;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 风铃 — 高频泛音 + 长余韵。答对/成功用。
pub fn ding(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let fundamentals = [1568.0, 2093.0, 2637.0]; // G6 / C7 / E7
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = (-t * 3.5).exp();
        let mut s = 0.0;
        for (k, f) in fundamentals.iter().enumerate() {
            s += (2.0 * PI * f * t).sin() / (k + 1) as f32;
        }
        s = s * env * 0.45;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 古筝拨弦 — 基频 + 二次谐波 + 短衰减 + chug 包络。成语落子用。
pub fn pluck(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let base = 440.0;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = (-t * 8.0).exp();
        let chug = 0.6 + 0.4 * (2.0 * PI * 6.0 * t).sin();
        let s = ((2.0 * PI * base * t).sin() + 0.4 * (2.0 * PI * 2.0 * base * t).sin()) * env * chug * 0.7;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 低频鼓声/沉闷撞击 — 50Hz 主体 + 衰减。答错/失败用。
pub fn thud(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = (-t * 4.0).exp();
        // 主频下扫 80→40Hz 模拟鼓点
        let freq = 80.0 - 40.0 * (1.0 - env);
        let s = (2.0 * PI * freq * t).sin() * env * 0.85;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 短胜利号角 — 上升大三和弦 (C-E-G) 串联。古风胜利 jingle 用。
pub fn fanfare(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    // 三个音阶 + 衰减: 523(C5) → 659(E5) → 784(G5)
    let notes = [(0.0, 523.25), (0.25, 659.25), (0.5, 783.99)];
    let dur = duration_secs;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let mut s = 0.0;
        for (start, freq) in notes.iter() {
            let local_t = t - start;
            if local_t >= 0.0 && local_t < dur * 0.5 {
                let env = (-local_t * 2.5).exp();
                s += (2.0 * PI * freq * local_t).sin() * env * 0.5;
            }
        }
        // 尾部泛音余韵
        if t > dur * 0.75 {
            let local = t - dur * 0.75;
            let env = (-local * 5.0).exp();
            s += (2.0 * PI * 1568.0 * local).sin() * env * 0.3;
        }
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 粉噪（确定性 seed）。测试用。
pub fn noise(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let mut rng: u32 = 0xDEAD_BEEF;
    for _ in 0..n {
        // xorshift32
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let r = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let si = (r * i16::MAX as f32 * 0.3) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

// ═══════════════ Racing 专用（FF / arcade）═══════════════
// 对齐 racing_audiogen.py 中 SFX_SPECS；样本整数频率 → 无缝循环。

/// 引擎驱动循环：4 谐波 (70/140/210/280Hz) + chug 包络 (24Hz) + 软削波。
pub fn engine_loop(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let chug = 0.6 + 0.4 * (2.0 * PI * 24.0 * t).sin();
        let s = (0.5 * (2.0 * PI * 70.0 * t).sin()
              + 0.32 * (2.0 * PI * 140.0 * t).sin()
              + 0.20 * (2.0 * PI * 210.0 * t).sin()
              + 0.12 * (2.0 * PI * 280.0 * t).sin()) * chug;
        // softclip via tanh approximation: clamp + smooth shoulders
        let s = s.tanh() * 0.9;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 加速 rev (1.2s): 80→~260Hz 线性 chirp + 二次谐波 + chug。
pub fn accel(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let chug = 0.55 + 0.45 * (2.0 * PI * 26.0 * t).sin();
        let base = 80.0 * t + 75.0 * t * t;
        let s = (0.5 * (2.0 * PI * base * t).sin()
              + 0.3 * (2.0 * PI * 2.0 * base * t).sin()) * chug;
        // fade in 40ms + fade out last 200ms
        let fade_in = (t * 25.0).min(1.0);
        let fade_out = ((duration_secs - t) * 5.0).min(1.0).max(0.0);
        let s = s.tanh() * 0.9 * fade_in * fade_out;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 轮胎打滑 screech (0.9s): 白噪 highpass 1600 + 2.2kHz 颤音谐振。
pub fn skid(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let mut rng: u32 = 0xCAFE_BABE;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
        let noise = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        // crude highpass: subtract slow-moving average
        let hp_noise = noise - (0.95 * (t * 1600.0).sin().clamp(-1.0, 1.0));
        let tone = 0.4 * (2.0 * PI * (2200.0 + 350.0 * (2.0 * PI * 9.0 * t).sin()) * t).sin();
        let s = 0.5 * hp_noise + 0.7 * tone;
        let fade_in = (t * 33.0).min(1.0);
        let fade_out = ((duration_secs - t) / 0.3).min(1.0).max(0.0);
        let s = s.tanh() * 0.9 * fade_in * fade_out;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 撞击 crash (0.6s): 粉噪冲击 + 200→0Hz 低频下扫，指数衰减。
pub fn crash(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let mut rng: u32 = 0x1234_5678;
    let mut lp_state = 0.0f32;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
        let white = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        // simple pink-ish via leaky integrator
        lp_state = lp_state * 0.96 + white * 0.04;
        let pink = lp_state * 4.0;
        let sweep = 0.7 * (2.0 * PI * (200.0 - 230.0 * t) * t).sin();
        let env = (-t * 4.0).exp();
        let s = (pink + sweep) * env;
        let s = s.tanh() * 0.95;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

/// 爆炸 explode (1.0s): 棕噪长衰减 + 120→30Hz 低频隆隆。
pub fn explode(duration_secs: f32, sr: u32, ch: u16) -> Pcm {
    let n = (duration_secs * sr as f32) as usize;
    let mut samples = Vec::with_capacity(n * ch as usize);
    let mut rng: u32 = 0xBADD_F00D;
    let mut lp_state = 0.0f32;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
        let white = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        // brown noise: stronger integration
        lp_state = lp_state * 0.995 + white * 0.005;
        let brown = lp_state * 3.0;
        let rumble = 0.8 * (2.0 * PI * (120.0 - 90.0 * t) * t).sin();
        let env = (-t * 2.5).exp();
        let s = (brown + rumble) * env;
        let s = s.tanh() * 0.95;
        let si = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for _ in 0..ch { samples.push(si); }
    }
    Pcm { samples, sample_rate: sr, channels: ch }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn woodblock_zero() {
        let p = woodblock(0.05, 22050, 1);
        assert_eq!(p.samples.len(), 1102);
        assert_eq!(p.sample_rate, 22050);
    }

    #[test]
    fn ding_pcm_basic() {
        let p = ding(1.0, 22050, 1);
        assert!(!p.samples.is_empty());
        assert_eq!(p.channels, 1);
        // 整体振幅应有明显衰减: 前 1/4 段峰值 >= 后 1/4 段峰值
        let quarter = p.samples.len() / 4;
        let front_max = p.samples[..quarter].iter().map(|s| s.abs()).max().unwrap();
        let back_max = p.samples[3*quarter..].iter().map(|s| s.abs()).max().unwrap();
        assert!(front_max > back_max, "ding should decay: front={front_max} back={back_max}");
    }
}