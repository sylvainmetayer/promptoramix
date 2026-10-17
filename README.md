# Promptoramix — IA du tournoi Linkx

[![CI](https://github.com/sylvainmetayer/promptoramix/actions/workflows/ci.yml/badge.svg)](https://github.com/sylvainmetayer/promptoramix/actions/workflows/ci.yml)
[![Qualité](https://github.com/sylvainmetayer/promptoramix/actions/workflows/quality.yml/badge.svg)](https://github.com/sylvainmetayer/promptoramix/actions/workflows/quality.yml)
[![Quality Gate](https://sonarcloud.io/api/project_badges/measure?project=sylvainmetayer_promptoramix&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=sylvainmetayer_promptoramix)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=sylvainmetayer_promptoramix&metric=coverage)](https://sonarcloud.io/summary/new_code?id=sylvainmetayer_promptoramix)
[![Maintainability](https://sonarcloud.io/api/project_badges/measure?project=sylvainmetayer_promptoramix&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=sylvainmetayer_promptoramix)
[![Reliability](https://sonarcloud.io/api/project_badges/measure?project=sylvainmetayer_promptoramix&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=sylvainmetayer_promptoramix)
[![Security](https://sonarcloud.io/api/project_badges/measure?project=sylvainmetayer_promptoramix&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=sylvainmetayer_promptoramix)

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![Image](https://img.shields.io/badge/ghcr.io-promptoramix-blue?logo=docker)](https://github.com/sylvainmetayer/promptoramix/pkgs/container/promptoramix)
[![Linkx](https://img.shields.io/badge/tournoi-Linkx-8a2be2)](https://marmelab.com/linkx/#/classement)

Le druide qui fait sortir la potion magique d'un simple prompt.

IA pour le [tournoi Linkx](https://marmelab.com/linkx/) de Marmelab, écrite en Rust from scratch :
règles et notation réimplémentées, recherche alpha-bêta à approfondissement itératif.
Protocole : [`docs/protocole-ia.md`](https://github.com/marmelab/linkx/blob/main/docs/protocole-ia.md).

## Structure

| Fichier | Rôle |
| --- | --- |
| `src/pieces.rs` | formes, orientations distinctes, écriture canonique |
| `src/game.rs` | plateau en bitboard `u128`, chute, support intégral, connexion 8-voisins, passes, blocage |
| `src/notation.rs` | lecture / écriture de la notation, rejeu d'une partie |
| `src/eval.rs` | distance de connexion pondérée par la gravité, plus grande zone |
| `src/search.rs` | négamax PVS, table de transposition, killers, historique, limite de temps |
| `src/bin/linkx-server.rs` | service HTTP (toute route `POST`), signature HMAC facultative |
| `src/bin/linkx-cli.rs` | coup pour une notation (`--ms`, `--depth`) ; `--legal` pour le contrôle croisé |
| `duel/` | banc d'essai contre l'IA de la maison officielle (Node 24) |

## Commandes

```bash
cargo test --release                 # tests unitaires
cargo run --release --bin linkx-cli -- "4Lr35 3Ir15" --ms 2000
PORT=8080 LINKX_SECRET=… cargo run --release --bin linkx-server
curl -s localhost:8080/ -d '{"protocol":1,"game":"x","color":"blue","record":"","deadline_ms":6000}'
```

Variables du serveur : `PORT` (8080), `LINKX_SECRET` (si définie, toute signature invalide reçoit un 401 ;
absente, aucune vérification),
`LINKX_MARGIN_MS` (800 : marge réseau retirée de `deadline_ms`), `LINKX_THREADS` (1 : fils de
recherche par requête ; la plateforme envoie au plus deux requêtes simultanées), `LINKX_CONFIG`
(réglages `clé=valeur,…` de l'évaluation, pour les essais), `LINKX_GAMES` (3 : parties dont la
recherche est gardée d'un appel à l'autre, 0 pour repartir de zéro à chaque coup), `LINKX_PONDER`
(1 : réflexion pendant le tour adverse, 0 pour la couper), `LINKX_JOURNAL` (absent : chemin d'un
fichier où chaque appel est ajouté, horodaté, en plus de la sortie d'erreur).

## Banc de duel

```bash
git clone --depth 1 https://github.com/marmelab/linkx.git duel/linkx-official
cargo build --release
cd duel
node crosscheck.ts 400          # nos règles contre l'arbitre officiel, position par position
node duel.ts --games 34 --openings --ours-ms 4500 --house-ms 900
```

L'IA de la maison du tournoi cherche 900 ms par coup (`searchQueue.ts`) ; le banc la reproduit.

## Journal et livre d'ouverture

`src/book.rs` est généré : pour chaque position de `examples/book_positions.txt`, une recherche
longue fixe le coup, retrouvé en jeu par son empreinte Zobrist. Entre deux rondes du tournoi, les
parties réelles enrichissent le livre : le serveur consigne chaque appel (`LINKX_JOURNAL`, un volume
sur flip ; à défaut, `docker logs`), `journal` en tire les positions de nos trois premiers coups que
le livre ne couvre pas, et `make_book` les analyse.

```bash
cd ~/Documents/homelab/ansible && mise exec -- ansible flip -b -m fetch \
  -a "src=/opt/apps/promptoramix/journal/coups.log dest=/tmp/coups.log flat=true"
cargo run --release --example journal -- 5 < /tmp/coups.log >> examples/book_positions.txt
sort -u -o examples/book_positions.txt examples/book_positions.txt
cargo run --release --example make_book -- 240 8 examples/book_positions.txt
cargo test --release   # vérifie que chaque entrée du livre est légale
```

Hors tournoi, un sparring d'une autre famille que la maison joue le même rôle : `mcts_duel` (UCT,
simulations guidées par l'évaluation) gagne rarement, mais quand il gagne c'est souvent par une
réplique que la maison ne joue jamais. `blunders` rejoue une partie et marque « ! » les chutes de
score vues par une recherche longue ; les positions ainsi relevées dans nos trois premiers coups
rejoignent `examples/book_positions.txt`.

```bash
cargo run --release --example mcts_duel -- --ms 300 --mcts-ms 1000 --threads 6 --rounds 3
cargo run --release --example blunders -- "<notation d'une partie perdue>" white 3000
```

## Intégration continue

- `ci.yml` : `cargo fmt`, `clippy -D warnings`, tests ; puis image `ghcr.io/sylvainmetayer/promptoramix`
  contrôlée (`/health` puis un coup) et, sur `main`, poussée avec les étiquettes `main` et `sha-…`.
- `quality.yml` : couverture `cargo-llvm-cov` et rapport Clippy envoyés à SonarQube Cloud
  (secret `SONAR_TOKEN`, analyse automatique désactivée côté SonarCloud).
- Renovate tient les actions et les crates à jour.

## Déploiement

En ligne sur <https://promptoramix.sylvain.cloud>.
