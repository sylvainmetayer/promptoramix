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
const BIN = new URL('../target/release/linkx-cli', import.meta.url).pathname
const startIndex = process.argv.indexOf('--start')
const starts = startIndex >= 0 ? [process.argv[startIndex + 1]] : process.argv.includes('--openings') ? ['', ...OPENINGS.map((o) => o.notation)] : ['']
const ONLY = process.argv.includes('--blue-only') ? 'blue' : process.argv.includes('--white-only') ? 'white' : null

let score = { ours: 0, house: 0, draw: 0, fault: 0 }
const shardArg = process.argv.indexOf('--shard')
const [SHARD, SHARDS] = shardArg >= 0 ? process.argv[shardArg + 1].split('/').map(Number) : [0, 1]
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
        move = execFileSync(BIN, [record, '--ms', String(OURS_MS)], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
      } catch (e) {
        fault = `crash: ${e}`
        break
      }
      if (Date.now() - t > 6000) { fault = `timeout ${Date.now() - t} ms`; break }
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
console.log(`\nBilan : nous ${score.ours} · maison ${score.house} · nuls ${score.draw} · fautes ${score.fault}`)
