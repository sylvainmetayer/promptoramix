// Compare nos règles aux règles officielles sur des parties aléatoires.
//   node duel/crosscheck.ts [parties]
import { spawnSync } from 'node:child_process'
import { parseGameRecord, serializeGameRecord, serializeMove } from './linkx-official/src/game/moveNotation.ts'
import { enumerateLegalMoves } from './linkx-official/src/game/legalMoves.ts'

const GAMES = Number(process.argv[2] ?? 300)
const records: string[] = []
const expected: string[] = []
let seed = 12345
const rand = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 2 ** 32)

for (let g = 0; g < GAMES; g++) {
  let record = rand() < 0.5 ? 'w' : ''
  for (;;) {
    const parsed = parseGameRecord(record)
    if (!parsed.ok) throw new Error(record)
    const s = parsed.state
    record = serializeGameRecord(s)
    records.push(record)
    if (s.phase === 'finished') {
      expected.push(`finished ${s.result!.winner === null ? 'None' : `Some(${s.result!.winner === 'blue' ? 0 : 1})`}`)
      break
    }
    const legal = enumerateLegalMoves(s.board, s.inventories[s.activePlayer])
    const tokens = [...new Set(legal.map((m) => serializeMove({ shapeId: m.shapeId, rotation: m.orientation.rotation, flipped: m.orientation.flipped, column: m.column })))].sort()
    expected.push(`${s.activePlayer} ${tokens.join(' ')}`)
    record = `${record} ${tokens[Math.floor(rand() * tokens.length)]}`.trim()
  }
}
const bin = new URL('../target/release/linkx-cli', import.meta.url).pathname
const out = spawnSync(bin, ['--legal'], { input: records.join('\n') + '\n', encoding: 'utf8', maxBuffer: 1 << 28 })
const got = out.stdout.trimEnd().split('\n')
let bad = 0
for (let i = 0; i < records.length; i++) {
  if (got[i] !== expected[i]) {
    if (bad++ < 5) console.log(`ÉCART sur « ${records[i]} »\n  officiel: ${expected[i]}\n  nous    : ${got[i]}`)
  }
}
const passes = records.filter((r) => r.includes('--')).length
console.log(`${records.length} positions, ${GAMES} parties, ${passes} avec passe : ${bad} écart(s)`)
process.exit(bad ? 1 : 0)
