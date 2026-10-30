#!/bin/bash
set -eu
cd /var/tmp/openbsd
cp --sparse=always miniroot80.img mini-gpt.img
truncate -s 8M mini-gpt.img
command -v sgdisk || apt-get install -y -qq gdisk >/dev/null
sgdisk -g mini-gpt.img
sgdisk -p mini-gpt.img
# the ESP type code, so the firmware finds it
sgdisk -t 1:ef00 mini-gpt.img
sgdisk -p mini-gpt.img | tail -4
