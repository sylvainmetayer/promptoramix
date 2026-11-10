#!/usr/bin/env bash
# Routine d'après-vague (README, « Journal et livre d'ouverture ») : journal de
# production, positions nouvelles, corrections de `wave`, livre, contrôles.
# Prérequis : l'export « Mes parties » de la vague dans linkx-parties.json.
# HOMELAB : dépôt homelab (ansible), ~/Documents/homelab par défaut.
set -euo pipefail
cd "$(dirname "$0")"
homelab=${HOMELAB:-$HOME/Documents/homelab}
book=examples/book_positions.txt

current=""
trap 'echo "Échec : $current" >&2' ERR
etape() {
  current=$1
  echo "==> $current" >&2
}

if [[ ! -f linkx-parties.json ]]; then
  echo "linkx-parties.json absent : exporter « Mes parties » de la vague d'abord." >&2
  exit 1
fi

etape "journal de production (flip)"
(cd "$homelab/ansible" && mise exec -- ansible flip -b -m fetch \
  -a "src=/opt/apps/promptoramix/journal/coups.log dest=/tmp/coups.log flat=true")

etape "positions du journal absentes du livre"
cargo run --release --example journal -- 5 < /tmp/coups.log >> "$book"
sort -u -o "$book" "$book"

etape "corrections des parties perdues (wave)"
cargo run --release --example wave -- linkx-parties.json | tee -a "$book"

# Une position déjà présente fait échouer make_book : remplacer sa ligne à la main.
etape "livre d'ouverture (make_book --keep)"
cargo run --release --example make_book -- 240 8 "$book" --keep
cargo fmt

etape "victoires touchées par le livre (wave)"
wave_report=$(cargo run --release --example wave -- linkx-parties.json 2>&1 >/dev/null)
echo "$wave_report" >&2
if grep -q "victoire touchée" <<<"$wave_report"; then
  echo "Victoire(s) touchée(s) par le livre : à examiner avant de continuer (tests non lancés)." >&2
  exit 1
fi

etape "tests (légalité du livre)"
cargo test --release

echo "Terminé. Comparer chaque correction à la même position sans exclusion (make_book --out), voir README." >&2
