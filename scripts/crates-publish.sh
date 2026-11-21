#!/usr/bin/env bash
# Publish the library crates to crates.io, in dependency order.
#
# Auth: crates.io Trusted Publishing — the job exchanges its OIDC JWT
# (CRATES_IO_ID_TOKEN, minted by the `id_tokens` keyword) for a short-lived
# crates.io token. No API token is stored anywhere; the exchanged token
# dies with the job.
#
# Idempotent: a version that already exists on crates.io is skipped, so a
# tag that doesn't bump every crate still completes, and a failed run can
# simply be retried — "already published" is success, not error.
set -euo pipefail

UA="minipro-rs-ci (https://gitlab.com/arcturus8081/minipro-rs)"

exchange_token() {
    curl -sf -X POST "https://crates.io/api/v1/trusted_publishing/tokens" \
        -H "Content-Type: application/json" \
        -H "User-Agent: ${UA}" \
        -d "{\"jwt\": \"${CRATES_IO_ID_TOKEN}\"}" | jq -r '.token'
}

# name version → 0 when the crate+version exists on crates.io
crate_exists() {
    curl -sf -H "User-Agent: ${UA}" \
        "https://crates.io/api/v1/crates/$1/$2" > /dev/null
}

# name version → 0 when the version is visible in the sparse index.
# `cargo publish` verifies path-dep replacements against the index, not the
# API, so a freshly-published dep must be index-visible before dependents
# can publish.
index_has() {
    local n=$1 path l
    l=${#n}
    if (( l >= 4 )); then path="${n:0:2}/${n:2:2}/$n"
    elif (( l == 3 )); then path="3/${n:1:1}/$n"
    elif (( l == 2 )); then path="2/$n"
    else path="1/$n"; fi
    curl -sf -H "User-Agent: ${UA}" "https://index.crates.io/${path}?t=$(date +%s)" \
        | grep -q "\"vers\":\"$2\""
}

wait_for_index() {  # name version — poll up to ~5 minutes
    local i
    for i in $(seq 1 30); do
        index_has "$1" "$2" && return 0
        sleep 10
    done
    echo "error: timed out waiting for $1 $2 in the crates.io index" >&2
    return 1
}

# Resolve a crate dir's version — `version.workspace` entries read the
# workspace-root manifest instead.
version_of() {
    local v
    # `^version *=` deliberately excludes `version.workspace = true`.
    v=$(grep -m1 '^version *=' "$1/Cargo.toml" | cut -d'"' -f2)
    if [ -z "$v" ]; then
        v=$(awk '/^\[workspace\.package\]/{f=1} f&&/^version/{print;exit}' \
            Cargo.toml | cut -d'"' -f2)
    fi
    echo "$v"
}

publish_if_new() {  # crate-dir crate-name
    local ver
    ver=$(version_of "$1")
    if [ -z "$ver" ]; then
        echo "error: could not determine version for $2 ($1)" >&2
        return 1
    fi
    if crate_exists "$2" "$ver"; then
        echo "$2 $ver already published — skipping."
        return 0
    fi
    echo "Publishing $2 $ver …"
    (cd "$1" && cargo publish)
    wait_for_index "$2" "$ver"
}

export CARGO_REGISTRY_TOKEN
CARGO_REGISTRY_TOKEN=$(exchange_token)

# rar4 first: minipro-core depends on it and `cargo publish`
# resolves the dep through crates.io, so it must exist and be
# index-visible before core publishes.
publish_if_new crates/rar4 rar4
publish_if_new crates/minipro-core minipro-core

echo "crates.io publish step complete."
