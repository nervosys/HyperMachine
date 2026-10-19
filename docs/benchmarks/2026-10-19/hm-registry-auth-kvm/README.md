# `hm sandbox vm run` and `build` against a private registry, on KVM (2026-10-19)

**What this checks:** that `--registry-user` and `--registry-password-env` let the
real `hm` binary pull from a registry that wants a password, with no Docker
anywhere, and that the password goes nowhere it should not. See
[`RUN_IMAGES.md`](../../../RUN_IMAGES.md).

## Method

`tools/check-hm-registry-auth-kvm.py` (6 cases) runs an owned `hv2-sandboxd`
with a guest kit on real KVM (WSL2, nested) and a registry of its own on
localhost that serves one tiny image and answers 401 to any request without the
right Basic credentials. The registry is this script's, not a real service: it
speaks Basic auth only, not the Bearer token flow.

1. Without credentials `run` fails and the registry saw no authorised request.
2. With the wrong password `run` fails.
3. With both flags `run` pulls the image and prints its marker file.
4. The password is in no daemon log line, no `GET /templates` answer and none of
   `hm`'s command lines (it is read from an environment variable).
5. One flag without the other, an unset or empty variable, and a malformed
   variable name each fail before the registry is contacted.
6. `build` of a Dockerfile `FROM` the private image fails without the flags and
   works with them.

## Result

6 of 6 cases passed on 3 runs in a row (`runs/run1..3.json`; `report.json` is
run 1). The inputs' SHA-256 sums are in each report.

## Not verified

- A real registry (Docker Hub, GHCR, ECR) with Bearer tokens: the OCI client's
  Bearer path predates this change and was not exercised here.
- Credentials for a registry other than the `FROM` image of a build.
- The password sits in the node's memory while the pull runs, and is not stored
  or echoed; that it is absent from a core dump is not checked.
