#!/bin/bash

set -e

# Change to the directory of the script
cd "$(dirname "$0")"

cargo build --target=x86_64-unknown-linux-musl --release

# If busybox is not at target/busybox, download it
if [ ! -f target/busybox ]; then
    echo Downloading busybox
    wget -O target/busybox https://busybox.net/downloads/binaries/1.35.0-x86_64-linux-musl/busybox
fi

# crun from https://github.com/containers/crun/releases/download/1.21/crun-1.21-linux-amd64
# Download crun if not present

if [ ! -f target/crun ]; then
    echo Downloading crun
    wget -O target/crun https://github.com/containers/crun/releases/download/1.21/crun-1.21-linux-amd64
fi

# If mke2fs is not at /sbin/mke2fs, build it
if [ ! -f target/mke2fs ]; then
    echo Building mke2fs
    wget -O target/e2fsprogs-1.47.1.tar.gz https://mirrors.edge.kernel.org/pub/linux/kernel/people/tytso/e2fsprogs/v1.47.1/e2fsprogs-1.47.1.tar.gz
    tar -xzf target/e2fsprogs-1.47.1.tar.gz -C target
    cd target/e2fsprogs-1.47.1
    ./configure CFLAGS='-g -static -O2 -no-pie' LDFLAGS="-static"
    make -j$(nproc)
    cd ../..
    cp target/e2fsprogs-1.47.1/misc/mke2fs target/mke2fs
    chmod +x target/mke2fs
    rm target/e2fsprogs-1.47.1.tar.gz
    rm -rf target/e2fsprogs-1.47.1
fi

echo Dependencies finished. Generating the rootfs

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

sudo cp target/x86_64-unknown-linux-musl/release/nodeagent target/nodeagent_tmp_rootfs/sbin/init
sudo chmod +x target/nodeagent_tmp_rootfs/sbin/init

sudo cp target/busybox target/nodeagent_tmp_rootfs/sbin/busybox
sudo chmod +x target/nodeagent_tmp_rootfs/sbin/busybox

sudo ln -s /sbin/busybox target/nodeagent_tmp_rootfs/bin/busybox

sudo chroot target/nodeagent_tmp_rootfs /sbin/busybox --install -s /bin

sudo cp target/mke2fs target/nodeagent_tmp_rootfs/sbin/mke2fs
sudo chmod +x target/nodeagent_tmp_rootfs/sbin/mke2fs

sudo cp target/crun target/nodeagent_tmp_rootfs/bin/crun
sudo chmod +x target/nodeagent_tmp_rootfs/bin/crun

touch target/resolv.conf
echo "nameserver 8.8.8.8" > target/resolv.conf
sudo mv target/resolv.conf target/nodeagent_tmp_rootfs/etc/resolv.conf

touch target/nsswitch.conf
echo "passwd: files" > target/nsswitch.conf
echo "group: files" >> target/nsswitch.conf
sudo mv target/nsswitch.conf target/nodeagent_tmp_rootfs/etc/nsswitch.conf

touch target/passwd
echo "root:x:0:0:root:/root:/bin/sh" > target/passwd
sudo mv target/passwd target/nodeagent_tmp_rootfs/etc/passwd

touch target/group
echo "root:x:0:" > target/group
sudo mv target/group target/nodeagent_tmp_rootfs/etc/group

echo Unmounting the rootfs
sudo umount target/nodeagent_tmp_rootfs
rm -rf target/nodeagent_tmp_rootfs

echo Rootfs created at target/rootfs.ext4
