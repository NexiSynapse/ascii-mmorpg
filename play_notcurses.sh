#!/usr/bin/env bash
set -Eeuo pipefail

# Arch Linux launcher for the ASCII MMORPG Notcurses client.
PROJECT_DIR='/run/media/codiosity/backup/Personal/codes/Projects/ASCII-Game/ascii-mmorpg'
PACKAGE='ascii-mmorpg-client'
FEATURES='notcurses'

if [[ ! -d "$PROJECT_DIR" ]]; then
    echo "ERROR: project directory not found: $PROJECT_DIR" >&2
    echo "Check that the backup drive is mounted and the path is correct." >&2
    exit 1
fi

cd -- "$PROJECT_DIR"

if [[ ! -f Cargo.toml ]]; then
    echo "ERROR: Cargo.toml not found in $PROJECT_DIR" >&2
    echo "Make sure PROJECT_DIR points to the Rust workspace root." >&2
    exit 1
fi

EXE="$PROJECT_DIR/target/release/$PACKAGE"

echo '[1/2] Building Notcurses client and embedded assets...'
echo '(build output hidden; shown only if the build fails)'
if ! command -v cargo >/dev/null 2>&1; then
    echo 'ERROR: cargo not found. Install it with: sudo pacman -S --needed rust cargo' >&2
    exit 127
fi

build_log="$(cargo build --release -p "$PACKAGE" --features "$FEATURES" 2>&1)" || {
    status=$?
    printf '%s\n' "$build_log"
    echo >&2
    echo "Build failed with exit code $status." >&2
    exit 1
}

if [[ ! -x "$EXE" ]]; then
    echo "ERROR: executable not found at: $EXE" >&2
    echo "Expected a native Linux release build at target/release/$PACKAGE." >&2
    exit 1
fi

echo '[2/2] Launching ASCII MMORPG...'
status=0
"$EXE" --notcurses --fps uncapped "$@" || status=$?
echo "Game finished with code $status."
exit "$status"
