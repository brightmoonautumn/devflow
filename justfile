default:
    @just --list

server:
    cargo run -p devflow

web:
    pnpm web:dev

build:
    pnpm web:build
    cargo build --release -p devflow

test:
    cargo test --workspace
    pnpm web:check

fmt:
    cargo fmt --all
