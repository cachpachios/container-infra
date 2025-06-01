#!/bin/bash

set -e

# Check if rustup is installed
if ! command -v rustup &> /dev/null
then
    echo "rustup could not be found, installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    source $HOME/.cargo/env
fi

rustup target add x86_64-unknown-linux-musl

sudo apt update
sudo apt install -y \
    build-essential \
    cmake \
    protobuf-compiler \
    libssl-dev \
    musl-dev \
    musl-tools


echo Downloading firecracker...

if [ ! -f nodemanager/target/firecracker/release-v1.11.0-x86_64/firecracker-v1.11.0-x86_64 ]; then

    mkdir -p nodemanager/target/firecracker || true
    wget -O nodemanager/target/firecracker/firecracker-v1.11.0-x86_64.tgz https://github.com/firecracker-microvm/firecracker/releases/download/v1.11.0/firecracker-v1.11.0-x86_64.tgz
    tar -xzf nodemanager/target/firecracker/firecracker-v1.11.0-x86_64.tgz -C nodemanager/target/firecracker
    rm nodemanager/target/firecracker/firecracker-v1.11.0-x86_64.tgz
fi

if [ ! -f nodemanager/target/vmlinux_6.1.102 ]; then
    echo Downloading a linux kernel...
    wget -O nodemanager/target/vmlinux_6.1.102 https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.11/x86_64/vmlinux-6.1.102
fi

echo Consturcting the rootfs...
./instance/makefs.sh

echo You are now ready to start a nodemanager.

