# Releasing the SDKs

The [Python](../sdk/python/README.md) and [TypeScript](../sdk/typescript/README.md)
clients are packaged for PyPI and npm, and tested as those packages. They are
**not published**: that needs accounts only an owner has, and a published
version cannot be taken back. This is what is in place, and what has to be
done once before the first release.

## What is in place

- **Packaging.** Each package carries the repository's `LICENSE` and
  `LICENSE-COMMERCIAL` (copies, which the check below keeps identical to the
  root's), its README, and the metadata a registry shows. The Python
  licence is the SPDX expression `AGPL-3.0-only OR LicenseRef-Commercial`
  (PEP 639), so it needs setuptools 77 or later to build.
- **`tools/check-sdk-packages.sh`** builds both packages and tests them as
  installed, not from the checkout:
  - `python DIST` builds the sdist and wheel, runs `twine check --strict`,
    checks that both hold the licence texts, installs the wheel into an
    empty virtual environment and runs the SDK's tests there. (The tests
    are copied out first: they put their own directory's parent on the
    path, so run in place they would import the source tree.)
  - `node DIST` packs the package, checks that it holds exactly
    `index.js`, `index.d.ts`, `README.md`, `package.json` and the two
    licences, installs the tarball into an empty project and imports it.
  - `version X` checks that `pyproject.toml`, `__version__` and
    `package.json` all say `X`.
- **`.github/workflows/sdk-packages.yml`** runs that on every change to the
  SDKs. For a tag named `sdk-vX.Y.Z` it also checks the tag against the
  three versions, then publishes the packages it just tested: to PyPI by
  trusted publishing, so no PyPI token is stored, and to npm with
  provenance. Both publish jobs need the `sdk-release` environment's
  approval.

## Once, before the first release

1. **PyPI.** Add a *pending publisher* for the project `hypermachine`, at
   <https://pypi.org/manage/account/publishing/>: owner `nervosys`,
   repository `HyperMachine`, workflow `sdk-packages.yml`, environment
   `sdk-release`. Nothing is stored in GitHub for PyPI.
2. **npm.** Own the `@nervosys` scope, make a granular access token that can
   publish packages in it, and store it as the secret `NPM_TOKEN` of the
   **`sdk-release` environment** (not the repository, so only an approved
   run can read it).
3. **GitHub.** Create the environment `sdk-release`, with **required
   reviewers** and deployment limited to `sdk-v*` tags. Without reviewers a
   tag publishes as soon as 1 and 2 are done. Consider a tag protection rule
   for `sdk-v*` too.

As of 2026-10-16 neither name was taken (`hypermachine` on PyPI and
`@nervosys/hypermachine` on npm both answered 404), which reserves nothing,
and npm answers the same whether or not the scope exists.

## A release

1. Set the same version in `sdk/python/pyproject.toml`,
   `sdk/python/hypermachine/__init__.py` and `sdk/typescript/package.json`,
   and merge.
2. Tag the merge `sdk-vX.Y.Z` and push the tag.
3. Approve the `sdk-release` environment for each publish job.
4. Afterwards change the two READMEs, which say each client "is not on PyPI"
   / "is not on npm", to the install commands.

## Not verified

- **Any upload.** The packages were built and tested, never uploaded, so
  PyPI's acceptance of the `LicenseRef-Commercial` licence expression,
  npm's handling of the same string in `license`, trusted publishing and
  provenance are untested. `twine check --strict` passes and `npm pack`
  prints no warning.
- **The publish jobs themselves.** They did not run: they need a tag, an
  approved environment and the setup above.
- **`sdk/node`** (`@nervosys/hypermachine-sandbox`) is a third package, not
  marked `private` and not covered by any of this. `npm publish` in it
  would publish it.
