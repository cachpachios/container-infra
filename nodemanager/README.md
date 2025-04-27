# NodeManager

Use the `setup_dev_ubuntu.sh` script. It will download firecracker, a linux kernel image, and construct the nodeagent rootfs.

Then build and run like this:
```bash
cargo build && sudo RUST_LOG=debug ./target/debug/nodemanager
```

You need sudo access to spin up VMs.

To run something look at the `nodecli` utility [README.md](../nodecli/README.md).
