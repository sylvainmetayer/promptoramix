// Ressemblance de chaque adversaire avec l'IA de la maison, depuis l'export
// « Mes parties » : à chacun de leurs coups, le coup de la maison (900 ms) dans
// la même position. Un adversaire qui la suit coup pour coup peut être battu
// hors ligne, en cherchant contre elle une ligne gagnante pour le livre.
//   node duel/clones.ts parties.json [--shard i/n]
import { readFileSync } from 'node:fs'
import { chooseBotMove } from './linkx-official/supabase/functions/bot-linkx/botMove.ts'

const shardArg = process.argv.indexOf('--shard')
const [SHARD, SHARDS] = shardArg >= 0 ? process.argv[shardArg + 1].split('/').map(Number) : [0, 1]
const { parties } = JSON.parse(readFileSync(process.argv[2], 'utf8'))
const games = parties.filter((p: any) => p.notation && p.vague_id)
games.forEach((p: any, g: number) => {
  if (g % SHARDS !== SHARD) return
  const moves: string[] = p.notation.split(' ')
  const theirs = p.couleur === 'blue' ? 'white' : 'blue'
  // Les bleus commencent ; `--` est une passe, jouée par la plateforme.
  const marks = moves.map((m, i) => {
    const color = i % 2 === 0 ? 'blue' : 'white'
    if (color !== theirs || m === '--') return null
    const r = chooseBotMove(moves.slice(0, i).join(' '), color, 900)
    return r.ok && r.move === m ? '=' : `${m}/${r.ok ? r.move : '?'}`
  })
  const own = marks.filter((x) => x !== null)
  console.log(JSON.stringify({ adversaire: p.adversaire, nous: p.couleur, issue: p.issue, same: own.filter((x) => x === '=').length, of: own.length, marks: own.join(' ') }))
})
