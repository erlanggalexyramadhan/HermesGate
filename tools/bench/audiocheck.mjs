// Audio check: sample the panel's audio line while the device is made to play
// something, so a forwarded *sound* can be told apart from forwarded silence.
// usage: node audiocheck.mjs <minutes-of-samples>   (10 samples, 1 s apart)
import { spawnSync } from 'node:child_process'

const adb = 'C:/Users/Lextronaut/Documents/project/HermesGate/src-tauri/binaries/adb.exe'
const serial = 'bc058b2c'
const key = (code) => spawnSync(adb, ['-s', serial, 'shell', 'input', 'keyevent', String(code)]).status

const port = process.env.HG_CDP_PORT || 9223
const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
const target = list.find((t) => t.type === 'page' && t.webSocketDebuggerUrl)
const ws = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((resolve) => ws.addEventListener('open', resolve, { once: true }))
let id = 0
const call = (method, params = {}) => new Promise((resolve) => {
  const mid = ++id
  const onMessage = (ev) => {
    const msg = JSON.parse(ev.data)
    if (msg.id !== mid) return
    ws.removeEventListener('message', onMessage)
    resolve(msg)
  }
  ws.addEventListener('message', onMessage)
  ws.send(JSON.stringify({ id: mid, method, params }))
})

const grab = async () => {
  const expression = `(() => {
    const lines = [...document.querySelectorAll('p')].map((p) => p.innerText);
    const sound = lines.find((t) => t.includes('loudest sample')) || '';
    const level = (sound.match(/last second: [^.]*\\./) || ['?'])[0];
    const played = (sound.match(/has played (\\d+) s/) || ['?', '?'])[1];
    const body = document.body.innerText;
    const state = (body.match(/MIRROR SESSION\\n(\\S+)/) || ['?', '?'])[1];
    const rate = (body.match(/AUDIO\\n(.*)/) || ['?', '?'])[1];
    return state + ' | audio=' + rate + ' | played=' + played + 's | ' + level;
  })()`
  const reply = await call('Runtime.evaluate', { expression, returnByValue: true })
  return reply.result?.result?.value ?? 'evaluate failed'
}

const before = process.argv[2] ? [] : null
for (let tick = 0; tick <= 12; tick++) {
  if (tick === 2 || tick === 4) console.log(`(adb volume up at tick ${tick}: ${key(24) === 0 ? 'sent' : 'failed'})`)
  if (tick === 8 || tick === 10) console.log(`(adb volume down at tick ${tick}: ${key(25) === 0 ? 'sent' : 'failed'})`)
  const line = await grab()
  console.log(String(tick).padStart(2) + 's ' + line)
  if (tick < 12) await new Promise((r) => setTimeout(r, 1000))
}
ws.close()
