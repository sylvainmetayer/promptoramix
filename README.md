# linkx-bot

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

Variables du serveur : `PORT` (8080), `LINKX_SECRET` (si définie, signature vérifiée, sinon 401),
`LINKX_MARGIN_MS` (1500 : marge réseau retirée de `deadline_ms`), `LINKX_THREADS` (1 : fils de
recherche par requête ; la plateforme envoie au plus deux requêtes simultanées), `LINKX_CONFIG`
(réglages `clé=valeur,…` de l'évaluation, pour les essais).

## Banc de duel

```bash
git clone --depth 1 https://github.com/marmelab/linkx.git duel/linkx-official
cargo build --release
cd duel
node crosscheck.ts 400          # nos règles contre l'arbitre officiel, position par position
node duel.ts --games 34 --openings --ours-ms 4500 --house-ms 900
```

L'IA de la maison du tournoi cherche 900 ms par coup (`searchQueue.ts`) ; le banc la reproduit.

## Déploiement

`docker build -t linkx-bot . && docker run -p 8080:8080 -e LINKX_SECRET=… linkx-bot`,
derrière un HTTPS public sur le port 443 (exigence de la plateforme). Un cœur par requête,
deux requêtes simultanées au plus : prévoir 2 vCPU.
