# Webhook signatures

A webhook delivery carries two signatures. Verify whichever your receiver
already speaks.

- **`hv2-signature`** (use this one). `v1=<hex>`, where hex is
  HMAC-SHA256, keyed by the webhook's secret, over `<timestamp>.<body>`, with
  the timestamp from the **`hv2-signature-timestamp`** header (Unix seconds).
  Reject a delivery whose timestamp is too old for you (five minutes is
  usual): the timestamp is signed, so a replay cannot change it.
- **`e2b-signature`**, for E2B's own SDKs and verifiers: base64, unpadded, of
  SHA-256 over the secret followed by the body. It is not an HMAC and signs
  no timestamp. It is kept as it is so E2B's verifiers work; it always uses
  the current secret.

## Rotating the secret

`PATCH /events/webhooks/{id}` with a new `signatureSecret` (16 characters at
least). The old secret **keeps signing for `overlapSeconds`** (a day if
absent, 0 for none, a week at most): while it does, `hv2-signature` holds two
space-separated `v1=` values, one for each secret, and a receiver that has
either finds its own. Change the receiver's secret in that window and no
delivery is lost. A second rotation inside the window replaces the older
secret.

Nothing returns a secret, old or new.

Checked by unit tests against RFC 4231's HMAC-SHA256 vector and by a delivery
to a loopback receiver after a rotation; **not** against another vendor's
verifier. Machines send no webhooks.
