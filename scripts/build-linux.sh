#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root/noc-agent"
source /etc/os-release
label="${1:-${ID}-${VERSION_ID}}"
[[ "$label" =~ ^[a-z0-9.-]+$ ]] || { echo 'Invalid distribution label' >&2; exit 1; }
[[ "$(uname -m)" == x86_64 ]] || { echo 'This packaging script targets x86_64.' >&2; exit 1; }
target=x86_64-unknown-linux-gnu
export NOC_SUITE_VERSION="$(tr -d '\r\n' < "$root/VERSION")"
if [[ "${GITHUB_REF_TYPE:-}" == tag ]]; then NOC_SUITE_VERSION="${GITHUB_REF_NAME#v}"; fi
[[ "$NOC_SUITE_VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[a-zA-Z0-9][a-zA-Z0-9.-]*)?$ ]] || { echo 'Invalid suite version' >&2; exit 1; }
export NOC_UPDATE_ASSET="noc-agent-$label-x64"
rustup target add --toolchain stable "$target"
cargo +stable build --release --locked --target "$target"
binary="target/$target/release/noc-agent"
"$binary" --version
desktop-file-validate noc-agent.desktop
stage="$(mktemp -d)"
ldd "$binary" > "$stage/dependencies.txt"
if grep -q 'not found' "$stage/dependencies.txt"; then
    cat "$stage/dependencies.txt"
    exit 1
fi

name="noc-agent-$label-x64"
package="$stage/$name"
mkdir -p "$package/assets" "$root/dist"
install -m755 "$binary" firefox-flatpak-wrapper.sh "$package/"
install -m644 assets/background.jpg assets/logo.png "$package/assets/"
install -m644 config.example.toml noc-agent.desktop noc-agent.service README.md "$package/"
install -m644 "$root/doc/linux.md" "$package/"
cp "$stage/dependencies.txt" "$package/DEPENDENCIES.txt"
{
    "$binary" --version
    printf 'Distribution: %s\nTarget: %s\nSource: %s\n' "$PRETTY_NAME" "$target" "${GITHUB_SHA:-local}"
    rustc +stable --version
    printf 'Graphical session validation on the destination system is still required.\n'
} > "$package/BUILD-INFO.txt"
(cd "$package" && sha256sum noc-agent > SHA256SUMS.txt)
tar -C "$stage" -czf "$root/dist/$name.tar.gz" "$name"
echo "Package: $root/dist/$name.tar.gz"
# Raw executable for the auto-updater, published alongside the full package.
install -m755 "$binary" "$root/dist/$name"
echo "Package: $root/dist/$name"
