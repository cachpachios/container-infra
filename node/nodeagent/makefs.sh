#!/bin/bash

set -e

# Change to the directory of the script
cd "$(dirname "$0")"

cargo build --target=x86_64-unknown-linux-musl --release

echo Static init built! Generating rootfs

rm -f target/rootfs.ext4 || true

truncate -s 32M target/rootfs.ext4
sudo mkfs.ext4 target/rootfs.ext4

rm -rf target/nodeagent_tmp_rootfs || true
mkdir target/nodeagent_tmp_rootfs

sudo mount target/rootfs.ext4 target/nodeagent_tmp_rootfs

sudo mkdir -p target/nodeagent_tmp_rootfs/sbin

sudo cp target/x86_64-unknown-linux-musl/release/nodeagent target/nodeagent_tmp_rootfs/sbin/init

sudo umount target/nodeagent_tmp_rootfs

rm -rf target/nodeagent_tmp_rootfs

echo Rootfs created at target/rootfs.ext4
