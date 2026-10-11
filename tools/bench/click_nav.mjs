// Deliver one navigation-key press through the app's own path, on request.
//
// usage: node click_nav.mjs <cdpPort> <Label> <triggerFile> <stampFile>
//
// Measurement vehicle for latency-product runs (see latency.ps1 -MirrorTitle):
// it connects to the app over CDP, waits for <triggerFile> to appear, writes the
// host clock (Date.now()) into <stampFile> the instant before it delivers the
// click, then clicks the session panel's <Label> button. Everything it does
// before that write — connecting, choosing the target, waiting — is deliberately
// outside the measured interval: the clock starts when the press is about to be
// handed to the app, so what remains is the app's own path (React handler →
// Tauri command → control socket → device) plus the device's reaction.
//
// The click is the human's mouse click, issued programmatically; the app cannot
// tell the difference and nothing about the product's path is bypassed.
import { writeFileSync, existsSync } from 'node:fs'

const [port, label, trigger, stamp] = process.argv.slice(2)
const die = (message) => {
  console.error(message)
  process.exit(1)
}

const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
const target = list.find((t) => t.type === 'page' && t.webSocketDebuggerUrl)
if (!target) die(`no page target on ${port}`)

const ws = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true })
  ws.addEventListener('error', () => reject(new Error('cdp socket failed')), { once: true })
})

let id = 0
const call = (method, params = {}) =>
  new Promise((resolve) => {
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

await call('Runtime.enable')

// Busy-wait for the sampler's go-ahead. One millisecond polls: the write below
// is what the measurement subtracts, so a late wake-up costs accuracy only if it
// lands after the write, which it cannot.
const deadline = Date.now() + 10_000
while (!existsSync(trigger)) {
  if (Date.now() > deadline) die('trigger never appeared')
  await new Promise((r) => setTimeout(r, 1))
}

const clickAt = Date.now()
writeFileSync(stamp, String(clickAt))

const expression = `(() => {
  const b = [...document.querySelectorAll(".nav-keys button")].find((x) => x.textContent.trim() === ${JSON.stringify(label)});
  if (!b) return JSON.stringify({ error: "no " + ${JSON.stringify(label)} + " button" });
  b.click();
  return JSON.stringify({ clicked: ${JSON.stringify(label)}, enabled: !b.disabled });
})()`

const res = await call('Runtime.evaluate', { expression, returnByValue: true })
const value = res.result?.result?.value
if (res.result?.exceptionDetails) die(`click failed: ${res.result.exceptionDetails.text}`)
console.log(JSON.stringify({ clickAt, value }))
ws.close()
process.exit(0)
