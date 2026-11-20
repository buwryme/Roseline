#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 <arch> <distro> <command>" >&2
    exit 1
fi

arch=$1
distro=$2
command=$3
repo=$(cd "$(dirname "$0")/.." && pwd)
rootfs=$(mktemp -d)
tarball=

cleanup() {
    status=$?
    trap - EXIT
    for target in work dev sys proc; do
        if mountpoint -q "$rootfs/$target"; then
            sudo umount -R "$rootfs/$target" 2>/dev/null ||
                sudo umount -Rl "$rootfs/$target" 2>/dev/null || true
        fi
    done
    if mountpoint -q "$rootfs"; then
        sudo umount -l "$rootfs" 2>/dev/null || true
    fi
    if ! mount | grep -Fq " $rootfs"; then
        sudo rm -rf --one-file-system "$rootfs"
    else
        echo "left the chroot mounts under $rootfs" >&2
    fi
    rm -f "$tarball" 2>/dev/null || true
    exit "$status"
}
trap cleanup EXIT

case "$arch" in
    aarch64) docker_arch=arm64 ;;
    x86_64) docker_arch=amd64 ;;
    *)
        echo "unsupported architecture: $arch" >&2
        exit 1
        ;;
esac

case "$distro" in
    arch)
        if [ "$arch" != aarch64 ]; then
            echo "the Arch rootfs is aarch64 only" >&2
            exit 1
        fi
        tarball=$(mktemp)
        curl -fL --retry 3 -o "$tarball" \
            https://archlinuxarm.org/os/ArchLinuxARM-aarch64-latest.tar.gz
        sudo tar -xzf "$tarball" -C "$rootfs"
        ;;
    fedora)
        container=$(docker create --platform "linux/$docker_arch" fedora:latest)
        docker export "$container" | sudo tar -x -C "$rootfs"
        docker rm "$container" >/dev/null
        ;;
    ubuntu)
        container=$(docker create --platform "linux/$docker_arch" ubuntu:24.04)
        docker export "$container" | sudo tar -x -C "$rootfs"
        docker rm "$container" >/dev/null
        ;;
    *)
        echo "unsupported distribution: $distro" >&2
        exit 1
        ;;
esac

sudo rm -f "$rootfs/etc/resolv.conf"
sudo cp /etc/resolv.conf "$rootfs/etc/resolv.conf"
sudo mkdir -p "$rootfs/proc" "$rootfs/sys" "$rootfs/dev" "$rootfs/work"
sudo mount --bind "$rootfs" "$rootfs"
sudo mount -t proc proc "$rootfs/proc"
sudo mount --rbind /sys "$rootfs/sys"
sudo mount --make-rslave "$rootfs/sys"
sudo mount --rbind /dev "$rootfs/dev"
sudo mount --make-rslave "$rootfs/dev"
sudo mount --bind "$repo" "$rootfs/work"

sudo --preserve-env=PKG_VERSION,VERSION,ARCH,DEB_ARCH \
    env HOME=/root PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    chroot "$rootfs" /bin/bash -c "cd /work && $command"
