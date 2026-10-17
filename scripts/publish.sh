#!/usr/bin/env bash
set -euo pipefail

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
fi

# Ordered by the crate dependency DAG: foundation -> engine -> render -> app.
# Format: "<directory>:<published package name>" (directory names are unchanged;
# package names carry the compograph- prefix).
CRATES=(
    "crates/foundation/cg-types:compograph-types"
    "crates/foundation/cg-geometry:compograph-geometry"
    "crates/foundation/cg-graph:compograph-graph"
    "crates/engine/cg-layout:compograph-layout"
    "crates/render/cg-render:compograph-render"
    "crates/render/cg-interact:compograph-interact"
    "crates/app/compograph:compograph"
)

for entry in "${CRATES[@]}"; do
    crate="${entry%%:*}"
    name="${entry##*:}"
    # Extract the package version; fall back to the workspace version
    # for crates using `version.workspace = true`.
    version=$(sed -n 's/^version *= *"\(.*\)".*/\1/p' "$crate/Cargo.toml" | head -1)
    if [[ -z "$version" ]]; then
        version=$(sed -n 's/^version *= *"\(.*\)".*/\1/p' Cargo.toml | head -1)
    fi
    echo "=== Publishing $name ==="
    # Skip if this version is already on crates.io (User-Agent is
    # required by the crates.io API).
    if curl -sf -A "compograph-publish-script" -o /dev/null "https://crates.io/api/v1/crates/$name/$version"; then
        echo "Skipping $name v$version: already published."
        echo ""
        continue
    fi
    set +e
    if $DRY_RUN; then
        (cd "$crate" && cargo publish --dry-run --allow-dirty --registry crates-io)
    else
        (cd "$crate" && cargo publish --allow-dirty --registry crates-io)
    fi
    rc=$?
    set -e
    if [[ $rc -ne 0 ]]; then
        # In dry-run mode, unpublished internal dependencies cannot be
        # resolved from the real index yet; warn instead of aborting.
        if $DRY_RUN; then
            echo "WARNING: dry-run failed for $name (likely unpublished internal deps); continuing."
        else
            exit $rc
        fi
    fi
done

echo "All crates published successfully."
