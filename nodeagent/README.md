# NodeAgent

The nodeagent is the program that runs inside the VM (as its init system).
Its responsible for fetching the container layers, setting up the environment and executing the container.
Except the statically linked nodeagent under `/sbin/init` it also comes with `busybox`, `mkfs.ext4` and `crun`.


## Build just the nodeagent
```bash
rustup target add x86_64-unknown-linux-musl
cargo build --target=x86_64-unknown-linux-musl
```

Note: Use the `makefs.sh` to build a full minimalistic rootfs for use in the VMs.

## Dev dependencies

On Ubuntu:
```bash
sudo apt install libssl-dev musl-dev musl-tools cmake build-essential protobuf-compiler
```

Tip: Just use the `../setup_dev_ubuntu.sh` script.

## Constructing the full rootfs

This will create a full rootfs with the `nodeagent`, `busybox`, `crun` and `mkfs.ext4` and some small expected configuration files.

```bash
./makefs.sh
```
It will ask for sudo to be able to mount the image on a temp folder while writing it.
It also fetches `busybox` and `crun` and builds `e2fsprogs`.

It outputs `target/rootfs.ext4`.

To build in debug mode (which enables debug logging) use:
```bash
./makefs.sh --debug
```
