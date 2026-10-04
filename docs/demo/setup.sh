#!/bin/sh
# Builds a made-up world for screenshots of Noon Commander, so that nothing of the real one
# shows: a home directory with a few projects, its own settings and history, an ssh_config with
# made-up hosts, and a fake ssh that serves each host from a local directory through the local
# sftp-server, with no network and no real ssh.
#
#   docs/demo/setup.sh [DIR]
#
# DIR (default /tmp/noc-demo) is removed and built again; it must be absent or made by this
# script. Keep its path short: the control sockets live in it, and their paths may have at most
# 104 bytes on macOS. Then DIR/bin/noc starts noc in that world. It runs NOC_BIN, by default the debug build
# of this checkout (`cargo build -p noc`).
#
# The local panel shows the real path of DIR, and the virtual root the name of this machine.
set -eu
# Times of the made-up files are written and shown in UTC, wherever the screenshots are taken.
TZ=UTC
export TZ

repo=$(cd "$(dirname "$0")/../.." && pwd -P)
dir=${1:-/tmp/noc-demo}
noc_bin=${NOC_BIN:-$repo/target/debug/noc}
fake_ssh=$repo/crates/noc-ssh/tests/support/fake-ssh
marker=.noc-demo

case $dir in
    /*) ;;
    *) dir=$(pwd -P)/$dir ;;
esac
if [ -e "$dir" ]; then
    if [ ! -e "$dir/$marker" ]; then
        echo "setup.sh: $dir exists and is not a demo directory; not touching it" >&2
        exit 1
    fi
    # Masters of an earlier run that did not quit: the fake ssh keeps its pid in the socket.
    for socket in "$dir"/run/noc/*; do
        [ -f "$socket" ] && kill "$(cat "$socket")" 2>/dev/null || true
    done
    chmod -R u+w "$dir"
    rm -rf "$dir"
fi
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
: >"$dir/$marker"

home=$dir/home
mkdir -p "$dir/bin" "$home/.config/noc" "$home/.local/share" "$home/.local/state" \
    "$home/.cache" "$dir/zoxide"
# The runtime directory holds the control sockets; noc wants its parent to exist and itself
# private.
mkdir -p "$dir/run"
chmod 700 "$dir/run"

# file PATH TIME [KIB]: a file of KIB KiB of zeros, or of the text on stdin, dated TIME
# ([[CC]YY]MMDDhhmm, as touch -t).
file() {
    mkdir -p "$(dirname "$1")"
    if [ $# -ge 3 ]; then
        dd if=/dev/zero of="$1" bs=1024 count="$3" 2>/dev/null
    else
        cat >"$1"
    fi
    touch -t "$2" "$1"
}

# --- Hosts: alias, user, address, port, label ---------------------------------------------------

hosts='prod-web deploy web1.example.com 22 Production
staging deploy staging.example.com 22 Staging
nas backup nas.home.arpa 2222 Backups'

{
    echo '# Made-up hosts for the Noon Commander demo; the fake ssh serves them.'
    echo "$hosts" | while read -r alias user address port _; do
        printf '\nHost %s\n    HostName %s\n    User %s\n' "$alias" "$address" "$user"
        [ "$port" = 22 ] || printf '    Port %s\n' "$port"
    done
} >"$dir/ssh_config"

{
    echo '# Made-up hosts for the Noon Commander demo.'
    echo "$hosts" | while read -r alias _ _ _ label; do
        printf '\n["%s"]\ntype = "sftp"\nlabel = "%s"\n' "$alias" "$label"
    done
} >"$home/.config/noc/hosts.toml"

cat >"$home/.config/noc/config.toml" <<EOF
# Settings of the Noon Commander demo, written by docs/demo/setup.sh.

[ssh]
program = "$dir/bin/ssh"
config_file = "$dir/ssh_config"

[volumes]
hide = ["/Volumes/*", "/System/Volumes/*"]

[ui]
# Its own colors, so that the terminal's palette does not change them.
theme = "noon-dark"
EOF

# The $ in single quotes is for the written script.
# shellcheck disable=SC2016
{
    echo '#!/bin/sh'
    echo '# Fake ssh of the Noon Commander demo: each host is served from its own directory.'
    echo 'dest= after='
    echo 'for arg in "$@"; do'
    echo '    if [ -n "$after" ]; then dest=$arg; break; fi'
    echo '    [ "$arg" = -- ] && after=1'
    echo 'done'
    echo 'case $dest in'
    echo "$hosts" | while read -r alias user address port _; do
        printf '    %s) FAKE_SSH_USER=%s FAKE_SSH_HOSTNAME=%s FAKE_SSH_PORT=%s ;;\n' \
            "$alias" "$user" "$address" "$port"
    done
    echo 'esac'
    printf 'FAKE_SSH_ROOT=%s/hosts/$dest\n' "$dir"
    echo 'export FAKE_SSH_USER FAKE_SSH_HOSTNAME FAKE_SSH_PORT FAKE_SSH_ROOT'
    printf 'exec %s "$@"\n' "$fake_ssh"
} >"$dir/bin/ssh"

cat >"$dir/bin/noc" <<EOF
#!/bin/sh
# Starts noc in the world of the Noon Commander demo, in the site project.
export HOME=$home
export XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share
export XDG_STATE_HOME=$home/.local/state XDG_CACHE_HOME=$home/.cache
export XDG_RUNTIME_DIR=$dir/run _ZO_DATA_DIR=$dir/zoxide
export COLORTERM=truecolor TZ=UTC
cd "\$HOME/projects/site"
exec $noc_bin "\$@"
EOF
chmod +x "$dir/bin/ssh" "$dir/bin/noc"

# --- The local home directory -------------------------------------------------------------------

site=$home/projects/site
mkdir -p "$home/Documents" "$home/Downloads" "$home/Pictures" "$home/projects/api" \
    "$home/projects/dotfiles"

file "$site/README.md" 202609281012 <<'EOF'
# Site

The public website: static pages built from Markdown, with a small search index.

## Build

    npm install
    npm run build

The pages land in `dist/`. `./deploy.sh prod-web` copies them to the server.

## Layout

- `content/`: the pages, one Markdown file each
- `assets/`: images, fonts, and styles
- `templates/`: the HTML around the pages
EOF
file "$site/package.json" 202609271830 <<'EOF'
{
  "name": "site",
  "version": "2.4.0",
  "private": true,
  "scripts": {
    "build": "node build.mjs",
    "serve": "node serve.mjs"
  }
}
EOF
file "$site/deploy.sh" 202609251144 <<'EOF'
#!/bin/sh
set -eu
npm run build
echo "Copy dist/ to ${1:?host} with Noon Commander: F5."
EOF
chmod +x "$site/deploy.sh"
file "$site/.gitignore" 202605140903 <<'EOF'
node_modules/
dist/
EOF
file "$site/build.mjs" 202609271829 6
file "$site/serve.mjs" 202607020915 2
file "$site/package-lock.json" 202609271830 184
file "$site/content/index.md" 202609300947 3
file "$site/content/about.md" 202608190731 2
file "$site/content/blog/2026-09-release.md" 202609291655 9
file "$site/assets/logo.svg" 202605140903 4
file "$site/assets/hero.jpg" 202609120840 812
file "$site/assets/fonts/inter.woff2" 202605140903 104
file "$site/assets/style.css" 202609281012 18
file "$site/templates/page.html" 202608190731 3
file "$site/dist/index.html" 202609301002 14
file "$site/dist/archive-2026-09.tar.gz" 202609301003 3072

file "$home/projects/api/Cargo.toml" 202609220918 1
file "$home/projects/api/src/main.rs" 202609220918 7
file "$home/projects/dotfiles/zshrc" 202604110820 3
file "$home/Documents/invoice-2026-09.pdf" 202610010915 96
file "$home/Documents/notes.md" 202609301740 2
file "$home/Downloads/ubuntu-24.04-server.iso" 202609150911 20480
file "$home/Pictures/screenshot.png" 202609261133 412

# --- The hosts ----------------------------------------------------------------------------------

web=$dir/hosts/prod-web
for release in 2026-09-24 2026-09-30; do
    file "$web/releases/$release/index.html" "$(echo "$release" | tr -d -)1002" 14
    file "$web/releases/$release/assets/style.css" 202609281012 18
done
ln -s releases/2026-09-30 "$web/current"
file "$web/shared/uploads/team.jpg" 202608030914 640
file "$web/logs/access.log" 202610031759 2210
file "$web/logs/error.log" 202610031244 37
file "$web/.profile" 202603020800 1

stage=$dir/hosts/staging
file "$stage/site/index.html" 202610021511 14
file "$stage/site/assets/style.css" 202610021511 19
file "$stage/notes.txt" 202610021520 <<'EOF'
Staging runs the next release. Copy it to prod-web after review.
EOF

nas=$dir/hosts/nas
file "$nas/backups/site-2026-09-30.tar.gz" 202609302300 4096
file "$nas/backups/site-2026-10-01.tar.gz" 202610012300 4100
file "$nas/backups/site-2026-10-02.tar.gz" 202610022300 4112
file "$nas/media/talk.mp4" 202607181930 8192

# Directories got the time of the run; give them fixed ones, so that screenshots do not change.
find "$home" "$dir/hosts" -type d -exec touch -t 202609301003 {} +
touch -t 202609291655 "$site/content" "$site/content/blog"
touch -t 202609281012 "$site/assets" "$site/templates"
touch -t 202605140903 "$home/.config" "$home/.local" "$home/.cache"
touch -h -t 202609301004 "$web/current"

echo "Demo ready: $dir/bin/noc"
