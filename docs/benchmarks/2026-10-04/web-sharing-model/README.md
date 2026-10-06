# Browser sharing model prerequisite

The owner-bound grant model validates opaque subjects, positive Unix-second expiries, a maximum of 256 unique subjects, canonical UUIDv4 revisions and strict deserialization. Grant ordering is canonical. Empty grants retain a revision tombstone; storage must enforce compare-and-swap before this prevents replay.

Authorization requires matching sandbox ID, trusted owner and exact creation timestamp. Node placement and pause state do not change identity in this model; this is not a migration verification. Expiry is exclusive and a negative clock fails closed. Credentials are authenticated separately.

The final isolated run passes five tests, including owner/incarnation changes, expiry boundary, unknown subjects, ownerless construction, revocation, invalid payloads and an integer timestamp above JavaScript's exact range. The initial four-test run is retained. Both runs checked accepted isolated protected hashes before and after; root protected sources were not read or built.

This is a typed model only. Memory/Redis atomic persistence, revision replay enforcement, owner APIs/CLI, proxy use, restart and real guest/TLS gates remain open. No feature-completion or performance claim is made.
