# tpt-visual — common tasks (`cargo install just`, or run the commands by hand)

default: gate

# Build everything
build:
    cargo build --workspace

# Run all tests (GPU tests skip automatically without an adapter)
test:
    cargo test --workspace

# Check formatting
fmt:
    cargo fmt --all --check

# Clippy with warnings as errors
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# License audit
deny:
    cargo deny check licenses

# Everything CI runs
gate: fmt clippy build test deny

# Render the multi-layer compositing demo to demo.avi
demo frames="90":
    cargo run --release -p tpt-av-visual --example compositor_demo -- --frames {{frames}} --out demo.avi

# Export a timeline JSON to an AVI: just render session.json out.avi 240
render json out="out.avi" frames="240":
    cargo run --release -p tpt-av-visual --example headless_render -- {{json}} --out {{out}} --frames {{frames}}

# Play a video file in a window (omit the path for the procedural pattern)
player *path:
    cargo run --release -p tpt-av-visual --example simple_player -- {{path}}

# Build the documentation and open it
docs:
    cargo doc --workspace --no-deps --open
