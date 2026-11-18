#!/usr/bin/env bash
set -euo pipefail

WIRE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MVN="${MVN:-$HOME/opt/apache-maven-3.9.9/bin/mvn}"
PG_IMAGE="${PG_IMAGE:-postgres:16}"
RUST_CMD=""
FILTER=""
ZONE="UTC"
KEEP_CONTAINER=0

usage() {
    cat <<EOF
usage: run.sh [--rust <command>] [--filter <substring>] [--zone <time zone>] [--keep-container]

Starts a seeded PostgreSQL container on 127.0.0.1, regenerates the request
corpus in corpus/, runs every case through the Java PostgreSQL handler and
writes out/java/ and out/java-normalized/.

--rust <command>  After the Java side, run <command> with the same environment
                  plus PARITY_WIRE_CORPUS and PARITY_WIRE_OUT=out/rust. The
                  command must write out/rust/<case>/rust-response.json or
                  rust-error.txt, and rust-aws.json when it touched the AWS
                  stubs. diff.py then compares out/java with out/rust.
--filter <text>   Run only cases whose directory name contains <text>.
--zone <zone>     Run both handlers with this default time zone (JVM
                  user.timezone and TZ). The corpus is still generated in UTC.
--keep-container  Leave the container running (for debugging).
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rust) RUST_CMD="$2"; shift 2 ;;
        --filter) FILTER="$2"; shift 2 ;;
        --zone) ZONE="$2"; shift 2 ;;
        --keep-container) KEEP_CONTAINER=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

CONTAINER="parity-wire-pg-$$"
OUT="$WIRE/out"
CORPUS="$WIRE/corpus"
JAVA_DIR="$WIRE/java"

cleanup() {
    if [[ "$KEEP_CONTAINER" == "0" ]]; then
        docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    else
        echo "container $CONTAINER left running" >&2
    fi
}
trap cleanup EXIT

PORT="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
PG_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_hex(16))')"
export PG_PASSWORD

echo "starting $PG_IMAGE as $CONTAINER on 127.0.0.1:$PORT"
docker run -d --name "$CONTAINER" \
    -e POSTGRES_USER=parity \
    -e POSTGRES_PASSWORD="$PG_PASSWORD" \
    -e POSTGRES_DB=paritydb \
    -e TZ=UTC \
    -e PGTZ=UTC \
    -p "127.0.0.1:$PORT:5432" \
    "$PG_IMAGE" >/dev/null

for _ in $(seq 1 60); do
    if docker exec "$CONTAINER" pg_isready -h 127.0.0.1 -U parity -d paritydb >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
docker exec "$CONTAINER" pg_isready -h 127.0.0.1 -U parity -d paritydb >/dev/null

docker exec -i "$CONTAINER" psql -v ON_ERROR_STOP=1 -q -U parity -d paritydb < "$WIRE/seed/seed.sql"
echo "seeded"

"$MVN" -o -q -f "$JAVA_DIR/pom.xml" compile dependency:build-classpath -Dmdep.outputFile="$JAVA_DIR/target/classpath.txt"
CLASSPATH_FILE="$JAVA_DIR/target/argfile"
printf -- '-cp %s:%s:%s\n' "$JAVA_DIR/target/classes" "$JAVA_DIR/src/main/resources" "$(cat "$JAVA_DIR/target/classpath.txt")" > "$CLASSPATH_FILE"

mkdir -p "$OUT"
JAVA_OPTS=(
    --add-opens=java.base/java.nio=ALL-UNNAMED
    -Duser.timezone=UTC
    -Dlog4j2.configurationFile=parity-log4j2.xml
    "@$CLASSPATH_FILE"
)

java "${JAVA_OPTS[@]}" -Dparity.log.file="$OUT/corpus-gen.log" com.amazonaws.athena.parity.wire.CorpusGen "$CORPUS"
JAVA_OPTS[1]="-Duser.timezone=$ZONE"

JDBC_URL="jdbc:postgresql://127.0.0.1:$PORT/paritydb?user=parity&password=$PG_PASSWORD"
CONNECTOR_ENV=(
    "default=postgres://$JDBC_URL"
    "pgmux_connection_string=postgres://$JDBC_URL"
    "PARITY_PG_HOSTPORT=127.0.0.1:$PORT"
    "AWS_LAMBDA_FUNCTION_NAME=parity-pg-fn"
    "spill_bucket=parity-spill-bucket"
    "spill_prefix=athena-spill"
    "disable_spill_encryption=true"
    "AWS_REGION=us-east-1"
    "AWS_ACCESS_KEY_ID=parity-stub"
    "AWS_SECRET_ACCESS_KEY=parity-stub"
    "AWS_EC2_METADATA_DISABLED=true"
    "TZ=$ZONE"
)

rm -rf "$OUT/java" "$OUT/java-normalized"
env "${CONNECTOR_ENV[@]}" java "${JAVA_OPTS[@]}" -Dparity.log.file="$OUT/java.log" \
    com.amazonaws.athena.parity.wire.JavaRunner "$CORPUS" "$OUT/java" ${FILTER:+"$FILTER"} > "$OUT/java-runner.txt"
tail -n 1 "$OUT/java-runner.txt"

python3 "$WIRE/diff.py" normalize "$OUT/java" "$OUT/java-normalized"

check_leak() {
    if grep -rlF -- "$PG_PASSWORD" "$@" >/dev/null 2>&1; then
        echo "the database password leaked into these files:" >&2
        grep -rlF -- "$PG_PASSWORD" "$@" >&2
        exit 1
    fi
}
check_leak "$OUT" "$CORPUS"

if [[ -n "$RUST_CMD" ]]; then
    rm -rf "$OUT/rust" "$OUT/rust-normalized"
    mkdir -p "$OUT/rust"
    env "${CONNECTOR_ENV[@]}" PARITY_WIRE_CORPUS="$CORPUS" PARITY_WIRE_OUT="$OUT/rust" PARITY_WIRE_FILTER="$FILTER" \
        bash -c "$RUST_CMD"
    check_leak "$OUT/rust"
    python3 "$WIRE/diff.py" normalize "$OUT/rust" "$OUT/rust-normalized"
    python3 "$WIRE/diff.py" compare "$OUT/java" "$OUT/rust" > "$OUT/diff-java-rust.txt" || status=$?
    grep -E '^(GROUP|SUMMARY)' "$OUT/diff-java-rust.txt"
    exit "${status:-0}"
fi
