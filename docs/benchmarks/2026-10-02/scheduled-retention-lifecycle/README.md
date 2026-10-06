# Scheduled backup retention lifecycle

The owned Linux Moto S3 emulator passed 21 checks. A scheduler-generated catalog pruned three expired exact versions while preserving a pinned recovery point. Repeating the same plan verified all three versions already absent. The pinned encrypted backup restored identical plaintext after pruning. An unregistered substitute version and a skipped uncertain upload remained untouched. A later scheduled capture succeeded; the next retention pass deleted one additional expired version and accepted the three previously deleted versions as absent.

Final inventory: six confirmed historical receipts, four remaining object versions, four confirmed deletions. Deleted receipts remain in the durable journal and derived catalog; retention accepts absent expired versions while requiring retained recovery points to exist and match their ciphertext identities. Retention application was serialized using the scheduler state lock.

Run on Linux with backup dependencies and Moto:

```sh
python -O check-scheduled-backups.py --output fresh-report.json
```

This is an owned in-process S3 emulator fixture using real encryption and decryption. It does not verify KVM recovery, a deployed timer, managed storage durability, IAM, distributed coordination, or performance leadership. The earlier scheduled-backups archive remains immutable.
