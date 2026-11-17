'use strict'
// A 1.8.8 server with the plugin and two players on it: every hit's velocity
// as the victim's connection receives it against what the plugin's console
// says it sent and against the numbers of its configuration; with the
// game's own knockback, the plugin's arithmetic against the server's; the
// wait between pearls; and the kit.
//
//     node test.js SERVER_DIR      (server.jar and plugins/kbtest.jar in it)
const { spawn } = require('child_process')
const fs = require('fs')
const path = require('path')
const mineflayer = require('mineflayer')

const dir = path.resolve(process.argv[2] || 'server')
const PORT = 25599
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

// The profiles of the plugin's configuration that come from the hit alone,
// and the horizontal push of the two that go the game's way.
const HIT = {
  servidor: { hG: 0.4, vG: 0.36075, hS: 0.5, vS: 0.1, stop: true },
  combo: { hG: 0.3, vG: 0.32, hS: 0.35, vS: 0.04, stop: false },
  cero: { hG: 0.0, vG: 0.0, hS: 0.0, vS: 0.0, stop: true }
}
const GAME = { bajo: 0.33, alto: 0.48 }
/** A velocity as its packet carries it. */
const sent = v => Math.trunc(Math.max(-3.9, Math.min(3.9, v)) * 8000)
const equal = (a, b) => a.length === b.length && a.every((x, i) => x === b[i])

let failed = 0
let passed = 0
function check(ok, what) {
  console.log((ok ? 'ok    ' : 'FAIL  ') + what)
  if (ok) passed++
  else failed++
}

// The server.

fs.mkdirSync(dir, { recursive: true })
fs.writeFileSync(path.join(dir, 'eula.txt'), 'eula=true\n')
fs.writeFileSync(path.join(dir, 'server.properties'), [
  'server-port=' + PORT, 'server-ip=127.0.0.1', 'online-mode=false', 'level-type=FLAT', 'generate-structures=false',
  'spawn-monsters=false', 'spawn-animals=false', 'spawn-npcs=false', 'spawn-protection=0', 'allow-nether=false',
  'pvp=true', 'gamemode=0', 'difficulty=1', 'view-distance=4', 'max-players=4', 'announce-player-achievements=false', ''
].join('\n'))
fs.writeFileSync(path.join(dir, 'bukkit.yml'), 'settings:\n  allow-end: false\n  connection-throttle: -1\n')

const lines = []
const server = spawn('java', ['-Xmx1G', '-jar', 'server.jar', '--nojline'], { cwd: dir })
let rest = ''
server.stdout.on('data', d => {
  rest += d
  let i
  while ((i = rest.indexOf('\n')) >= 0) {
    const line = rest.slice(0, i).replace(/\r$/, '')
    rest = rest.slice(i + 1)
    lines.push(line)
    console.log('[server] ' + line)
  }
})
server.stderr.on('data', d => process.stdout.write('[server] ' + d))
let exited = false
server.on('exit', () => { exited = true })
const say = command => server.stdin.write(command + '\n')

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

function parse(l) {
  let m = /KBVERIFY (same|DIFFERS) (ground|air) level (\d+) to (\S+) game (\S+) (\S+) (\S+) plugin (\S+) (\S+) (\S+)/.exec(l)
  if (m) {
    return { kind: 'verify', same: m[1] === 'same', state: m[2], level: +m[3], to: m[4], sent: [m[5], m[6], m[7]].map(x => sent(parseFloat(x))) }
  }
  m = /KBHIT (ground|air) level (\d+) to (\S+) profile (\S+) sent (-?\d+) (-?\d+) (-?\d+)/.exec(l)
  if (m) {
    return { kind: 'hit', state: m[1], level: +m[2], to: m[3], profile: m[4], sent: [+m[5], +m[6], +m[7]] }
  }
  return null
}

// The players.

function join(name) {
  return new Promise((resolve, reject) => {
    const bot = mineflayer.createBot({ host: '127.0.0.1', port: PORT, username: name, version: '1.8.8', auth: 'offline' })
    bot.vel = []
    bot.said = []
    bot.once('spawn', () => resolve(bot))
    bot.on('kicked', reason => console.log(name + ' was kicked: ' + JSON.stringify(reason)))
    bot.on('error', e => console.log(name + ': ' + e))
    bot.on('messagestr', m => bot.said.push(m))
    bot._client.on('entity_velocity', p => {
      if (bot.entity && p.entityId === bot.entity.id) bot.vel.push([p.velocity.x, p.velocity.y, p.velocity.z])
    })
    setTimeout(() => reject(new Error(name + ' did not get into the world')), 60000)
  })
}

async function place(bot, x, y, z, yaw) {
  const moved = new Promise(resolve => bot.once('forcedMove', resolve))
  // With its decimals: the game puts a whole number in the middle of its block.
  say(`tp ${bot.username} ${x.toFixed(2)} ${y.toFixed(2)} ${z.toFixed(2)} ${yaw} 0`)
  await Promise.race([moved, sleep(3000)])
}

async function main() {
  if (!await line(/Done \(/, 0, 240000)) throw new Error('the server did not start')
  check(!!await line(/Enabling KbTest/, 0, 1000), 'the server loads the plugin')
  const uno = await join('Uno')
  const dos = await join('Dos')
  await sleep(4000)
  say('difficulty 0')
  // Nobody dies of the test: hits take next to nothing and it is back at once.
  for (const name of ['Uno', 'Dos']) {
    say(`effect ${name} 11 100000 3`)
    say(`effect ${name} 10 100000 10`)
  }
  const at = uno.entity.position
  const [X, Y, Z] = [Math.floor(at.x) + 0.5, Math.round(at.y), Math.floor(at.z) + 0.5]
  // Uno looks along +z at Dos, two and a half blocks away, and Dos back.
  async function reset() {
    await Promise.all([place(uno, X, Y, Z, 0), place(dos, X, Y, Z + 2.5, 180)])
    await sleep(700)
  }
  await reset()
  check(uno.entity.onGround && dos.entity.onGround && uno.entity.position.y === Y && dos.entity.position.z === Z + 2.5,
    `the two stand where they were put (${uno.entity.position}, ${dos.entity.position})`)

  /** One hit; what the console said of it and what the victim received. */
  async function strike(att, vic, what, want) {
    const [n, v] = [lines.length, vic.vel.length]
    const target = att.players[vic.username] && att.players[vic.username].entity
    if (!target) {
      check(false, `${what}: ${att.username} does not see ${vic.username}`)
      return
    }
    att.attack(target)
    await sleep(450)
    const told = lines.slice(n).map(parse).filter(x => x && x.to === vic.username)
    const got = vic.vel.slice(v)
    let why = ''
    if (told.length !== 1 || got.length !== 1) {
      why = `${told.length} lines on the console and ${got.length} packets`
    } else {
      const t = told[0]
      if (!equal(t.sent, got[0])) why += ` the console says ${t.sent} and the victim received ${got[0]};`
      for (const k of Object.keys(want)) {
        if (k === 'exact') {
          if (!equal(want.exact, got[0])) why += ` it should be ${want.exact};`
        } else if (t[k] !== want[k]) {
          why += ` ${k} is ${t[k]}, not ${want[k]};`
        }
      }
    }
    check(why === '', `${what}: ${got.length === 1 ? got[0].join(' ') : ''}${why === '' ? '' : ' <-' + why}`)
    await reset()
  }
  async function sprint(bot, on) {
    bot.setControlState('sprint', on)
    await sleep(200)
  }
  async function jumping(bot, f) {
    bot.setControlState('jump', true)
    const until = Date.now() + 2000
    while (bot.entity.onGround && Date.now() < until) await sleep(10)
    await sleep(100)
    await f()
    bot.setControlState('jump', false)
    await sleep(700)
  }

  // The game's own knockback: the plugin only looks, and its arithmetic
  // has to give the server's own answer.
  say('kb use vanilla')
  say('kb verify on')
  say('kb log on')
  await sleep(300)
  const game = { kind: 'verify', same: true }
  for (let i = 0; i < 3; i++) await strike(uno, dos, 'the game, standing', { ...game, state: 'ground', level: 0 })
  for (let i = 0; i < 2; i++) await strike(dos, uno, 'the game, the other way', { ...game, state: 'ground', level: 0 })
  await sprint(uno, true)
  await strike(uno, dos, 'the game, with a sprint', { ...game, level: 1 })
  await strike(uno, dos, 'the game, the sprint is spent', { ...game, level: 0 })
  await sprint(uno, false)
  for (let i = 0; i < 2; i++) await jumping(dos, () => strike(uno, dos, 'the game, in the air', { ...game, state: 'air' }))
  await sprint(dos, true)
  await jumping(uno, () => strike(dos, uno, 'the game, in the air with a sprint', { ...game, state: 'air', level: 1 }))
  await sprint(dos, false)

  // The game's way with other numbers.
  for (const name of Object.keys(GAME)) {
    say('kb use ' + name)
    await sleep(300)
    const z = sent(GAME[name])
    const told = { kind: 'hit', profile: name }
    const before = dos.vel.length
    await strike(uno, dos, name + ', standing', { ...told, state: 'ground', level: 0 })
    const got = dos.vel[before]
    check(!!got && got[0] === 0 && got[2] === z, `${name}: ${z} along the hit`)
    await strike(dos, uno, name + ', the other way', { ...told, state: 'ground', level: 0 })
    await sprint(uno, true)
    await strike(uno, dos, name + ', with a sprint', { ...told, level: 1 })
    await sprint(uno, false)
    await jumping(dos, () => strike(uno, dos, name + ', in the air', { ...told, state: 'air' }))
  }

  // From the hit alone.
  for (const name of Object.keys(HIT)) {
    const p = HIT[name]
    say('kb use ' + name)
    await sleep(300)
    const told = { kind: 'hit', profile: name }
    const still = [0, sent(p.vG), sent(p.hG)]
    const run = [0, sent(p.vG + p.vS), sent(p.hG + 1 * p.hS)]
    await strike(uno, dos, name + ', standing', { ...told, state: 'ground', level: 0, exact: still })
    await strike(dos, uno, name + ', the other way', { ...told, state: 'ground', level: 0, exact: [0, sent(p.vG), sent(-p.hG)] })
    await sprint(uno, true)
    await strike(uno, dos, name + ', with a sprint', { ...told, state: 'ground', level: 1, exact: run })
    // A second hit of the same sprint carries it only where a server does not ask for a new one.
    await strike(uno, dos, name + ', the same sprint again', { ...told, state: 'ground', level: p.stop ? 0 : 1, exact: p.stop ? still : run })
    await sprint(uno, false)
    await sprint(uno, true)
    await strike(uno, dos, name + ', a new sprint', { ...told, state: 'ground', level: 1, exact: run })
    await sprint(uno, false)
    await jumping(dos, () => strike(uno, dos, name + ', in the air', { ...told, state: 'air' }))
  }

  // A profile for one player only.
  say('kb use servidor')
  say('kb use cero Dos')
  await sleep(300)
  await strike(uno, dos, 'a profile of its own for Dos', { kind: 'hit', profile: 'cero', exact: [0, 0, 0] })
  await strike(dos, uno, 'and Uno as everybody', { kind: 'hit', profile: 'servidor', exact: [0, sent(0.36075), sent(-0.4)] })
  say('kb clear Dos')
  await sleep(300)
  await strike(uno, dos, 'Dos as everybody again', { kind: 'hit', profile: 'servidor', exact: [0, sent(0.36075), sent(0.4)] })

  // The kit.
  say('kit Uno')
  await sleep(1000)
  const slot = i => uno.inventory.slots[i]
  const is = (i, name, count, meta) => !!slot(i) && slot(i).name === name && slot(i).count === count && (meta === undefined || slot(i).metadata === meta)
  const all = (from, to, name, count, meta) => {
    for (let i = from; i <= to; i++) if (!is(i, name, count, meta)) return false
    return true
  }
  check(is(36, 'diamond_sword', 1) && is(37, 'ender_pearl', 16) && all(38, 42, 'potion', 1, 16421) && is(43, 'potion', 1, 8226) && is(44, 'cooked_beef', 64),
    'the kit: sword, pearls, five healing potions, Speed and steak in the hotbar')
  check(all(9, 20, 'potion', 1, 16421) && all(21, 22, 'potion', 1, 8226) && !slot(23), 'the kit: twelve healing potions and two of Speed in the rows')
  check(is(5, 'diamond_helmet', 1) && is(6, 'diamond_chestplate', 1) && is(7, 'diamond_leggings', 1) && is(8, 'diamond_boots', 1), 'the kit: diamond armour on')

  // The wait between pearls.
  say('kb pearl 3')
  await sleep(300)
  uno.setQuickBarSlot(1)
  await uno.look(uno.entity.yaw, -Math.PI / 2, true)
  await sleep(300)
  const pearls = () => slot(37) ? slot(37).count : 0
  uno.activateItem()
  await sleep(600)
  check(pearls() === 15, `a pearl is thrown (${pearls()} left)`)
  const heard = uno.said.length
  uno.activateItem()
  await sleep(600)
  check(pearls() === 15 && uno.said.slice(heard).some(m => /Perla en \d/.test(m)), `the next one is refused and the player told (${pearls()} left, ${JSON.stringify(uno.said.slice(heard))})`)
  await sleep(2400)
  uno.activateItem()
  await sleep(600)
  check(pearls() === 14, `after the wait a pearl is thrown again (${pearls()} left)`)

  // What the plugin counted.
  const n = lines.length
  say('kb ver')
  const m = await line(/Golpes cambiados (\d+)\. Verificados (\d+), distintos (\d+)/, n, 5000)
  check(!!m && +m[1] > 0 && +m[2] >= 10 && +m[3] === 0, 'the plugin counted: ' + (m ? m[0] : 'nothing'))
  uno.quit()
  dos.quit()
}

const limit = setTimeout(() => {
  console.log('FAIL  the test took too long')
  server.kill('SIGKILL')
  process.exit(1)
}, 420000)

main().catch(e => {
  console.log('FAIL  ' + (e && e.stack || e))
  failed++
}).then(async () => {
  say('stop')
  const until = Date.now() + 30000
  while (!exited && Date.now() < until) await sleep(100)
  if (!exited) server.kill('SIGKILL')
  clearTimeout(limit)
  console.log(`${passed} checks passed, ${failed} failed`)
  process.exit(failed === 0 && passed > 0 ? 0 : 1)
})
