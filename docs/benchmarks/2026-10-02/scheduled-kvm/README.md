# Scheduled capture and real KVM recovery

The durable scheduler captured an offline store containing a real paused KVM guest, its layered memory base, a named snapshot and a mounted 9P volume. The owned Moto S3 HTTP emulator supplied versioned storage. Capture used five multipart parts for 36,658,262 encrypted bytes representing 1,088,573,081 expanded bytes. A second scheduler invocation created no duplicate; an exact-version pin persisted and the integrated locked retention plan protected the registered recovery point.

The original store path was made unavailable. After a delete marker hid the current object, restore fetched its exact version and independently verified the ciphertext receipt. A daemon using the relocated recovered store resumed the same guest with its original ID and access token. Live process/environment memory, boot identity, tmpfs files and guest-written mounted-volume bytes survived. A recovered named snapshot created another functioning guest. The ordinary application file user.snap retained its bytes.

All 16 check groups passed. Guest and volume inventories were empty after cleanup, all three owned processes exited zero, and all recorded artifacts remained unchanged. Keys, guest snapshot bytes, operator credentials and binaries are excluded from this archive. The daemon is the previously accepted clean build; build-context.json records its provenance and excluded provisional boot sources. No daemon rebuild or runtime change was needed.

```sh
python -O tools/check-object-backup.py --output /new/owned/run \
  --daemon /path/to/accepted/hv2-sandboxd --kernel /path/to/kernel \
  --initrd /path/to/compatible/guest --scheduled --multipart
```

Use the daemon and guest input SHA-256 values in report.json. This verifies scheduled one-shot invocation and recovery with real nested KVM, not a deployed timer, coordinated automatic guest maintenance, managed durability, IAM, distributed coordination, or performance leadership. Earlier manual and emulator evidence archives remain immutable.
