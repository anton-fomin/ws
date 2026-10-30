#!/bin/sh
# Install a prebuilt ws release from GitHub.
#
#   curl -fsSL https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/anton-fomin/ws/main/install.sh | sh -s -- --version v0.7.0
#
# Run it again to upgrade. Uninstall with `rm ~/.local/bin/ws`.

set -eu

repo="anton-fomin/ws"
# The gnu builds come from ubuntu-24.04 and need at least this glibc.
min_glibc_major=2
min_glibc_minor=39

usage() {
	cat <<EOF
Install ws from a GitHub release.

Usage: install.sh [options]

Options:
  --version <tag>      release to install, e.g. v0.7.0 (default: the latest)
  --bin-dir <dir>      where to put the binary (default: \$WS_INSTALL_DIR or ~/.local/bin)
  --libc <musl|gnu>    Linux only: static musl build (default) or the glibc build,
                       which needs glibc ${min_glibc_major}.${min_glibc_minor} or newer
  -h, --help           show this help
EOF
}

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() {
	printf 'error: %s\n' "$*" >&2
	exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }

version=""
bin_dir="${WS_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"
libc=""

while [ $# -gt 0 ]; do
	case $1 in
	--version)
		[ $# -ge 2 ] || die "--version needs a value"
		version=$2
		shift 2
		;;
	--version=*)
		version=${1#*=}
		shift
		;;
	--bin-dir)
		[ $# -ge 2 ] || die "--bin-dir needs a value"
		bin_dir=$2
		shift 2
		;;
	--bin-dir=*)
		bin_dir=${1#*=}
		shift
		;;
	--libc)
		[ $# -ge 2 ] || die "--libc needs a value"
		libc=$2
		shift 2
		;;
	--libc=*)
		libc=${1#*=}
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) die "unknown option: $1 (see --help)" ;;
	esac
done

# --- tools ---------------------------------------------------------------

if has curl; then
	fetch() { curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"; }
elif has wget; then
	fetch() { wget -q -O "$2" "$1"; }
else
	die "need curl or wget to download the release"
fi
has tar || die "need tar to unpack the release"
if has sha256sum; then
	sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
elif has shasum; then
	sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
	die "need sha256sum or shasum to verify the download"
fi

# --- platform ------------------------------------------------------------

os=$(uname -s)
arch=$(uname -m)
case $arch in
x86_64 | amd64) arch=x86_64 ;;
aarch64 | arm64) arch=aarch64 ;;
*) die "no prebuilt ws for the $arch architecture; build it with cargo or nix instead" ;;
esac

case $os in
Linux)
	case ${libc:-musl} in
	musl) ;;
	gnu)
		found=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed 's/^glibc //') || found=""
		major=${found%%.*}
		minor=${found#*.}
		minor=${minor%%.*}
		case $major$minor in
		'' | *[!0-9]*) die "--libc gnu needs glibc ${min_glibc_major}.${min_glibc_minor}, and none was found; drop --libc gnu" ;;
		esac
		if [ "$major" -lt "$min_glibc_major" ] ||
			{ [ "$major" -eq "$min_glibc_major" ] && [ "$minor" -lt "$min_glibc_minor" ]; }; then
			die "--libc gnu needs glibc ${min_glibc_major}.${min_glibc_minor}, found $found; drop --libc gnu"
		fi
		;;
	*) die "--libc must be musl or gnu, not $libc" ;;
	esac
	target="$arch-unknown-linux-${libc:-musl}"
	;;
Darwin)
	[ -z "$libc" ] || die "--libc only applies to Linux"
	# A shell running under Rosetta reports x86_64 on Apple Silicon.
	if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ]; then
		arch=aarch64
	fi
	target="$arch-apple-darwin"
	;;
*) die "no prebuilt ws for $os; build it with cargo or nix instead" ;;
esac

# --- version -------------------------------------------------------------

if [ -z "$version" ]; then
	# releases/latest redirects to releases/tag/<tag>, which avoids the API and its rate limit.
	latest="https://github.com/$repo/releases/latest"
	if has curl; then
		url=$(curl -fsSLI --proto '=https' --tlsv1.2 -o /dev/null -w '%{url_effective}' "$latest") ||
			die "could not look up the latest release"
		version=${url##*/releases/tag/}
		[ "$version" != "$url" ] || version=""
	else
		version=$(wget -S -O /dev/null "$latest" 2>&1 |
			sed -n 's|.*[Ll]ocation: .*/releases/tag/\([^[:space:]]*\).*|\1|p' | tail -n 1)
	fi
	[ -n "$version" ] || die "could not look up the latest release; pass --version"
fi
case $version in
v*) ;;
*) version="v$version" ;;
esac

# --- download and verify -------------------------------------------------

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t ws-install)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

asset="ws-$version-$target.tar.gz"
base="https://github.com/$repo/releases/download/$version"

say "Downloading ws $version for $target"
fetch "$base/$asset" "$tmp/$asset" ||
	die "could not download $base/$asset (is there a $target build of $version?)"
fetch "$base/$asset.sha256" "$tmp/$asset.sha256" ||
	die "could not download the checksum $base/$asset.sha256"

expected=$(cut -d ' ' -f 1 <"$tmp/$asset.sha256")
actual=$(sha256 "$tmp/$asset")
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
	die "checksum mismatch for $asset: expected $expected, got $actual"
fi

mkdir "$tmp/unpacked"
tar -xzf "$tmp/$asset" -C "$tmp/unpacked"
[ -f "$tmp/unpacked/ws" ] || die "$asset does not contain ws"

# --- install -------------------------------------------------------------

as_root=""
if ! mkdir -p "$bin_dir" 2>/dev/null || [ ! -w "$bin_dir" ]; then
	has sudo || die "$bin_dir is not writable and sudo is not available; pass --bin-dir"
	say "$bin_dir is not writable, using sudo"
	as_root="sudo"
	$as_root mkdir -p "$bin_dir"
fi

dest="$bin_dir/ws"
previous=""
if [ -x "$dest" ]; then
	previous=$("$dest" --version 2>/dev/null | cut -d ' ' -f 2) || previous=""
fi

# Stage next to the destination and try it there before replacing anything, so a
# binary that cannot run on this system never takes the place of a working one.
$as_root cp "$tmp/unpacked/ws" "$dest.new"
$as_root chmod 755 "$dest.new"
if ! installed=$("$dest.new" --version 2>/dev/null); then
	$as_root rm -f "$dest.new"
	die "the downloaded ws does not run on this system; nothing was changed"
fi
$as_root mv -f "$dest.new" "$dest"
installed=${installed#ws }

if [ -n "$previous" ] && [ "$previous" != "$installed" ]; then
	say "Upgraded ws v$previous -> v$installed in $dest"
elif [ -n "$previous" ]; then
	say "Reinstalled ws v$installed in $dest"
else
	say "Installed ws v$installed in $dest"
fi

# --- next steps ----------------------------------------------------------

case ":${PATH:-}:" in
*":$bin_dir:"*) ;;
*)
	say "" >&2
	warn "$bin_dir is not on PATH; add it in your shell's rc file:"
	say "  export PATH=\"$bin_dir:\$PATH\"" >&2
	;;
esac

if ! has git; then
	say "" >&2
	warn "git is not installed, and ws needs it"
fi

cat <<'EOF'

Next steps:
  eval "$(ws shell-init zsh)"     in ~/.zshrc (or bash), so `ws switch` also cds
  ws completions zsh > ...        shell completion, see the README
  tmux or herdr, fzf, zoxide      optional, see https://github.com/anton-fomin/ws#install
EOF
