#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
out="${1:-$repo/rust/athena-federation-substrait/tests/fixtures}"
version="${SDK_VERSION:-2022.47.1}"
m2="${M2_REPO:-$HOME/.m2/repository}/com/amazonaws/aws-athena-federation-sdk/$version"
target="$repo/athena-federation-sdk/target"
queries="$here/queries"

pick() {
    for candidate in "$@"; do
        if [ -e "$candidate" ]; then
            echo "$candidate"
            return
        fi
    done
    echo "none of these exist: $*" >&2
    exit 1
}

withdep="$(pick "$m2/aws-athena-federation-sdk-$version-withdep.jar" "$target/aws-athena-federation-sdk-$version-withdep.jar")"
tests="$(pick "$m2/aws-athena-federation-sdk-$version-tests.jar" "$target/test-classes")"
classpath="$withdep:$tests"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

javac -nowarn -d "$work/classes" -cp "$classpath" "$here/VerifyProbe.java" 2> "$work/javac.log" || {
    cat "$work/javac.log" >&2
    exit 1
}

run_list() {
    local list="$1" dest="$2" result="$work/result.tsv"
    rm -f "$result"
    java -Xmx"${PROBE_HEAP:-2g}" -cp "$work/classes:$classpath" VerifyProbe "$list" "$result" 2>> "$work/probe.log"
    if [ -f "$result" ]; then
        cat "$result" >> "$dest"
    fi
}

run_each_line() {
    local list="$1" dest="$2" single="$work/single.txt"
    while IFS= read -r line || [ -n "$line" ]; do
        printf '%s\n' "$line" > "$single"
        run_list "$single" "$dest"
    done < "$list"
}

verifier="$out/calcite-verifier.tsv"
: > "$verifier"
for list in queries2 queries3 queries4 queries q_c; do
    run_list "$queries/$list.txt" "$verifier"
done
run_each_line "$queries/rejected.txt" "$verifier"

: > "$out/calcite-verifier-extra.tsv"
run_list "$queries/verifier-extra.txt" "$out/calcite-verifier-extra.tsv"

: > "$out/calcite-extended.tsv"
run_list "$queries/extended.txt" "$out/calcite-extended.tsv"

grep '^SKIP' "$work/probe.log" >&2 || true
