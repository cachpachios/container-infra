# NodeManager

Use the `setup_dev_ubuntu.sh` script. It will download firecracker, a linux kernel image, and construct the instance rootfs.

Then build and run like this:
```bash
cargo build && sudo RUST_LOG=debug ./target/debug/nodemanager
```

You need sudo access to spin up VMs.

To run something look at the `nodecli` utility [README.md](../nodecli/README.md).


## Host configuration

Notice the `config.json` file. The `setup_dev_ubuntu.sh` script automatically downloads a kernel and firecracker binaries to the `target` folder as expected.

Also make sure ipv4 forwarding is enabled on the host. You can do this by running:
```bash
sudo sysctl -w net.ipv4.ip_forward=1
```

HOWEVER! For enabling NAT for the VMs check if the interface on your host is the same as in the `config.json` file.
You can use `ip a` to check the interface name.
