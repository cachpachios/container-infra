# ContainerInfra

A project for running containers in microVMs at (eventually, maybe) hyperscale.

## Setting up development environment

You can only run and develop this on a Linux system with virtualization enabled. For MacOS or Windows you need a virtualized environment with **nested virtualization enabled**. Verify this by checking if `/dev/kvm` exists on your system.

Run `./setup_dev_ubuntu.sh`. It should be enough on Ubuntu 24.04.
It will download all required dependencies and then build a nodeagent rootfs.

## Get up and running

Install, download and build prerequisites with `./setup_dev_ubuntu.sh`.

Start the nodemanager! See its [README](./nodemanager/README.md) for details.

Use the `nodecli` utility to run some VMs. See its [README](./nodecli/README.md) for details.
