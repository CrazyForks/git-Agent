#!/usr/bin/env bash
# Install only the Git Agent virtual host; preserve all other Caddy sites.
set -euo pipefail

archive=${1:?Usage: install-site.sh /tmp/git-agent-site-ID.tar.gz ID}
release_id=${2:?Missing release ID}
[[ $(id -u) == 0 ]] || { echo "Run as root" >&2; exit 1; }
[[ $release_id =~ ^[0-9]{8}T[0-9]{6}Z$ ]] || { echo "Invalid release ID" >&2; exit 1; }
[[ $archive == "/tmp/git-agent-site-$release_id.tar.gz" && -f $archive ]] || exit 1

site_root=/srv/git-agent-site
release_dir="$site_root/releases/$release_id"
caddyfile=/etc/caddy/Caddyfile
fragment=/etc/caddy/sites/git-agent.caddy
[[ ! -e "$release_dir" ]] || { echo "Release already exists" >&2; exit 1; }
if tar -tzf "$archive" | grep -Eq '(^/|(^|/)\.\.(/|$))'; then
  echo "Unsafe archive path" >&2
  exit 1
fi
install -d -m 755 "$release_dir" /etc/caddy/sites
tar -xzf "$archive" -C "$release_dir" --no-same-owner
[[ -f "$release_dir/public/index.html" && -f "$release_dir/deploy/git-agent.caddy" ]] || exit 1
find "$release_dir/public" -type d -exec chmod 755 {} +
find "$release_dir/public" -type f -exec chmod 644 {} +
install -m 644 "$caddyfile" "$release_dir/Caddyfile.before"
install -m 644 "$caddyfile" "$release_dir/Caddyfile.next"
if [[ -f "$fragment" ]]; then
  install -m 644 "$fragment" "$release_dir/git-agent.caddy.before"
fi
previous_target=$(readlink "$site_root/current" || true)
if [[ -e "$site_root/current" && ! -L "$site_root/current" ]]; then
  echo "Current path is not a symlink; refusing to overwrite" >&2
  exit 1
fi
if ! grep -Fxq 'import /etc/caddy/sites/git-agent.caddy' "$release_dir/Caddyfile.next"; then
  printf '\nimport /etc/caddy/sites/git-agent.caddy\n' >> "$release_dir/Caddyfile.next"
fi

rollback() {
  echo "Deployment failed; restoring the previous Caddy configuration." >&2
  install -m 644 "$release_dir/Caddyfile.before" "$caddyfile"
  if [[ -f "$release_dir/git-agent.caddy.before" ]]; then
    install -m 644 "$release_dir/git-agent.caddy.before" "$fragment"
  fi
  if [[ -n "$previous_target" ]]; then
    ln -s "$previous_target" "$site_root/rollback-$release_id"
    mv -Tf "$site_root/rollback-$release_id" "$site_root/current"
  fi
  systemctl reload caddy || true
}
trap rollback ERR
install -m 644 "$release_dir/deploy/git-agent.caddy" "$fragment"
caddy validate --config "$release_dir/Caddyfile.next" --adapter caddyfile
ln -s "$release_dir/public" "$site_root/current-$release_id"
mv -Tf "$site_root/current-$release_id" "$site_root/current"
install -m 644 "$release_dir/Caddyfile.next" "$caddyfile"
systemctl reload caddy
systemctl is-active --quiet caddy
trap - ERR
printf 'Deployed: %s\nPrevious release: %s\nCaddy backup: %s\n' "$release_dir" "${previous_target:-none}" "$release_dir/Caddyfile.before"
