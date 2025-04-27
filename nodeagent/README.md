# NodeAgent

The nodeagent is the program that runs inside the VM (as its init system).
Its responsible for fetching the container layers, setting up the environment and executing the container.
It uses `busybox` for to have some system tools available (mount etc) and `crun` to execute the container.

Might ship a full userspace in the future.
But now we build everything fully statically with musl as libc:

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

## Constructing the minimal rootfs

Run the `./makefs.sh`. It will ask for sudo to be able to mount the image on a temp folder while writing it.
It also fetches `busybox` and `crun` and builds `e2fsprogs`.

It outputs `target/rootfs.ext4`.
