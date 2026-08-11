//! audiogen dry-run: load jobs.json, build MiniMax envelopes, print them.
//! Validates the submit pipeline end-to-end except the actual SSH LPUSH.

use audiogen::jobs::JobsFile;
use audiogen::pipeline;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: cargo run --bin dry_run -- <jobs.json>");
        std::process::exit(1);
    }
    let jf = JobsFile::load(&PathBuf::from(&args[1]))?;
    println!("== jobs file: {} ==", jf.game);
    println!("output_dir: {}", jf.output_dir);
    println!("install_dir: {:?}", jf.install_dir);
    println!("style: {:?}", jf.style);
    println!("voice_id: {:?}", jf.voice_id);
    println!();
    let mut n_proc = 0;
    let mut n_music = 0;
    let mut n_voice = 0;
    for job in &jf.jobs {
        match job.kind {
            audiogen::jobs::JobKind::Procedural => {
                println!("[procedural] {} (synth={:?} dur={:?}s)",
                         job.name, job.synth, job.duration_secs);
                n_proc += 1;
            }
            _ => {
                let kwargs = pipeline::build_kwargs(&jf, job)?;
                let envelope = serde_json::json!({
                    "task": "audio.gen",
                    "kwargs": kwargs,
                });
                println!("[{}] {} → cache_key={}/audio/{}",
                         match job.kind {
                             audiogen::jobs::JobKind::Music => { n_music += 1; "music" }
                             audiogen::jobs::JobKind::Voice => { n_voice += 1; "voice" }
                             audiogen::jobs::JobKind::Procedural => unreachable!(),
                         },
                         job.name,
                         jf.game,
                         job.name);
                println!("    envelope: {}", serde_json::to_string(&envelope)?);
            }
        }
    }
    println!();
    println!("== total: {} music, {} voice, {} procedural ==", n_music, n_voice, n_proc);
    Ok(())
}