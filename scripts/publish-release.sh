#!/usr/bin/env bash
set -euo pipefail

: "${RELEASE_TAG:?Release tag is required}"
: "${GH_REPO:?GitHub repository is required}"
: "${GH_TOKEN:?GitHub token is required}"
if [[ ! "$RELEASE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9][a-zA-Z0-9.-]*)?$ ]]; then
    echo 'Expected a tag such as v1.0.0 or v1.1.0-rc.1.' >&2
    exit 1
fi

assets=(
    release-assets/noc-manager-windows-server-2012-x64.zip
    release-assets/noc-manager-windows-server-2016-x64.zip
    release-assets/noc-display-windows-server-2012-x64.zip
    release-assets/noc-display-windows-server-2016-x64.zip
    release-assets/noc-agent-ubuntu-22.04-x64.tar.gz
    release-assets/noc-agent-ubuntu-24.04-x64.tar.gz
    # Raw executables consumed directly by the auto-updater (no archive to open).
    release-assets/noc-manager.exe
    release-assets/noc-display.exe
    release-assets/noc-agent-ubuntu-22.04-x64
    release-assets/noc-agent-ubuntu-24.04-x64
)
for asset in "${assets[@]}"; do
    [[ -s "$asset" ]] || { echo "Missing or empty package: $asset" >&2; exit 1; }
done

if draft=$(gh release view "$RELEASE_TAG" --json isDraft --jq .isDraft); then
    if [[ "$draft" == false ]]; then
        echo "Release $RELEASE_TAG already published; keeping its existing assets."
        exit 0
    fi
    [[ "$draft" == true ]] || { echo 'Unexpected release status.' >&2; exit 1; }
else
    options=(--verify-tag --draft --generate-notes --title "NOC $RELEASE_TAG")
    if [[ "$RELEASE_TAG" == *-* ]]; then
        options+=(--prerelease)
    fi
    gh release create "$RELEASE_TAG" "${options[@]}"
fi

# Keep incomplete uploads in a draft; a retry can finish that same draft.
gh release upload "$RELEASE_TAG" "${assets[@]}" --clobber
gh release edit "$RELEASE_TAG" --draft=false
echo "Published NOC $RELEASE_TAG with all ten packages."
