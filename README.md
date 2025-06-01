# ContainerInfra

A project for provisioning containers as microVMs (using Firecracker).

## Components

Each folder contains a separate component of the project. The components are:

- **nodemanager**: The main service that manages the lifecycle of microVMs and configures devices, disks, and the virtualized networking etc. Runs as a gRPC service and invokes a jailed firecracker process.
- **instance**: The handcrafted linux "distrubution" with a minimalistic init system that runs inside each microVM. Responsible for fetching the container layers, setting up the environment and executing the container.
- **nodecli**: A debug command line utility for interacting with the nodemanager service over gRPC.
- **proto**: The protobuf definitions for the nodemanager gRPC service.

## Setting up development environment

You can only run and develop this on a Linux system with KVM virtualization enabled. For MacOS or Windows you need a virtualized environment with **nested virtualization enabled**. Verify this by checking if `/dev/kvm` exists in your VM.

NOTE: Currently only x86_64 is supported. However ARM64 shouldnt be any large issue, but `makefs.sh` goes for x86 to build/download the `instance`, `busybox`, `mkefs2` and `crun`. 
Will probably add arm64 support myself at some point for running using nested virtualization on apple silicon.

Run `./setup_dev_ubuntu.sh`. It should be enough on Ubuntu 24.04.
It will download all required dependencies and then build a instance rootfs.

## Get up and running

Install, download and build prerequisites with `./setup_dev_ubuntu.sh`.

Start the nodemanager! See its [README](./nodemanager/README.md) for details.

Use the `nodecli` utility to run some containers. See its [README](./nodecli/README.md) for details.
