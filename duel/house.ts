// Sert l'IA de la maison officielle sur stdin/stdout : une ligne JSON {record, budget} par coup.
import { createInterface } from 'node:readline'
import { chooseBotMove } from './linkx-official/supabase/functions/bot-linkx/botMove.ts'

const rl = createInterface({ input: process.stdin })
for await (const line of rl) {
  const { record, budget } = JSON.parse(line)
  const result = chooseBotMove(record, null, budget)
  process.stdout.write(JSON.stringify(result) + '\n')
}
