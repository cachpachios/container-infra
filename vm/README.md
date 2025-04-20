# NodeAgent

Might need to be shipped with a full userspace in the future.
But now we build fully statically with musl as libc:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --target=x86_64-unknown-linux-musl
```

## Dev dependencies

On Ubuntu:
```bash
sudo apt install libssl-dev musl-dev musl-tools
```
