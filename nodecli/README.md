# NODECLI - Internal Development CLI for talking directly to nodemanager services

This CLI connects directly to the nodemanager service and allows you to run commands

## Build and run the CLI
Build a debug build:

```bash
cargo build
```

See the CLI help. 

```bash
./target/debug/nodecli --help
```

## Example usage

### Provision a VM

Lets run the latest nginx container, which will bind to port 80 in the guest.
```bash
./target/debug/nodecli provision nginx
```

Running this command will spin up a VM and run the nginx container in it.
Nodecli automatically streams the logs. You can Ctrl-C to stop tailing them. (It wont kill the VM)

Look at the nodemanager logs to find the local-link IP of the VM. (only shown with `RUST_LOG=debug`)
It will probably be `176.16.0.2` if its the first VM.

If everything is working, you should be able to see the nginx welcome page at [http://172.16.0.2/](http://172.16.0.2/).

When done, call `./target/debug/nodecli drain` to shutdown all VMs before stopping the nodemanager.

### Shutdown VM(s)

Use the `./target/debug/nodecli deprovision <uuid>` command to shutdown a specific VM.
Or just call `./target/debug/nodecli drain` to shutdown all running VMs.

Note: Using a guest kernel compiled without support for the serial input device used by Firecracker will not shutdown cleanly but will forcefully killed after 3 seconds. This will be fixed in the future by not using the serial input device.

