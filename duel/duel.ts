// Duel local : notre IA (binaire Rust) contre l'IA de la maison officielle.
// L'arbitre est le code officiel : chaque coup est rejoué par parseGameRecord.
//   node duel/duel.ts --games 8 --ours-ms 4500 --house-ms 900 [--openings]
import { execFileSync } from 'node:child_process'
import { chooseBotMove } from './linkx-official/supabase/functions/bot-linkx/botMove.ts'
import { parseGameRecord, serializeGameRecord } from './linkx-official/src/game/moveNotation.ts'
import { OPENINGS } from './linkx-official/supabase/functions/_shared/openings.ts'

const arg = (name: string, fallback: number) => {
  const i = process.argv.indexOf(name)
  return i >= 0 ? Number(process.argv[i + 1]) : fallback
}
const GAMES = arg('--games', 4)
const OURS_MS = arg('--ours-ms', 4500)
const HOUSE_MS = arg('--house-ms', 900)
const serverArg = process.argv.indexOf('--server')
const SERVER = serverArg >= 0 ? process.argv[serverArg + 1] : null
const BIN = process.env.LINKX_BIN || new URL('../target/release/linkx-cli', import.meta.url).pathname
const startIndex = process.argv.indexOf('--start')
const starts = startIndex >= 0 ? [process.argv[startIndex + 1]] : process.argv.includes('--openings') ? ['', ...OPENINGS.map((o) => o.notation)] : ['']
const ONLY = process.argv.includes('--blue-only') ? 'blue' : process.argv.includes('--white-only') ? 'white' : null

let score = { ours: 0, house: 0, draw: 0, fault: 0 }
const shardArg = process.argv.indexOf('--shard')
const [SHARD, SHARDS] = shardArg >= 0 ? process.argv[shardArg + 1].split('/').map(Number) : [0, 1]
// Notre coup le plus lent, vu du client : la plateforme coupe à 6 s.
let slowest = 0
for (let g = 0; g < GAMES; g++) {
  if (g % SHARDS !== SHARD) continue
  const start = starts[Math.floor(g / 2) % starts.length]
  const oursColor = ONLY ?? (g % 2 === 0 ? 'blue' : 'white')
  let record = start
  let fault = ''
  const t0 = Date.now()
  for (;;) {
    const parsed = parseGameRecord(record)
    if (!parsed.ok) throw new Error(`notation invalide ${record}`)
    const state = parsed.state
    if (state.phase === 'finished') {
      const r = state.result!
      const who = r.winner === null ? 'draw' : r.winner === oursColor ? 'ours' : 'house'
      score[who]++
      const zones = r.largestZones ? ` ${JSON.stringify(r.largestZones)}` : ''
      console.log(`#${g + 1} [${start || 'vide'}] nous=${oursColor} → ${who} (${r.reason}${zones}) ${((Date.now() - t0) / 1000).toFixed(0)} s\n   ${record}`)
      break
    }
    record = serializeGameRecord(state)
    let move: string
    if (state.activePlayer === oursColor) {
      const t = Date.now()
      try {
        if (SERVER) {
          // Comme la plateforme : un appel HTTP par coup, identifié par la partie.
          const res = await fetch(SERVER, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ protocol: 1, game: `duel-${process.pid}-${g}`, color: oursColor, record, deadline_ms: 6000 }),
          })
          if (res.status !== 200) throw new Error(`HTTP ${res.status}`)
          move = (await res.json()).move
        } else {
          move = execFileSync(BIN, [record, '--ms', String(OURS_MS)], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
        }
      } catch (e) {
        fault = `crash: ${e}`
        break
      }
      const dt = Date.now() - t
      if (dt > slowest) slowest = dt
      if (dt > 6000) { fault = `timeout ${dt} ms`; break }
    } else {
      const res = chooseBotMove(record, state.activePlayer, HOUSE_MS)
      if (!res.ok) throw new Error(JSON.stringify(res))
      move = res.move
    }
    const next = parseGameRecord(`${record} ${move}`.trim())
    if (!next.ok) {
      if (state.activePlayer === oursColor) { fault = `illégal ${move}: ${JSON.stringify(next.error)}`; break }
      throw new Error(`maison illégale ${move}`)
    }
    record = `${record} ${move}`.trim()
  }
  if (fault) {
    score.fault++
    score.house++
    console.log(`#${g + 1} FAUTE ${fault}\n   ${record}`)
  }
}
console.log(`\nBilan : nous ${score.ours} · maison ${score.house} · nuls ${score.draw} · fautes ${score.fault} · coup le plus lent ${slowest} ms`)
