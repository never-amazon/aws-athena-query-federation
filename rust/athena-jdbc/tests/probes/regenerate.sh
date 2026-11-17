#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
java_repo="${JAVA_REPO:?set JAVA_REPO to a checkout with athena-jdbc and athena-federation-sdk built by Maven}"
out="${1:-$here/../fixtures/split_query_builder_integration.tsv}"
version="${SDK_VERSION:-2022.47.1}"
m2="${M2_REPO:-$HOME/.m2/repository}"

classpath="$java_repo/athena-jdbc/target/athena-jdbc-$version.jar"
classpath="$classpath:$java_repo/athena-jdbc/target/test-classes"
classpath="$classpath:$java_repo/athena-federation-sdk/target/aws-athena-federation-sdk-$version-tests.jar"
classpath="$classpath:$m2/com/h2database/h2/2.4.240/h2-2.4.240.jar"
classpath="$classpath:$m2/org/mockito/mockito-core/4.11.0/mockito-core-4.11.0.jar"
classpath="$classpath:$m2/net/bytebuddy/byte-buddy/1.12.19/byte-buddy-1.12.19.jar"
classpath="$classpath:$m2/net/bytebuddy/byte-buddy-agent/1.12.19/byte-buddy-agent-1.12.19.jar"
classpath="$classpath:$m2/org/objenesis/objenesis/3.3/objenesis-3.3.jar"
classpath="$classpath:$m2/org/junit/jupiter/junit-jupiter-api/5.14.4/junit-jupiter-api-5.14.4.jar"
classpath="$classpath:$m2/org/junit/jupiter/junit-jupiter-params/5.14.4/junit-jupiter-params-5.14.4.jar"
classpath="$classpath:$m2/org/opentest4j/opentest4j/1.3.0/opentest4j-1.3.0.jar"
classpath="$classpath:$m2/org/junit/platform/junit-platform-commons/1.14.4/junit-platform-commons-1.14.4.jar"
classpath="$classpath:$m2/junit/junit/4.13.2/junit-4.13.2.jar"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
javac -nowarn -d "$work" -cp "$classpath" "$here/SplitQueryBuilderIntegrationProbe.java"
java -cp "$work:$classpath" SplitQueryBuilderIntegrationProbe "$out" 2> "$work/probe.log" || {
    cat "$work/probe.log" >&2
    exit 1
}
