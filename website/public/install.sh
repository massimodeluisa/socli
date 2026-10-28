#!/bin/sh
# Installs the latest public SoCLI release on Linux.
# Debian-family systems get the .deb, RPM systems the .rpm, Alpine the .apk.
# Arch, Omarchy, and anything else get the musl binary, because there is no
# pacman package. Alpine's apk is not signed, so it is installed with
# --allow-untrusted only after SHA256SUMS matches.
# https://socli.sh/install.sh
set -eu

repo=${SOCLI_REPO:-massimodeluisa/socli}

die() {
  printf 'socli: %s\n' "$*" >&2
  exit 1
}

family_for() {
  blob=$(printf '%s %s' "$1" "$2" | tr '[:upper:]' '[:lower:]')
  case "$blob" in
    *debian*|*ubuntu*|*linuxmint*|*pop*|*raspbian*|*elementary*|*kali*)
      printf '%s\n' deb
      ;;
    *fedora*|*rhel*|*centos*|*rocky*|*alma*|*amzn*|*nobara*|*opensuse*|*sles*|*suse*)
      printf '%s\n' rpm
      ;;
    *alpine*)
      printf '%s\n' apk
      ;;
    *)
      printf '%s\n' tar
      ;;
  esac
}

cpu_for() {
  case "$1" in
    x86_64|amd64) printf '%s\n' x86_64 ;;
    aarch64|arm64) printf '%s\n' aarch64 ;;
    armv7l|armv7|armhf) printf '%s\n' armv7 ;;
    i386|i486|i586|i686) printf '%s\n' i386 ;;
    *) return 1 ;;
  esac
}

version_ok() {
  case "$1" in
    *[!0-9A-Za-z.-]*) return 1 ;;
    [0-9]*.[0-9]*.[0-9]*) return 0 ;;
    *) return 1 ;;
  esac
}

triple_for() {
  case "$1" in
    x86_64) printf '%s\n' x86_64-unknown-linux-musl ;;
    aarch64) printf '%s\n' aarch64-unknown-linux-musl ;;
    armv7) printf '%s\n' armv7-unknown-linux-musleabihf ;;
    i386) printf '%s\n' i686-unknown-linux-musl ;;
    *) return 1 ;;
  esac
}

asset_for() {
  family=$1
  cpu=$2
  version=$3
  case "$family:$cpu" in
    deb:x86_64) printf 'socli_%s-1_amd64.deb\n' "$version" ;;
    deb:aarch64) printf 'socli_%s-1_arm64.deb\n' "$version" ;;
    deb:armv7) printf 'socli_%s-1_armhf.deb\n' "$version" ;;
    deb:i386) printf 'socli_%s-1_i386.deb\n' "$version" ;;
    rpm:x86_64) printf 'socli-%s-1.x86_64.rpm\n' "$version" ;;
    rpm:aarch64) printf 'socli-%s-1.aarch64.rpm\n' "$version" ;;
    rpm:armv7) printf 'socli-%s-1.armv7hl.rpm\n' "$version" ;;
    rpm:i386) printf 'socli-%s-1.i386.rpm\n' "$version" ;;
    apk:x86_64) printf 'socli_%s-r1_x86_64.apk\n' "$version" ;;
    apk:aarch64) printf 'socli_%s-r1_aarch64.apk\n' "$version" ;;
    apk:armv7) printf 'socli_%s-r1_armv7.apk\n' "$version" ;;
    apk:i386) printf 'socli_%s-r1_x86.apk\n' "$version" ;;
    tar:*)
      triple=$(triple_for "$cpu") || return 1
      printf 'socli-%s-%s.tar.gz\n' "$version" "$triple"
      ;;
    *) return 1 ;;
  esac
}

file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk 'NR==1 { print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk 'NR==1 { print $1 }'
  else
    die "sha256sum is not installed"
  fi
}

verify_sha256() {
  file=$1
  sums=$2
  name=${file##*/}
  hash=$(file_sha256 "$file")
  want=$(awk -v name="$name" '
    $2 == name { print $1; found++ }
    END { if (found != 1) exit 1 }
  ' "$sums") || return 1
  case "$want" in
    *[!0-9a-f]*) return 1 ;;
  esac
  [ "${#want}" -eq 64 ] || return 1
  [ "$hash" = "$want" ]
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  elif command -v doas >/dev/null 2>&1; then
    doas "$@"
  else
    die "run this as root"
  fi
}

release_version() {
  if [ -n "${SOCLI_VERSION:-}" ]; then
    printf '%s\n' "${SOCLI_VERSION#v}"
    return
  fi
  command -v curl >/dev/null 2>&1 || die "curl is not installed"
  json=$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest") || die "could not read the latest release"
  tag=$(printf '%s\n' "$json" | awk -F\" '/"tag_name"/ { print $4; exit }')
  [ -n "$tag" ] || die "the latest release has no tag"
  printf '%s\n' "${tag#v}"
}

main() {
  if [ "${SOCLI_INSTALL_CHECK:-}" = 1 ]; then
    [ "$#" -eq 2 ] || die "checksum check needs a file and SHA256SUMS"
    verify_sha256 "$1" "$2" || die "checksum did not match"
    printf '%s\n' ok
    exit 0
  fi

  kernel=${SOCLI_KERNEL:-$(uname -s)}
  case "$kernel" in
    Linux) ;;
    Darwin) die "this installer is for Linux. On macOS run: brew tap massimodeluisa/tap && brew install socli" ;;
    *) die "this installer is for Linux" ;;
  esac

  if [ -n "${SOCLI_ID+x}" ]; then
    id=$SOCLI_ID
    like=${SOCLI_ID_LIKE-}
  elif [ -r /etc/os-release ]; then
    # The file is key=value assignments from the distribution.
    # shellcheck disable=SC1091
    . /etc/os-release
    id=${ID-}
    like=${ID_LIKE-}
  else
    id=
    like=
  fi

  family=$(family_for "$id" "$like")
  machine=${SOCLI_ARCH:-$(uname -m)}
  cpu=$(cpu_for "$machine") || die "unsupported architecture: $machine"
  version=$(release_version)
  version_ok "$version" || die "refusing version: $version"
  asset=$(asset_for "$family" "$cpu" "$version") || die "no package for $family $cpu"

  if [ "${SOCLI_INSTALL_PRINT:-}" = 1 ]; then
    printf 'version=%s\nfamily=%s\nasset=%s\n' "$version" "$family" "$asset"
    exit 0
  fi

  command -v curl >/dev/null 2>&1 || die "curl is not installed"
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT INT TERM
  base="https://github.com/${repo}/releases/download/v${version}"
  printf 'Downloading socli %s (%s)\n' "$version" "$asset"
  curl -fsSL "$base/SHA256SUMS" -o "$work/SHA256SUMS" || die "could not download SHA256SUMS"
  curl -fsSL "$base/$asset" -o "$work/$asset" || die "could not download $asset"
  verify_sha256 "$work/$asset" "$work/SHA256SUMS" || die "checksum did not match for $asset"

  case "$family" in
    deb)
      as_root dpkg -i "$work/$asset"
      bin=/usr/bin/socli
      ;;
    rpm)
      if command -v dnf >/dev/null 2>&1; then
        as_root dnf install -y "$work/$asset"
      elif command -v zypper >/dev/null 2>&1; then
        as_root zypper install -y "$work/$asset"
      else
        as_root rpm -Uvh "$work/$asset"
      fi
      bin=/usr/bin/socli
      ;;
    apk)
      printf '%s\n' "The Alpine package is not signed. The checksum matched, so apk will install this file."
      as_root apk add --allow-untrusted "$work/$asset"
      bin=/usr/bin/socli
      ;;
    tar)
      printf '%s\n' "This distribution has no socli package. Installing the release binary into /usr/local/bin."
      triple=$(triple_for "$cpu") || die "no binary for $cpu"
      tar -xzf "$work/$asset" -C "$work"
      unpacked=$work/socli-${version}-${triple}/socli
      [ -f "$unpacked" ] || die "the archive did not contain socli"
      as_root mkdir -p /usr/local/bin
      as_root cp "$unpacked" /usr/local/bin/socli
      as_root chmod 755 /usr/local/bin/socli
      bin=/usr/local/bin/socli
      ;;
  esac

  printf '\n'
  printf 'socli %s is installed.\n' "$version"
  printf '\n'
  printf 'Open the reader:\n'
  printf '  socli\n'
  printf '\n'
  printf 'Try it before signing in:\n'
  printf '  socli --demo\n'
  case "$bin" in
    /usr/local/bin/socli)
      case ":${PATH:-}:" in
        *:/usr/local/bin:*) ;;
        *)
          printf '\n'
          printf 'socli is at /usr/local/bin/socli. Add that directory to PATH if the command is not found.\n'
          ;;
      esac
      ;;
  esac
}

main "$@"
