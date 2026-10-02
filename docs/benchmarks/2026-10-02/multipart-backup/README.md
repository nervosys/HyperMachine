# Multipart encrypted snapshot-store backup

The backup helper now uses conditional multipart S3 completion for larger
encrypted bundles, retaining a whole-ciphertext SHA-256 recovery receipt and
separate composite part checksum. Its format cap is 64 GiB of ciphertext;
64 MiB upload parts and threshold are the defaults. No daemon code changed.

An owned loopback Moto 5.2.3 S3 HTTP emulator verifies upload and recovery.
Nine check groups cover a three-part round trip, refusal to overwrite, abort
isolation from another invocation's upload, part checksum mismatch, failed
abort, lost creation response, interruption, lost completion response,
completion checksum mismatch, invalid settings and the encryption format bound.
Lost completion responses retain a receipt that recovers the committed object.
Failed aborts and unknown upload IDs preserve their uncertainty; fixture-only
operator cleanup removes the injected orphan sessions afterward.

The large cohort additionally creates 5,001,000,000 bytes of incompressible
fixture data, uploads more than 5,000,000,000 ciphertext bytes in 75 parts,
checks the downloaded ciphertext against its receipt, and recovers identical
plaintext. This crosses the previous single-PutObject limit with real HTTP
operations. It is a capacity/recovery check, not a throughput benchmark.

The KVM cohort passes all 15 existing backup checks while forcing five-part
upload of the paused guest store. It verifies original path unavailability,
relocated layered memory, live process/memory markers, guest filesystem and
boot ID, mounted 9P volume data and identity, named snapshot recovery, and an
ordinary application `.snap` filename. All owned processes stop cleanly and
guest/volume inventories are empty. The accepted daemon hash is
`2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f`;
its clean-source build evidence is in the preceding [backup archive](../object-backup/README.md).
The three provisional workspace boot changes were not compiled or executed.

Both initial failed cohorts remain archived with their exact coordinators.
Moto's HeadObject exposes the composite digest without AWS's `-N` part-count
suffix. The corrected fixture checks that exposed digest and records the
limitation; production completion validation still requires AWS's full form.
Emulation establishes no AWS durability, IAM, retention, replication or
managed-service performance guarantee. Reports identify all dependency versions
and verify that their tools remain unchanged during execution.

Only reports, logs and frozen Python tools are archived. No snapshot memory,
plaintext store, ciphertext bundle, private encryption key, credentials or
executable is included. `manifest.json` hashes the archived files. Verify with:

```sh
python -O tools/verify-multipart-backup.py docs/benchmarks/2026-10-02/multipart-backup
```

Reproduce with the pinned backup dependencies plus `moto[s3,server]==5.2.3`
in a Linux environment, using a new output directory:

```sh
python -O tools/check-multipart-backup.py --large --output /private/new-multipart-run
python -O tools/check-object-backup.py --multipart --daemon /owned/hv2-sandboxd \
  --kernel /owned/bzImage --initrd /owned/guest-ssh.cpio.gz --output /private/new-kvm-run
```

The large fixture requires space for several copies of the 5 GB object and
enough RAM for Moto's buffered completion. The production uploader buffers one
configured part at a time. Shared WSL host load limits any timing inference.
