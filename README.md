# ContainerInfra

A project for running containers on microVMs at hyperscale.

## Setting up development environment

You can only run and develop this on a Linux system with virtualization enabled. For MacOS or Windows you need a virtualized environment with **nested virtualization enabled**. Verify this by checking if `/dev/kvm` exists on your system.

Run `./setup_dev_ubuntu.sh`. It should be enough on Ubuntu 24.04.
It will download all required dependencies and then build a nodeagent rootfs.
