#!/usr/bin/env bash
# roseline remote installer
# usage: curl -fsSL https://buwryy.net/api/roseline/setup-script | bash

set -euo pipefail

REPO="buwryme/Roseline"
FLATPAK_ID="net.buwryy.Roseline"
RELEASES_URL="${ROSELINE_RELEASES_URL:-https://api.github.com/repos/$REPO/releases?per_page=100}"

METHOD=""
WANT_VERSION=""
LATEST=0
ASSUME_YES=0
DRY_RUN=0
SYSTEM_KIND=""
ARCH=""
DEB_ARCH=""
ASSET_GLOB=""
SELECTED_TAG=""
SELECTED_ASSET=""
SELECTED_URL=""
TMP_DIR=""
SUDO_PID=""
TTY_SAVED=""

if [[ -t 1 ]]; then
    B='\033[1m'; BLUE='\033[34m'; GREEN='\033[32m'; YELLOW='\033[33m'; RED='\033[31m'; DIM='\033[2m'; NC='\033[0m'
    SPIN_FRAMES=("⠋" "⠙" "⠹" "⠸" "⠼" "⠴" "⠦" "⠧" "⠇" "⠏")
    NAV_HINT=$'\uf0d7\uf0d8'
else
    B=''; BLUE=''; GREEN=''; YELLOW=''; RED=''; DIM=''; NC=''
    SPIN_FRAMES=()
    NAV_HINT=''
fi

spin_pid=""
start_spin() {
    [[ ${#SPIN_FRAMES[@]} -gt 0 ]] || return 0
    local msg="$1"
    (
        local i=0
        while true; do
            printf "\r  ${BLUE}${SPIN_FRAMES[$((i % ${#SPIN_FRAMES[@]}))]}${NC} %s  " "$msg"
            ((i += 1))
            sleep 0.12
        done
    ) &
    spin_pid=$!
    disown "$spin_pid" 2>/dev/null || true
}

stop_spin() {
    if [[ -n "$spin_pid" ]]; then
        kill "$spin_pid" 2>/dev/null || true
        wait "$spin_pid" 2>/dev/null || true
        spin_pid=""
        [[ ! -t 1 ]] || printf '\r\033[K'
    fi
}

say() { stop_spin; printf "${BLUE}  →${NC} %s\n" "$*"; }
ok() { stop_spin; printf "${GREEN}  ✓${NC} %s\n" "$*"; }
warn() { stop_spin; printf "${YELLOW}  !${NC} %s\n" "$*"; }
die() { stop_spin; printf "${RED}  ✗${NC} %s\n" "$*" >&2; exit 1; }
section() { stop_spin; printf "\n${BLUE}${B}%s${NC}\n" "$*"; }
finish() { stop_spin; printf "\n${BLUE}${B}%s${NC}\n" "$*"; }
need_cmd() { command -v "$1" >/dev/null 2>&1 || die "missing $1 — install it first"; }

run_progress() {
    local label="$1" log status
    shift
    log="$TMP_DIR/${label//[^a-zA-Z0-9_-]/_}.log"
    start_spin "$label..."
    if "$@" >"$log" 2>&1; then
        ok "$label"
    else
        status=$?
        stop_spin
        cat "$log" >&2
        die "$label failed (exit $status)"
    fi
}

save_tty() {
    if [[ -t 0 ]]; then
        TTY_SAVED=$(stty -g 2>/dev/null || true)
    elif { exec 3</dev/tty; } 2>/dev/null && [[ -t 3 ]]; then
        TTY_SAVED=$(stty -g <&3 2>/dev/null || true)
        exec 3<&-
    fi
}

restore_tty() {
    [[ -n "$TTY_SAVED" ]] || return 0
    if [[ -t 0 ]]; then
        stty "$TTY_SAVED" 2>/dev/null || true
    elif { exec 3</dev/tty; } 2>/dev/null && [[ -t 3 ]]; then
        stty "$TTY_SAVED" <&3 2>/dev/null || true
        exec 3<&-
    fi
}

start_sudo() {
    [[ -z "$SUDO_PID" ]] || return 0
    ( while true; do sudo -n true 2>/dev/null || exit; sleep 50; done ) &
    SUDO_PID=$!
    disown "$SUDO_PID" 2>/dev/null || true
}

stop_sudo() {
    if [[ -n "$SUDO_PID" ]]; then
        kill "$SUDO_PID" 2>/dev/null || true
        wait "$SUDO_PID" 2>/dev/null || true
        SUDO_PID=""
    fi
}

cleanup() {
    local status=$?
    trap - EXIT HUP INT TERM
    restore_tty
    stop_sudo
    if [[ -n "${TMP_DIR:-}" && -d "$TMP_DIR" ]]; then
        rm -rf -- "$TMP_DIR"
    fi
    stop_spin
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

usage() {
    cat <<'EOF'
usage:
  curl -fsSL https://buwryy.net/api/roseline/setup-script | bash
  curl -fsSL https://buwryy.net/api/roseline/setup-script | bash -s -- --system --version v0.1.2 --yes
  curl -fsSL https://buwryy.net/api/roseline/setup-script | bash -s -- --flatpak --latest --yes

options:
  --system, --flatpak   skip the install-method prompt
  --version <tag>       pick an exact release (e.g. v0.1.2)
  --latest              skip the version prompt and take the recommended release
  --yes, -y             skip the confirmation prompt
  --dry-run             print what would be downloaded and installed
EOF
}

while (($#)); do
    case "$1" in
        --system) METHOD=system; shift ;;
        --flatpak) METHOD=flatpak; shift ;;
        --version)
            (($# >= 2)) || die "--version needs a value"
            WANT_VERSION="$2"
            shift 2
            ;;
        --latest) LATEST=1; shift ;;
        --yes | -y) ASSUME_YES=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done
[[ -z "$WANT_VERSION" || "$LATEST" -eq 0 ]] || die "--version and --latest cannot be combined"

# arrow-key picker; returns the chosen index in SELECTED
select_menu() {
    local prompt="$1"
    shift
    local -a labels=("$@")
    local count=${#labels[@]}
    local cursor=0 drawn=0 index key rest read_fd=0
    SELECTED=0
    ((count > 0)) || return 0

    if [[ ! -t 0 ]]; then
        if exec 3</dev/tty 2>/dev/null && [[ -t 3 ]]; then
            read_fd=3
        else
            die "this step needs a terminal; use --system or --flatpak plus --version or --latest, and --yes"
        fi
    fi
    need_cmd stty
    stty -icanon min 1 time 0 -echo <&"$read_fd" 2>/dev/null || true
    printf '\n'

    while true; do
        if ((drawn > 0)); then
            printf '\033[%dA' "$drawn"
        fi
        printf "\r\033[K${B}%s${NC} ${DIM}%s${NC}\n" "$prompt" "$NAV_HINT"
        for ((index = 0; index < count; index++)); do
            if ((index == cursor)); then
                printf "\r\033[K  ${BLUE}${B}>${NC} %b\n" "${labels[index]}"
            else
                printf "\r\033[K    %b\n" "${labels[index]}"
            fi
        done
        drawn=$((count + 1))

        key=""
        IFS= read -rsn1 -u "$read_fd" key || true
        [[ -n "$key" ]] || break
        if [[ "$key" == $'\e' || "$key" == $'\033' ]]; then
            rest=""
            IFS= read -rsn2 -u "$read_fd" rest || true
            case "$rest" in
                '[A' | 'OA') cursor=$(((cursor - 1 + count) % count)) ;;
                '[B' | 'OB') cursor=$(((cursor + 1) % count)) ;;
            esac
            continue
        fi
        case "$key" in
            $'\r' | $'\n') break ;;
            $'\003' | $'\004') die "aborted" ;;
        esac
    done

    restore_tty
    if ((read_fd == 3)); then exec 3<&-; fi

    printf '\033[%dA' "$drawn"
    for ((index = 0; index < drawn; index++)); do
        printf '\r\033[K\n'
    done
    printf '\033[%dA' "$drawn"
    printf "  ${GREEN}✓${NC} %b\n" "${labels[$cursor]}"
    SELECTED=$cursor
}

confirm() {
    ((ASSUME_YES)) && return 0
    local fd=0
    if [[ ! -t 0 ]]; then
        if exec 3</dev/tty 2>/dev/null && [[ -t 3 ]]; then
            fd=3
        else
            die "confirmation needs a terminal; pass --yes for unattended installs"
        fi
    fi
    IFS= read -r -u "$fd" _ || die "aborted"
    if ((fd == 3)); then exec 3<&-; fi
}

detect_arch() {
    local machine
    machine="$(uname -m)"
    case "$machine" in
        x86_64 | amd64) ARCH=x86_64; DEB_ARCH=amd64 ;;
        aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
        *) die "roseline runs on x86_64 and aarch64 only (this machine is $machine)" ;;
    esac
}

detect_system() {
    local id like
    id=""
    like=""
    if [[ -r /etc/os-release ]]; then
        id="$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"')"
        like="$(sed -n 's/^ID_LIKE=//p' /etc/os-release | tr -d '"')"
    fi
    case " $id $like " in
        *" arch "*) SYSTEM_KIND=arch ;;
        *" debian "* | *" ubuntu "*) SYSTEM_KIND=deb ;;
        *" fedora "* | *" rhel "* | *" centos "*) SYSTEM_KIND=rpm ;;
        *)
            if command -v pacman >/dev/null 2>&1; then
                SYSTEM_KIND=arch
            elif command -v apt-get >/dev/null 2>&1; then
                SYSTEM_KIND=deb
            elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; then
                SYSTEM_KIND=rpm
            else
                die "system-wide installs are available for Fedora/RHEL, Debian/Ubuntu and Arch; use --flatpak instead"
            fi
            ;;
    esac
    case "$SYSTEM_KIND" in
        rpm) ASSET_GLOB="*.$ARCH.rpm" ;;
        deb) ASSET_GLOB="*_$DEB_ARCH.deb" ;;
        arch) ASSET_GLOB="*-$ARCH.pkg.tar.zst" ;;
    esac
}

fetch_releases() {
    local http
    start_spin "fetching releases..."
    http=$(curl -sSL -w '%{http_code}' \
        -H 'Accept: application/vnd.github+json' \
        -H 'X-GitHub-Api-Version: 2022-11-28' \
        "${AUTH_ARGS[@]}" \
        "$RELEASES_URL" -o "$TMP_DIR/releases.json" || true)
    stop_spin
    case "$http" in
        200) ;;
        403) die "GitHub API rate limit hit (HTTP 403); set GH_TOKEN or wait and retry" ;;
        404) die "no roseline releases were found" ;;
        *) die "couldn't fetch releases (HTTP ${http:-no response})" ;;
    esac
    jq -e 'type == "array" and length > 0' "$TMP_DIR/releases.json" >/dev/null 2>&1 ||
        die "no roseline releases are published yet"
}

REL_TAG=() REL_BASE=() REL_CAT=() REL_HOTFIX=() REL_DATE=() REL_ASSET=() REL_URL=()
declare -A BEST_IDX=()
ORDERED_BASES=() VERSION_LABELS=() STABLE_BASES=() PREFIX_BASES=()

load_releases() {
    local tag date name url base suffix category hotfix
    local -A seen=()
    REL_TAG=() REL_BASE=() REL_CAT=() REL_HOTFIX=() REL_DATE=() REL_ASSET=() REL_URL=()
    while IFS=$'\t' read -r tag date name url; do
        [[ -n "$tag" && -n "$name" ]] || continue
        # shellcheck disable=SC2254
        case "$name" in
            $ASSET_GLOB) ;;
            *) continue ;;
        esac
        [[ -z "${seen[$tag]:-}" ]] || continue
        [[ "$tag" =~ ^v?([0-9]+(\.[0-9]+)+)(-(.+))?$ ]] || continue
        base="${BASH_REMATCH[1]}"
        suffix="${BASH_REMATCH[4]:-}"
        category=0
        hotfix=0
        if [[ -z "$suffix" ]]; then
            category=1
        elif [[ "$suffix" =~ ^(hotfix|hf)([.-]?([0-9]+))?$ ]]; then
            category=2
            hotfix=$((10#${BASH_REMATCH[3]:-1}))
        fi
        seen[$tag]=1
        REL_TAG+=("$tag")
        REL_BASE+=("$base")
        REL_CAT+=("$category")
        REL_HOTFIX+=("$hotfix")
        REL_DATE+=("$date")
        REL_ASSET+=("$name")
        REL_URL+=("$url")
    done < <(jq -r '
        .[] | select(.draft | not) as $r
        | $r.assets[]?
        | [$r.tag_name, ($r.published_at // $r.created_at // ""), .name, .browser_download_url]
        | @tsv
    ' "$TMP_DIR/releases.json")
    if ((${#REL_TAG[@]} == 0)); then
        if [[ "$METHOD" == "system" ]]; then
            die "no native $ARCH packages for $SYSTEM_KIND were found in the releases; try --flatpak"
        fi
        die "no $ARCH Flatpak packages were found in the releases"
    fi
}

choose_releases() {
    local i base category hotfix date
    local -A best_cat=() best_hot=() best_date=() best_idx=()
    for i in "${!REL_TAG[@]}"; do
        base="${REL_BASE[$i]}"
        category="${REL_CAT[$i]}"
        hotfix="${REL_HOTFIX[$i]}"
        date="${REL_DATE[$i]}"
        if [[ -z "${best_idx[$base]:-}" ]]; then
            best_cat[$base]=$category
            best_hot[$base]=$hotfix
            best_date[$base]=$date
            best_idx[$base]=$i
            continue
        fi
        if ((category > best_cat[$base])) ||
            ((category == best_cat[$base] && hotfix > best_hot[$base])); then
            best_cat[$base]=$category
            best_hot[$base]=$hotfix
            best_date[$base]=$date
            best_idx[$base]=$i
        elif ((category == best_cat[$base] && hotfix == best_hot[$base])) &&
            [[ "$date" > "${best_date[$base]}" ]]; then
            best_date[$base]=$date
            best_idx[$base]=$i
        fi
    done
    BEST_IDX=()
    for base in "${!best_idx[@]}"; do
        BEST_IDX[$base]="${best_idx[$base]}"
    done
}

build_versions() {
    local base index idx label
    STABLE_BASES=()
    PREFIX_BASES=()
    for base in "${!BEST_IDX[@]}"; do
        idx="${BEST_IDX[$base]}"
        if ((REL_CAT[idx] >= 1)); then
            STABLE_BASES+=("$base")
        else
            PREFIX_BASES+=("$base")
        fi
    done
    if ((${#STABLE_BASES[@]})); then
        mapfile -t STABLE_BASES < <(printf '%s\n' "${STABLE_BASES[@]}" | sort -Vr)
    fi
    if ((${#PREFIX_BASES[@]})); then
        mapfile -t PREFIX_BASES < <(printf '%s\n' "${PREFIX_BASES[@]}" | sort -Vr)
    fi
    ORDERED_BASES=()
    if ((${#STABLE_BASES[@]})); then ORDERED_BASES+=("${STABLE_BASES[@]}"); fi
    if ((${#PREFIX_BASES[@]})); then ORDERED_BASES+=("${PREFIX_BASES[@]}"); fi
    ((${#ORDERED_BASES[@]})) || die "no installable versions were found"

    VERSION_LABELS=()
    for index in "${!ORDERED_BASES[@]}"; do
        base="${ORDERED_BASES[$index]}"
        idx="${BEST_IDX[$base]}"
        label="${REL_TAG[$idx]}"
        if ((REL_CAT[idx] == 0)); then
            label="${YELLOW}${label} (pre-release)${NC}"
        elif ((index == 0)); then
            label="${GREEN}${label} (recommended)${NC}"
        fi
        VERSION_LABELS+=("$label")
    done
}

choose_version() {
    local want want_base i base idx
    if [[ -n "$WANT_VERSION" ]]; then
        want="${WANT_VERSION#v}"
        for i in "${!REL_TAG[@]}"; do
            if [[ "${REL_TAG[$i]#v}" == "$want" ]]; then
                SELECTED_TAG="${REL_TAG[$i]}"
                SELECTED_ASSET="${REL_ASSET[$i]}"
                SELECTED_URL="${REL_URL[$i]}"
                return 0
            fi
        done
        want_base="${want%%-*}"
        for base in "${ORDERED_BASES[@]}"; do
            if [[ "$base" == "$want_base" ]]; then
                idx="${BEST_IDX[$base]}"
                SELECTED_TAG="${REL_TAG[$idx]}"
                SELECTED_ASSET="${REL_ASSET[$idx]}"
                SELECTED_URL="${REL_URL[$idx]}"
                return 0
            fi
        done
        die "version $WANT_VERSION is not available for $METHOD on $ARCH"
    fi
    if ((LATEST)); then
        base="${ORDERED_BASES[0]}"
        idx="${BEST_IDX[$base]}"
    else
        select_menu "choose a version" "${VERSION_LABELS[@]}"
        base="${ORDERED_BASES[$SELECTED]}"
        idx="${BEST_IDX[$base]}"
    fi
    SELECTED_TAG="${REL_TAG[$idx]}"
    SELECTED_ASSET="${REL_ASSET[$idx]}"
    SELECTED_URL="${REL_URL[$idx]}"
}

validate_asset() {
    case "$SYSTEM_KIND" in
        rpm)
            command -v rpm >/dev/null 2>&1 || return 0
            rpm -qp "$ASSET_PATH" >/dev/null 2>&1 || die "the downloaded file is not a valid RPM"
            ;;
        deb)
            command -v dpkg-deb >/dev/null 2>&1 || return 0
            dpkg-deb --info "$ASSET_PATH" >/dev/null 2>&1 || die "the downloaded file is not a valid deb"
            ;;
        *) ;;
    esac
}

TMP_DIR=$(mktemp -d)
save_tty

need_cmd curl
need_cmd jq
need_cmd uname
need_cmd sed
need_cmd sort
need_cmd tr
need_cmd grep
need_cmd awk
need_cmd mktemp

AUTH_ARGS=()
if [[ -n "${GITHUB_TOKEN:-${GH_TOKEN:-}}" ]]; then
    AUTH_ARGS=(-H "Authorization: Bearer ${GITHUB_TOKEN:-$GH_TOKEN}")
fi

section "roseline setup"
say "install the macOS Roblox client on Linux"
detect_arch

if [[ -z "$METHOD" ]]; then
    select_menu "would you like to install roseline system-wide, or via flatpak?" "system-wide" "flatpak"
    if ((SELECTED == 0)); then
        METHOD=system
    else
        METHOD=flatpak
    fi
fi

if [[ "$METHOD" == "system" ]]; then
    detect_system
else
    ASSET_GLOB="*-$ARCH.flatpak"
    need_cmd flatpak
fi

fetch_releases
load_releases
choose_releases
build_versions
choose_version

section "confirm"
printf "  version: %s\n" "$SELECTED_TAG"
printf "  package to fetch & install: %s\n" "$SELECTED_ASSET"
printf '  %bpress enter to confirm%b\n' "$DIM" "$NC"
confirm
ok "confirmed"

if [[ "$METHOD" == "system" ]]; then
    section "sudo"
    if ((DRY_RUN)); then
        say "dry run: sudo would be requested"
    elif ((EUID == 0)); then
        ok "running as root; no sudo needed"
    else
        say "sudo is needed to install the package"
        sudo -v || die "sudo was not granted"
        start_sudo
        ok "sudo ready"
    fi
else
    section "permissions"
    say "flatpak installs into your user account; no sudo needed"
    ok "ready"
fi

ASSET_PATH="$TMP_DIR/$SELECTED_ASSET"

if ((DRY_RUN)); then
    say "dry run: would download $SELECTED_URL"
else
    section "download"
    run_progress "downloading $SELECTED_ASSET" curl -fL --retry 3 --retry-delay 1 \
        -o "$ASSET_PATH" "$SELECTED_URL"
    [[ -s "$ASSET_PATH" ]] || die "the downloaded package is empty"
    validate_asset
fi

if [[ "$METHOD" == "system" ]]; then
    case "$SYSTEM_KIND" in
        rpm)
            if command -v dnf >/dev/null 2>&1; then
                INSTALL_CMD=(dnf install -y --nogpgcheck "$ASSET_PATH")
            elif command -v yum >/dev/null 2>&1; then
                INSTALL_CMD=(yum install -y --nogpgcheck "$ASSET_PATH")
            else
                die "dnf or yum is missing"
            fi
            ;;
        deb)
            INSTALL_CMD=(env DEBIAN_FRONTEND=noninteractive apt-get install -y "$ASSET_PATH")
            ;;
        arch)
            INSTALL_CMD=(pacman -U --noconfirm "$ASSET_PATH")
            ;;
    esac
    if ((EUID != 0)); then
        INSTALL_CMD=(sudo "${INSTALL_CMD[@]}")
    fi
    if ((DRY_RUN)); then
        say "dry run: would install with: ${INSTALL_CMD[*]}"
    else
        section "install"
        run_progress "installing roseline $SELECTED_TAG" "${INSTALL_CMD[@]}"
        ok "installed"
    fi
else
    if ! flatpak remotes --user 2>/dev/null | awk '{print $1}' | grep -qx flathub; then
        if ((DRY_RUN)); then
            say "dry run: would add the flathub remote"
        else
            run_progress "adding flathub" flatpak remote-add --user --if-not-exists \
                flathub https://flathub.org/repo/flathub.flatpakrepo
        fi
    fi
    FLATPAK_CMD=(flatpak install --user --noninteractive -y "$ASSET_PATH")
    if ((DRY_RUN)); then
        say "dry run: would install with: ${FLATPAK_CMD[*]}"
    else
        section "install"
        run_progress "installing roseline $SELECTED_TAG" "${FLATPAK_CMD[@]}"
        ok "installed"
    fi
fi

if ((DRY_RUN)); then
    finish "dry run complete"
    say "nothing was installed"
else
    finish "roseline is installed"
    if [[ "$METHOD" == "flatpak" ]]; then
        say "launch it with: flatpak run $FLATPAK_ID"
    else
        say "launch it from your app menu, or run: roseline"
    fi
fi
