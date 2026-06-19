# Sourced by the install-*.sh scripts. Reads the pinned (version, asset, sha256)
# from ../tools.lock and verifies a download against it. Fail-closed on mismatch.
HP_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
LOCK="$HP_ROOT/tools.lock"

pin() {  # pin <tool> <field>  -> prints the value, or empty
  awk -v t="$1" -v f="$2" '
    $1==t { for (i=2;i<=NF;i++){ n=index($i,"="); if (n>0 && substr($i,1,n-1)==f) print substr($i,n+1) } }
  ' "$LOCK"
}

verify_sha() {  # verify_sha <file> <expected-hex>
  case "$2" in
    [0-9a-f]*) ;;  # a real hex digest
    *) echo "host-prove: no pinned sha256 for this artifact (got '$2') — refusing to install" >&2; exit 2 ;;
  esac
  got=$(sha256sum "$1" | cut -d' ' -f1)
  if [ "$got" != "$2" ]; then
    echo "host-prove: SHA256 MISMATCH for $1" >&2
    echo "  got  $got" >&2
    echo "  want $2"   >&2
    exit 2
  fi
  echo "host-prove: sha256 verified ($2)" >&2
}
