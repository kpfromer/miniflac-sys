default: test

build:
    cargo build

build-std:
    cargo build --features std

test:
    cargo test --features std

play file:
    cargo run --example play --features std -- {{file}}

play-volume file volume:
    cargo run --example play --features std -- {{file}} --volume {{volume}}

check:
    cargo check --features std

clippy:
    cargo clippy --features std

clean:
    cargo clean
