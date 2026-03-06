# video-generator

> Rust video generation tools built on FFmpeg.

## Sub-projects

| Directory | Description |
|-----------|-------------|
| [movie-maker](movie-maker/) | Core library — programmatic video generation with FFmpeg, image compositing, and tween animations |
| [demo](demo/) | Demo application (hs-mvp) — example usage of movie-maker for scene rendering |

## movie-maker

A library for generating videos from code. Supports:

- FFmpeg-based video encoding
- Image compositing with `image` + `imageproc`
- Tween-based animation system
- Performance benchmark binary (`perf_main`)

## demo (hs-mvp)

A demo application that uses `movie-maker` to render scenes into video output.

## Build

```bash
# Build the library
cd movie-maker && cargo build --release

# Build the demo
cd demo && cargo build --release
```

**Requires:** FFmpeg development libraries installed on the system.

## License

See LICENSE file.
