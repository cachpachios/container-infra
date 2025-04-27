# ContainerInfra

A project for running containers in microVMs (using Firecracker).

## Components

- **nodemanager**: The main service that manages the lifecycle of microVMs and configures networking etc.
- **nodeagent**: The init system that runs inside the microVM. It is responsible for fetching the container layers, setting up the environment and executing the container.
- **nodecli**: A debug command line utility for interacting with the nodemanager service over gRPC.

## Setting up development environment

You can only run and develop this on a Linux system with virtualization enabled. For MacOS or Windows you need a virtualized environment with **nested virtualization enabled**. Verify this by checking if `/dev/kvm` exists on your system.

Run `./setup_dev_ubuntu.sh`. It should be enough on Ubuntu 24.04.
It will download all required dependencies and then build a nodeagent rootfs.

## Get up and running

Install, download and build prerequisites with `./setup_dev_ubuntu.sh`.

Start the nodemanager! See its [README](./nodemanager/README.md) for details.

Use the `nodecli` utility to run some containers. See its [README](./nodecli/README.md) for details.
