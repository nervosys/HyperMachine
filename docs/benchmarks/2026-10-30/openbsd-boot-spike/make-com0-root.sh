#!/bin/bash
set -eu
cd /var/tmp/openbsd
rm -rf root ffs.img mini-com0.img
mkdir -p root/etc
cp bsd.rd root/bsd
printf 'stty com0 115200\nset tty com0\n' > root/etc/boot.conf
# 'a' is 10368 sectors = 5308416 bytes, starting 8192 bytes in the FFS image is its own boot area
makefs -t ffs -o version=1,bsize=8192,fsize=1024,minfree=0,density=65536 -s 5308416 ffs.img root
ls -l ffs.img
cp mini-gpt.img mini-com0.img
# keep the original first 8 KiB (boot sector + disklabel); lay the new FS from 8192 on
dd if=ffs.img of=mini-com0.img bs=512 seek=$((1024+16)) skip=16 count=$((10368-16)) conv=notrunc status=none
cmp <(dd if=mini-gpt.img bs=512 skip=1024 count=16 status=none) <(dd if=mini-com0.img bs=512 skip=1024 count=16 status=none) && echo "boot area + disklabel unchanged"
python3 - <<'EOF'
import struct
d=open("/var/tmp/openbsd/mini-com0.img","rb").read()
sb=d[1024*512+8192:1024*512+8192+2048]
print("new FS magic", hex(struct.unpack("<I",sb[1372:1376])[0]))
EOF
