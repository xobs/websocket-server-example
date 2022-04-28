# Work-in-progress Websocket Server Example

This is a work-in-progress example of a websocket server. It is designed to be used as both a library and a server.

## Building

To build, compile with `cargo build --target riscv32imac-unknown-xous-elf`. To build in release mode, compile with `cargo build --target riscv32imac-unknown-xous-elf --release`

## Usage

To use on hardware, add `target/riscv32imac-unknown-xous-elf/release/websocket-demo` to the release image.