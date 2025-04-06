#!/bin/bash

set -e

# Change to the directory of the script
cd "$(dirname "$0")"

cargo build --target=x86_64-unknown-linux-musl --release

echo Static init built! Generating rootfs

rm -f target/rootfs.ext4 || true

echo Creating the rootfs
truncate -s 128M target/rootfs.ext4
sudo mkfs.ext4 target/rootfs.ext4

rm -rf target/nodeagent_tmp_rootfs || true
mkdir target/nodeagent_tmp_rootfs

echo Mounting the rootfs
sudo mount target/rootfs.ext4 target/nodeagent_tmp_rootfs

sudo mkdir -p target/nodeagent_tmp_rootfs/{sbin,dev,proc,run,sys,bin,etc,mnt}
sudo mkdir -p target/nodeagent_tmp_rootfs/dev/pts
sudo mkdir -p target/nodeagent_tmp_rootfs/var/run
echo Coping the static init to the rootfs
# DEBUG!!!
sudo cp target/x86_64-unknown-linux-musl/release/nodeagent target/nodeagent_tmp_rootfs/sbin/init
sudo chmod +x target/nodeagent_tmp_rootfs/sbin/init

# If busybox is not at target/busybox, download it
if [ ! -f target/busybox ]; then
    echo Downloading busybox
    wget -O target/busybox https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox
fi
sudo cp target/busybox target/nodeagent_tmp_rootfs/sbin/busybox
sudo chmod +x target/nodeagent_tmp_rootfs/sbin/busybox

sudo ln -s /sbin/busybox target/nodeagent_tmp_rootfs/bin/busybox

sudo chroot target/nodeagent_tmp_rootfs /sbin/busybox --install -s /bin

# crun from https://github.com/containers/crun/releases/download/1.21/crun-1.21-linux-amd64
# Download crun if not present

if [ ! -f target/crun ]; then
    echo Downloading crun
    wget -O target/crun https://github.com/containers/crun/releases/download/1.21/crun-1.21-linux-amd64
fi
sudo cp target/crun target/nodeagent_tmp_rootfs/bin/crun
sudo chmod +x target/nodeagent_tmp_rootfs/bin/crun

touch target/resolv.conf
echo "nameserver 8.8.8.8" > target/resolv.conf
sudo mv target/resolv.conf target/nodeagent_tmp_rootfs/etc/resolv.conf

echo Unmounting the rootfs
sudo umount target/nodeagent_tmp_rootfs
rm -rf target/nodeagent_tmp_rootfs

echo Rootfs created at target/rootfs.ext4
