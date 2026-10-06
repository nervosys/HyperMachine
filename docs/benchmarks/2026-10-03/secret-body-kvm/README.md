# Real KVM raw/form bodies and rejected reload retention

The extended checker passed eleven owned checks with thirteen observed upstream
HTTPS requests. Verification inputs exactly match the preceding
[secret lifecycle archive](../secret-substitution-kvm/README.md), which contains
the tested Rust sources, build log and isolated source catalog. No runtime source
or binary changes were needed for these additional body checks.

The new checks observe raw body replacement with surrounding bytes preserved,
form replacement with unrelated plus/percent-decoded values preserved, and a
secret containing spaces, ampersand, plus, percent, equals, slash, query/colon,
quote and backslash characters. That secret round-trips through headers, Basic
auth, query, JSON, form and raw bodies with Content-Length matching actual bytes.
A malformed SIGHUP policy is refused; a subsequent real guest request reaches the
upstream carrying the previous active secret, proving retention beyond a log
message or an empty inventory check.

The previous rotation, hostname exclusion, fork scope exclusion, pause/resume
and revocation checks also pass. Curl verifies owned TLS; every invocation opens
a new connection. Policies, keys, tokens and raw API responses are temporary and
not archived. All owned guests, daemon and listener are cleaned up. Accepted
performance inputs remain unchanged. No managed competitor measurement or
performance win is claimed. Binary raw payloads, concurrent lifecycle/reload
races and managed organization scopes remain incomplete.

Reproduce using the preceding archive's verification binary, kernel and separate
HTTPS client image:
`python3 tools/check-secret-substitution-kvm.py --daemon BIN --kernel KERNEL
--initrd HTTPS_CLIENT_IMAGE --output NEW.json`. Linux KVM, OpenSSL and ip are
required. Outputs must not exist; result.json records exact input SHA-256 values.
