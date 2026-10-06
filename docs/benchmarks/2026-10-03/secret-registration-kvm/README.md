# Scope attachment at live sandbox registration

Asynchronous bring-up originally attached a secret scope before publishing its
gateway in the live registry. A reload could finish between that lookup and
registration, miss the unpublished gateway and leave its old absent/revoked
store attached. Registration now refreshes the exact-ID scope while holding the
same live-registry lock used by reload attachment. The failed-pause reinsertion
path also refreshes under that lock. Reload releases its scope-map write lock
before acquiring the registry lock; scope lookup never awaits under this lock.

All 41 daemon tests pass, and the separate verification binary passes twelve
real KVM checks across seventeen owned HTTPS requests. The previous body,
rotation, hostname, fork, pause/resume and revocation checks remain. Four new
cycles submit resume concurrently with revoke/re-add reloads, wait for both,
then observe the current secret upstream. At least one resume call was still
pending when reload was issued. Exact registration interleavings are not forced,
and failed-pause recovery is not induced, so this is not exhaustive race proof.

The archived main.rs bytes match the isolated checkout and workspace. The full
isolated source hash catalog identifies the build; protected workspace core
sources were neither read nor built. Other networking implementation sources
are unchanged from the prior secret-substitution-kvm archive. The input daemon
is a separate verification build; accepted benchmark inputs remain unchanged.
The checker removes guests, reaps daemon/listener and deletes private temporary
keys and policies. No secret policy, token, key or raw API response is archived.
No managed competitor parity or performance claim is established.

Reproduce in the isolated accepted-core checkout with the archived source:
`cargo test -p hv2-sandboxd`, `cargo build -p hv2-sandboxd`, then
`python3 tools/check-secret-substitution-kvm.py --daemon BIN --kernel KERNEL
--initrd HTTPS_CLIENT_IMAGE --output NEW.json`. Linux KVM, OpenSSL, ip and the
separate HTTPS client image are required. The report records exact input hashes.
