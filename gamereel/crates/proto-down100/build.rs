fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = [
        "proto/messages.proto",
        "proto/fps_events.proto",
        "proto/moba_events.proto",
        "proto/down100.proto",
        "proto/replay.proto",
    ];
    prost_build::compile_protos(&protos, &["proto/"])?;
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }
    Ok(())
}
