#!/usr/bin/env bash
# The SDKs as the packages a registry would hold, built and tested as installed.
#
#   tools/check-sdk-packages.sh python DIST   build the Python sdist and wheel
#   tools/check-sdk-packages.sh node DIST     pack the TypeScript package
#   tools/check-sdk-packages.sh version X     both packages say version X
#
# `python` builds the packages, checks their metadata (twine, strictly),
# checks that each holds the licence texts, installs the wheel into an empty
# virtual environment and runs the SDK's tests there, with the tests copied
# out of the checkout: they put their own directory's parent on the path, so
# run in place they would test the source tree and not the package.
#
# `node` packs the package, checks that it holds exactly the files meant for
# a registry, installs the tarball into an empty project and imports it.
#
# Nothing here publishes. The workflow that does is
# .github/workflows/sdk-packages.yml, and only for a tag.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
what=${1:?python, node or version}

licences_match() {
  # The copies in each package are the repository's, byte for byte.
  local dir=$1 file
  for file in LICENSE LICENSE-COMMERCIAL; do
    cmp "$here/$file" "$here/$dir/$file" ||
      { echo "$dir/$file differs from the repository's $file" >&2; return 1; }
  done
}

python_package() {
  local dist work build run wheel sdist
  mkdir -p "$2"
  dist=$(cd "$2" && pwd)
  work=$(mktemp -d)
  # Expanded now: the trap runs at exit, when this function's locals are gone.
  trap "rm -rf '$work'" EXIT
  licences_match sdk/python

  python3 -m venv "$work/build"
  build="$work/build/bin/python"
  "$build" -m pip install --quiet --disable-pip-version-check 'build>=1.2' 'twine>=6'
  # Built from a copy: setuptools writes build/ and an egg-info beside the
  # source, and a check should leave the checkout as it found it.
  mkdir "$work/src"
  cp -r "$here/sdk/python/." "$work/src/"
  "$build" -m build --outdir "$dist" "$work/src"
  wheel=$(ls "$dist"/hypermachine-*.whl)
  sdist=$(ls "$dist"/hypermachine-*.tar.gz)
  "$build" -m twine check --strict "$wheel" "$sdist"

  "$build" - "$wheel" "$sdist" <<'PY'
import sys, tarfile, zipfile
wheel, sdist = sys.argv[1:3]
in_wheel = zipfile.ZipFile(wheel).namelist()
in_sdist = tarfile.open(sdist).getnames()
for name in ("LICENSE", "LICENSE-COMMERCIAL"):
    assert any(n.endswith(f".dist-info/licenses/{name}") for n in in_wheel), f"the wheel holds no {name}"
    assert any(n.endswith(f"/{name}") for n in in_sdist), f"the sdist holds no {name}"
print("licences are in the wheel and the sdist")
PY

  python3 -m venv "$work/run"
  run="$work/run/bin/python"
  "$run" -m pip install --quiet --disable-pip-version-check "$wheel"
  mkdir "$work/tests"
  cp -r "$here/sdk/python/tests/." "$work/tests/"
  (
    cd "$work"
    "$run" -I -c '
import hypermachine
assert "site-packages" in hypermachine.__file__, hypermachine.__file__
print("hypermachine", hypermachine.__version__, "from", hypermachine.__file__)'
    "$run" -I -m unittest discover -s "$work/tests"
  )
  echo "python package: $(basename "$wheel"), $(basename "$sdist")"
}

node_package() {
  local dist work tarball expected actual
  mkdir -p "$2"
  dist=$(cd "$2" && pwd)
  work=$(mktemp -d)
  trap "rm -rf '$work'" EXIT
  licences_match sdk/typescript

  (cd "$here/sdk/typescript" && npm pack --pack-destination "$dist" --silent >/dev/null)
  tarball=$(ls "$dist"/nervosys-hypermachine-*.tgz)

  expected=$(printf '%s\n' LICENSE LICENSE-COMMERCIAL README.md index.d.ts index.js package.json | LC_ALL=C sort)
  actual=$(tar -tzf "$tarball" | sed 's|^package/||' | LC_ALL=C sort)
  if [ "$expected" != "$actual" ]; then
    echo "the tarball holds the wrong files:" >&2
    diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
    exit 1
  fi

  (
    cd "$work"
    npm init -y >/dev/null
    npm install --silent --no-audit --no-fund "$tarball"
    node --input-type=module -e '
import * as sdk from "@nervosys/hypermachine";
for (const name of ["ApiError", "Client", "Machine", "Run", "Prompt", "Conversation", "Snapshot"]) {
  if (typeof sdk[name] !== "function") throw new Error("the package does not export " + name);
}
console.log("@nervosys/hypermachine imports, exporting", Object.keys(sdk).length, "names");'
    test -f node_modules/@nervosys/hypermachine/index.d.ts
  )
  echo "node package: $(basename "$tarball")"
}

versions_are() {
  local want=$1 py ts init
  py=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$here/sdk/python/pyproject.toml")
  init=$(sed -n 's/^__version__ = "\(.*\)"$/\1/p' "$here/sdk/python/hypermachine/__init__.py")
  ts=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$here/sdk/typescript/package.json")
  echo "pyproject $py, __version__ $init, package.json $ts; wanted $want"
  [ "$py" = "$want" ] && [ "$init" = "$want" ] && [ "$ts" = "$want" ]
}

case "$what" in
  python) python_package "$@" ;;
  node) node_package "$@" ;;
  version) versions_are "${2:?a version}" ;;
  *) echo "usage: $0 python DIST | node DIST | version X" >&2; exit 2 ;;
esac
