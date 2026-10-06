# Real KVM binary payloads and upstream hostname refusal

The extended owned checker passes fourteen checks, with nineteen successful
upstream HTTPS requests and one rejected wrong-hostname attempt. Input hashes
exactly match the preceding secret-registration-kvm archive, which contains the
verified daemon source, build context and test logs. No runtime changes or
benchmark input changes were needed for these additional checks.

Binary payloads are written inside the guest using the existing BusyBox base64
applet and sent with curl --data-binary from a file. The upstream observes NUL
and non-UTF8 bytes unchanged around the replaced placeholder. Both the ordinary
secret and the delimiter-bearing secret are verified with correct Content-Length.

A third hostname is added explicitly to the sandbox's secret scope but is absent
from the owned server certificate SANs. Curl trusts the interception CA; the
gateway trusts the owned upstream root. The request fails, gateway decisions
confirm that this name entered HTTPS interception, and no HTTP payload reaches
the owned server. A subsequent request to the valid hostname succeeds. This
checks name verification through KVM rather than relying only on library tests.

Earlier body/lifecycle checks remain: headers, Basic auth, query, JSON, form,
raw bodies, malformed reload retention, hostname exclusion, rotation, fork scope
exclusion, pause/resume, overlapping resume/reload calls and revocation. All
owned guests and processes are cleaned up; private files and raw responses are
not archived. Binary guest files are removed after successful requests.

Each curl invocation creates a connection. Exact registration interleavings and
failed-pause recovery are not forced. Managed organization scope, competitor
measurement and performance superiority remain unverified. Accepted benchmark
inputs remain unchanged.

Reproduce with the preceding verification binary and separately built HTTPS
client image: `python3 tools/check-secret-substitution-kvm.py --daemon BIN
--kernel KERNEL --initrd HTTPS_CLIENT_IMAGE --output NEW.json`. Linux KVM,
OpenSSL, ip and an unused report path are required.
