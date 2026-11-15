#!/usr/bin/env bash
# Firmware binaries cannot link as host executables; exercise the production
# parsing/relocation primitives directly, without substituting a test copy.
set -euo pipefail
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../src" && pwd)
test_dir=$(mktemp -d)
trap 'rm -rf -- "$test_dir"' EXIT
for module in boot_image image_integrity; do
    rustc --edition 2021 --test "$source_dir/$module.rs" -o "$test_dir/$module"
    "$test_dir/$module"
done
