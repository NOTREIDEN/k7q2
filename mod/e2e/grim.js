'use strict'
// The mod on a real server: a 1.8.8 server with the knockback plugin and the
// Grim anticheat, the game client with the mod on it and a second player to
// fight. Prints every line Grim's console has about the mod's player, by
// check, and what that player did meanwhile. A report, not a verdict: it
// fails only if the server, Grim or the client did not do their part.
//
//     node grim.js SERVER_DIR MOD_DIR [SECONDS]
// server.jar and plugins/ in SERVER_DIR; `gradle servertest` is run in MOD_DIR.
const { spawn } = require('child_process')
const fs = require('fs')
const path = require('path')
const mineflayer = require('mineflayer')

const dir = path.resolve(process.argv[2] || 'server')
const modDir = path.resolve(process.argv[3] || '..')
const SECONDS = parseInt(process.argv[4] || '120', 10)
const PORT = 25599
const RIVAL = 'Rival'
/** Lines of the server that are kept and printed: one that fails in a loop writes without end. */
const MOST = 60000
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
const plain = s => s.replace(/\u001b\[[0-9;]*m/g, '').replace(/§./g, '')

fs.mkdirSync(dir, { recursive: true })
fs.writeFileSync(path.join(dir, 'eula.txt'), 'eula=true\n')
fs.writeFileSync(path.join(dir, 'server.properties'), [
  'server-port=' + PORT, 'server-ip=127.0.0.1', 'online-mode=false', 'level-type=FLAT', 'generate-structures=false',
  'spawn-monsters=false', 'spawn-animals=false', 'spawn-npcs=false', 'spawn-protection=0', 'allow-nether=false',
  'pvp=true', 'gamemode=0', 'difficulty=1', 'view-distance=4', 'max-players=4', 'announce-player-achievements=false',
  // The network code this old server brings for Linux does not run on the Java that Grim needs.
  'use-native-transport=false', ''
].join('\n'))
fs.writeFileSync(path.join(dir, 'bukkit.yml'), 'settings:\n  allow-end: false\n  connection-throttle: -1\n')

let lines = []
let server = null
let exited = true
function start() {
  lines = []
  exited = false
  server = spawn(process.env.SERVER_JAVA || 'java', ['-Xmx1G', '-jar', 'server.jar', '--nojline'], { cwd: dir })
  let rest = ''
  server.stdout.on('data', d => {
    rest += d
    let i
    while ((i = rest.indexOf('\n')) >= 0) {
      const line = plain(rest.slice(0, i).replace(/\r$/, ''))
      rest = rest.slice(i + 1)
      if (lines.length >= MOST) {
        if (lines.length === MOST) {
          lines.push('')
          console.log('FAIL  the server said more than ' + MOST + ' lines; the rest is not kept')
        }
        continue
      }
      lines.push(line)
      console.log('[server] ' + line)
    }
  })
  let errors = 0
  server.stderr.on('data', d => {
    errors += d.length
    if (errors < 2000000) process.stdout.write('[server] ' + d)
  })
  server.on('exit', () => { exited = true })
}
const say = command => server.stdin.write(command + '\n')
async function stop() {
  if (exited) return
  say('stop')
  const until = Date.now() + 30000
  while (!exited && Date.now() < until) await sleep(100)
  if (!exited) server.kill('SIGKILL')
}
async function line(re, from, ms) {
  const until = Date.now() + ms
  while (Date.now() < until) {
    for (let i = from; i < lines.length; i++) {
      const m = re.exec(lines[i])
      if (m) return m
    }
    if (exited) break
    await sleep(50)
  }
  return null
}

/** Every file under a folder. */
function files(d) {
  let r = []
  if (!fs.existsSync(d)) return r
  for (const f of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, f.name)
    r = f.isDirectory() ? r.concat(files(p)) : r.concat([p])
  }
  return r
}

/** The second player: walks up to the other one, looks at it and hits it when it is near. */
function rival() {
  return new Promise((resolve, reject) => {
    const bot = mineflayer.createBot({ host: '127.0.0.1', port: PORT, username: RIVAL, version: '1.8.8', auth: 'offline' })
    bot.once('spawn', () => resolve(bot))
    bot.on('kicked', reason => console.log(RIVAL + ' was kicked: ' + JSON.stringify(reason)))
    bot.on('error', e => console.log(RIVAL + ': ' + e))
    bot.on('spawn', () => setTimeout(() => { if (!exited) say('kit ' + RIVAL) }, 1000))
    let last = 0
    bot.on('physicsTick', () => {
      let near = null
      for (const name of Object.keys(bot.players)) {
        const e = bot.players[name].entity
        if (name !== RIVAL && e && (!near || bot.entity.position.distanceTo(e.position) < bot.entity.position.distanceTo(near.position))) near = e
      }
      if (!near) {
        bot.setControlState('forward', false)
        return
      }
      const d = bot.entity.position.distanceTo(near.position)
      bot.lookAt(near.position.offset(0, 1.5, 0), true)
      bot.setControlState('forward', d > 5)
      if (bot.quickBarSlot !== 0) bot.setQuickBarSlot(0)
      if (d < 2.9 && Date.now() - last > 110) {
        last = Date.now()
        bot.attack(near)
      }
    })
    setTimeout(() => reject(new Error(RIVAL + ' did not get into the world')), 60000)
  })
}

let failed = 0
function check(ok, what) {
  console.log((ok ? 'ok    ' : 'FAIL  ') + what)
  if (!ok) failed++
}

async function main() {
  // A first start, for Grim to write its files; then they are told to print everything to the console.
  start()
  if (!await line(/Done \(/, 0, 300000)) throw new Error('the server did not start')
  await sleep(3000)
  await stop()
  const grimFiles = files(path.join(dir, 'plugins')).filter(f => /grim/i.test(f) && /\.ya?ml$/.test(f))
  console.log('Grim wrote: ' + grimFiles.map(f => path.relative(dir, f)).join(', '))
  check(grimFiles.length > 0, 'Grim is on the server and wrote its configuration')
  for (const f of grimFiles) {
    const before = fs.readFileSync(f, 'utf8')
    let after = before.replace(/(print-to-console:\s*)false/g, '$1true')
    if (after !== before) console.log('printing to the console switched on in ' + path.relative(dir, f))
    if (/punishments\.ya?ml$/.test(f)) {
      // Grim tells and does not kick or ban, as on the server this is for.
      const kept = []
      for (const l of after.split('\n')) {
        if (/^\s*-\s/.test(l) && /\b(kick|ban|tempban)\b/i.test(l)) console.log('taken out of ' + path.relative(dir, f) + ': ' + l.trim())
        else kept.push(l)
      }
      after = kept.join('\n')
      const left = Array.from(new Set(kept.filter(l => /^\s*-\s*"\d+:\d+\s/.test(l)).map(l => l.trim().replace(/^-\s*"\d+:\d+\s+/, '').replace(/"$/, ''))))
      console.log('what Grim does about a flag: ' + left.join(' | '))
    }
    if (after !== before) fs.writeFileSync(f, after)
  }

  start()
  if (!await line(/Done \(/, 0, 300000)) throw new Error('the server did not start again')
  say('kb use servidor')
  say('gamerule keepInventory false')
  const bot = await rival()
  await sleep(2000)

  // The game client with the mod.
  const log = fs.createWriteStream(path.join(modDir, 'runclient.log'))
  const from = lines.length
  const client = spawn('xvfb-run', ['-a', '-s', '-screen 0 854x480x24', 'gradle', 'servertest', '-Pserver=127.0.0.1:' + PORT, '-Pseconds=' + SECONDS, '--stacktrace'],
    { cwd: modDir, env: Object.assign({}, process.env, { LIBGL_ALWAYS_SOFTWARE: '1' }) })
  client.stdout.pipe(log)
  client.stderr.pipe(log)
  let clientDone = false
  client.on('exit', () => { clientDone = true })
  // This server does not tell its console of a join in the words the players read.
  const joined = await line(/\]: (\w+)\[\/[^\]]*\] logged in with entity id/, from, 900000)
  const name = joined ? joined[1] : null
  check(!!name && name !== RIVAL, 'the client with the mod joined as ' + name)
  if (!name) {
    client.kill('SIGKILL')
    return
  }
  // Its kit now and after every death, and now and then hungry and without Speed, to see it eat, drink and go to its inventory.
  let seen = lines.length
  let kits = 0
  const outfit = who => {
    say('kit ' + who)
    if (who === name) {
      kits++
      say(`effect ${name} clear`)
      if (kits % 2 === 1) say(`effect ${name} 17 2 30`)
    }
  }
  setTimeout(() => outfit(name), 1500)
  // A death also as the second player reads it, should the console not have it.
  let lastKit = 0
  bot.on('messagestr', m => {
    if (m.startsWith(name + ' ') && /slain|killed|died|fell|shot|blown|burned|drowned|starved|withered/.test(m) && Date.now() - lastKit > 4000) {
      lastKit = Date.now()
      setTimeout(() => { if (!exited) outfit(name) }, 2500)
    }
  })
  // The ping the server has for it: what the player list says and what Grim measures.
  const pings = []
  const asked = setInterval(() => {
    if (exited || clientDone) return
    const listed = bot.players[name] ? bot.players[name].ping : null
    if (listed !== null) pings.push(listed)
    console.log(`PING  ${name} in the player list: ${listed} ms; ${RIVAL}: ${bot.players[RIVAL] ? bot.players[RIVAL].ping : null} ms`)
    say('grim profile ' + name)
  }, 20000)
  const until = Date.now() + (SECONDS + 600) * 1000
  while (!clientDone && Date.now() < until) {
    for (; seen < lines.length; seen++) {
      const m = /\]: (\w+) (was slain|was killed|died|was shot|fell|hit the ground|starved|withered|drowned|burned|went up|was blown|suffocated|was pricked|walked into|tried to swim)/.exec(lines[seen])
      if (m && m[1] === name && Date.now() - lastKit > 4000) {
        lastKit = Date.now()
        setTimeout(() => { if (!exited) outfit(name) }, 2500)
      }
    }
    await sleep(200)
  }
  clearInterval(asked)
  if (!clientDone) client.kill('SIGKILL')
  const result = path.join(modDir, 'run', 'servertest.txt')
  const told = fs.existsSync(result) ? fs.readFileSync(result, 'utf8').trim() : 'nothing written'
  check(/^ok/.test(told), 'the mod fought: ' + told)

  // That flags do reach the console: the second player does something no client can.
  const mark = lines.length
  const p = bot.entity.position
  for (let i = 1; i <= 6; i++) {
    bot._client.write('position', { x: p.x, y: p.y + 4 * i, z: p.z, onGround: false })
    await sleep(60)
  }
  await sleep(3000)
  const control = lines.slice(mark).filter(l => l.includes(RIVAL) && /fail|flag|violat|Simulation|setback/i.test(l))
  check(control.length > 0, `a player that jumps 24 blocks up is told on the console (${control.length} lines)`)
  bot.quit()

  // What Grim said about the mod's player.
  const about = lines.slice(from).filter(l => l.includes(name) && /grim|fail|flag|violat/i.test(l) && !/joined the game|left the game|logged in|lost connection|Kit para/.test(l))
  const by = {}
  for (const l of about) {
    const m = /failed\s+(\S+)/i.exec(l)
    const k = m ? m[1] : '(other)'
    by[k] = (by[k] || 0) + 1
  }
  console.log('')
  console.log(`PING  of ${name} in the player list while it fought: ${pings.join(', ')} ms`)
  for (const l of lines.slice(from).filter(l => /ping/i.test(l) && !/Server Ping Player Sample/.test(l)).slice(-12)) console.log('PING  | ' + l)
  console.log(`GRIM about ${name}: ${about.length} lines` + (about.length ? '' : ' (nothing flagged)'))
  for (const k of Object.keys(by).sort((a, b) => by[b] - by[a])) console.log(`GRIM   ${k}: ${by[k]}`)
  for (const l of about.slice(0, 400)) console.log('GRIM   | ' + l)
  const rivalLines = lines.slice(from, mark).filter(l => l.includes(RIVAL) && /failed/i.test(l)).length
  console.log(`GRIM about ${RIVAL} (a scripted player, for comparison): ${rivalLines} lines before its jump`)
}

const limit = setTimeout(() => {
  console.log('FAIL  the test took too long')
  if (server) server.kill('SIGKILL')
  process.exit(1)
}, (SECONDS + 1500) * 1000)

main().catch(e => {
  console.log('FAIL  ' + (e && e.stack || e))
  failed++
  for (const l of lines.slice(-120)) console.log('LAST  | ' + l)
}).then(async () => {
  await stop()
  clearTimeout(limit)
  console.log(failed === 0 ? 'the test ran as meant' : failed + ' parts of the test did not run as meant')
  process.exit(failed === 0 ? 0 : 1)
})
