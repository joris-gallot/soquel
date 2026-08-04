#!/usr/bin/env bash
# Launches the app on a throwaway data dir carrying the market connection
# profiles, for landing screenshots. Needs `pnpm db:market` first.
#
# The data dir is rebuilt on every run: a profile added during a shoot is gone
# on the next one, which is what keeps a retaken screenshot identical. Open
# tabs, editor contents and query history live in the webview's localStorage,
# not here, so those are set by hand each time.
set -euo pipefail

cd "$(dirname "$0")/.."

hosts=(
  db.northwind.internal
  db-staging.northwind.internal
  mysql.northwind.internal
  cache.northwind.internal
  mongo.northwind.internal
)
missing=()
for host in "${hosts[@]}"; do
  node -e 'require("node:dns").lookup(process.argv[1], err => process.exit(err ? 1 : 0))' "$host" 2> /dev/null ||
    missing+=("$host")
done
if [ ${#missing[@]} -gt 0 ]; then
  cat >&2 << MSG
${#missing[@]} market hostname(s) do not resolve. They exist so the app shows an
endpoint that reads as a real deployment rather than localhost; .internal is
reserved for exactly this. Add one line to /etc/hosts:

  127.0.0.1 ${hosts[*]}
MSG
  exit 1
fi

data_dir="$PWD/.market-data"
rm -rf "$data_dir"
mkdir -p "$data_dir"
cp scripts/market-data/connections.json scripts/market-data/tunnels.json "$data_dir/"

# Written here rather than committed: these are the dev containers' own
# passwords, and a secrets.json sitting in the repo invites the wrong reading.
cat > "$data_dir/secrets.json" << 'JSON'
{
  "connection:mkt-prod-pg": "api",
  "connection:mkt-staging-pg": "api",
  "connection:mkt-billing-mysql": "api",
  "connection:mkt-cache-redis": "soquel",
  "connection:mkt-events-mongo": "api"
}
JSON

SOQUEL_DATA_DIR="$data_dir" SOQUEL_INSECURE_FILE_SECRETS=1 pnpm tauri dev
