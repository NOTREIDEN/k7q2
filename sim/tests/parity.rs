//! Recordings of the real 1.8.9 game, replayed against the simulator.
//!
//! Each file in `tests/traces` was written by the rig in `parity/` while
//! the real client and its integrated server ran a scripted scene. The
//! simulator has to reproduce every one of them bit for bit: positions,
//! motion, angles, every packet sent in either direction and the state of
//! the server's entity tracker.
//!
//! In the recordings named `view_*` the mod's own code (mod/client) sat in
//! the real client: it read the client before every tick and pressed the
//! scene's keys. What it read has to be what the simulator's client holds
//! at that moment. In those named `bot_*` a policy played through that
//! code: there the simulator's agent, replaying, has to be shown the same
//! numbers and to press the same keys as well.
use pvpsim::replay::{read_trace, replay, Options};

fn traces() -> Vec<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/traces");
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .expect("tests/traces is missing")
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_string_lossy().into_owned())
        .filter(|p| p.ends_with(".log.gz") || p.ends_with(".log"))
        .collect();
    v.sort();
    v
}

#[test]
fn the_real_game_is_reproduced_bit_for_bit() {
    let files = traces();
    assert!(files.len() >= 67, "expected the recordings in tests/traces, found {}", files.len());
    let (mut views, mut flying_views, mut shown, mut played, mut twice) = (0, 0, 0, 0, 0);
    let (mut ticks, mut hits, mut sprint_hits, mut crits, mut attacks, mut teleports, mut kb) = (0, 0, 0, 0, 0, 0, 0);
    let (mut blocked, mut right_clicks, mut lost, mut silent, mut swords) = (0, 0, 0, 0, 0);
    let (mut thrown, mut landed, mut on_player, mut slots, mut bare, mut seen, mut flying, mut outside) = (0, 0, 0, 0, 0, 0, 0, 0);
    let (mut potions, mut broken, mut direct, mut both, mut healed, mut health_lost) = (0, 0, 0, 0, 0, 0);
    let (mut drinks, mut steak, mut bottle) = (0, 0, 0);
    let (mut opened, mut inv_ticks, mut let_go, mut clicks, mut moved, mut answered) = (0, 0, 0, 0, 0, 0);
    let (mut taken, mut put_away, mut nothing, mut refused, mut unseen, mut in_use) = (0, 0, 0, 0, 0, 0);
    for f in &files {
        let r = replay(&read_trace(f).unwrap(), &Options::default()).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(r.identical(), "{f}\n{}", r.describe());
        ticks += r.client.ticks + r.server.ticks;
        hits += r.server.get("hits");
        sprint_hits += r.server.get("sprint hits");
        crits += r.server.get("critical hits");
        attacks += r.client.get("attacks");
        teleports += r.server.get("teleports back");
        kb += r.client.get("knockbacks taken");
        blocked += r.server.get("blocked hits");
        right_clicks += r.client.get("right clicks");
        lost += r.client.get("clicks lost to a block");
        silent += r.client.get("blocks dropped silently");
        swords += r.client.get("swords sent again");
        thrown += r.server.get("pearls thrown");
        landed += r.server.get("pearls landed");
        on_player += r.server.get("pearls landed on a player");
        slots += r.server.get("slot changes");
        bare += r.server.get("hits without the sword");
        seen += r.client.get("pearls seen");
        flying += r.client.get("ticks with a pearl in flight");
        outside += r.server.get("pearl ticks outside the loaded world");
        potions += r.server.get("potions thrown");
        broken += r.server.get("potions broken");
        direct += r.server.get("potions broken on a player");
        both += r.server.get("potions that healed both");
        healed += r.server.get("health from potions, in tenths");
        health_lost += r.server.get("health lost, in tenths");
        drinks += r.server.get("drinks begun");
        steak += r.server.get("clicks with steak");
        bottle += r.server.get("clicks with a bottle");
        opened += r.client.get("inventories opened");
        inv_ticks += r.client.get("ticks with the inventory open");
        let_go += r.client.get("uses ended by the inventory");
        clicks += r.client.get("inventory clicks");
        moved += r.client.get("clicks that moved a stack");
        answered += r.client.get("refused clicks answered");
        taken += r.server.get("potions taken from the rows");
        put_away += r.server.get("stacks put away");
        nothing += r.server.get("clicks that moved nothing");
        refused += r.server.get("clicks refused");
        unseen += r.server.get("clicks not looked at");
        in_use += r.server.get("clicks on the item in use");
        views += r.client.get("views compared");
        flying_views += r.client.get("views with a pearl or a potion");
        shown += r.client.get("observations compared");
        played += r.client.get("ticks with the agent at the keys");
        twice += r.client.get("ticks with two clicks");
    }
    // The recordings have to keep covering what a duel is made of.
    assert!(ticks > 15000, "ticks {ticks}");
    assert!(hits > 500 && sprint_hits > 300 && crits > 50, "hits {hits}, sprint {sprint_hits}, critical {crits}");
    assert!(attacks > 300 && kb > 400, "attacks {attacks}, knockbacks {kb}");
    assert!(teleports > 0, "no recording has the server putting a player back");
    // Sword blocking: hits taken on a block, right clicks, clicks made
    // while blocking, the server sending the sword again after a hit, and
    // the client dropping its block when that arrives.
    assert!(blocked > 150 && right_clicks > 400 && lost > 800, "blocked {blocked}, right clicks {right_clicks}, lost {lost}");
    assert!(swords > 200 && silent >= 5, "swords sent again {swords}, blocks dropped silently {silent}");
    // Ender pearls: thrown by both players, landing on blocks and on the
    // other player, seen in flight by the client, with the changes of
    // hotbar slot around them and hits made while the sword was not the
    // item the server had in the attacker's hand.
    assert!(thrown > 150 && landed > 150 && on_player > 15, "pearls thrown {thrown}, landed {landed}, on a player {on_player}");
    assert!(slots > 400 && bare > 15, "slot changes {slots}, hits without the sword {bare}");
    assert!(seen > 150 && flying > 400, "pearls seen {seen}, client ticks with one in flight {flying}");
    // No pearl got out of the part of the world the rig keeps loaded, where
    // the game stops updating it.
    assert!(outside == 0, "{outside} pearl ticks outside the loaded world");
    // Health and splash potions of healing: hits that cost health, potions
    // thrown by both players that broke on blocks and on players, and the
    // health they gave back, to the thrower alone and to both.
    assert!(health_lost > 10000, "health lost, in tenths: {health_lost}");
    assert!(potions > 60 && broken > 60 && direct > 8, "potions thrown {potions}, broken {broken}, on a player {direct}");
    assert!(both > 30 && healed > 1500, "potions that healed both {both}, health from potions in tenths {healed}");
    // Potions of Speed drunk and steak eaten, to the end and cut short, and
    // right clicks with the bottle a drink leaves.
    assert!(drinks >= 15 && steak > 150 && bottle > 60, "drinks begun {drinks}, clicks with steak {steak}, with a bottle {bottle}");
    // The inventory: its screen opened in the middle of a fight, of a block,
    // a drink and a meal, the ticks it stays open, potions shift-clicked
    // down from its rows and bottles put away by both players, clicks that
    // find nothing to move, clicks the server refuses (and the client's
    // answer to them), clicks it does not look at until that answer has
    // come, and clicks on the item in use.
    assert!(opened >= 90 && inv_ticks > 600 && let_go >= 25, "inventories opened {opened}, ticks open {inv_ticks}, uses ended {let_go}");
    assert!(clicks > 150 && moved > 80 && answered >= 10, "clicks {clicks}, that moved a stack {moved}, refusals answered {answered}");
    assert!(taken > 140 && put_away > 80 && nothing > 180, "potions taken {taken}, stacks put away {put_away}, clicks on nothing {nothing}");
    assert!(refused > 50 && unseen >= 20 && in_use >= 3, "clicks refused {refused}, not looked at {unseen}, on the item in use {in_use}");
    // The mod's code in the real client: what it read before a tick, with
    // and without pearls and potions in flight, and a policy playing
    // through it, shown what the simulator's agent is shown and pressing
    // what it presses.
    assert!(views > 12000 && flying_views > 900, "views compared {views}, of which with something in flight {flying_views}");
    assert!(shown > 4000 && played > 4000, "observations compared {shown}, ticks with the agent at the keys {played}");
    // Two clicks inside one tick, as a mouse clicked more than twenty times
    // a second sends them.
    assert!(twice > 500, "ticks with two clicks {twice}");
}

/// A recording with one thing changed in its text.
fn changed(name: &str, from: &str, to: &str) -> String {
    let f = traces().into_iter().find(|f| f.contains(name)).unwrap_or_else(|| panic!("no {name} recording"));
    let text = read_trace(&f).unwrap();
    assert!(text.contains(from), "{name} has no {from}");
    text.replacen(from, to, 1)
}

#[test]
fn a_view_read_differently_is_noticed() {
    // One number more at the front: everything after it is out of place.
    let r = replay(&changed("view_pot5", "\nCV 300 v=", "\nCV 300 v=0,"), &Options::default()).unwrap();
    assert!(!r.identical() && r.client.get("views that differ") == 1, "{}", r.describe());
    // The health of the other player, told as one less.
    let text = changed("view_pot5", "\nCV 300 v=", "\nCV 300 v=");
    let line = text.lines().find(|l| l.starts_with("CV 300 ")).unwrap();
    let mut n: Vec<String> = line["CV 300 v=".len()..].split(',').map(|s| s.to_string()).collect();
    let k = (0..n.len()).find(|&k| pvpsim::agent::view_name(k) == "other health").unwrap();
    n[k] = if n[k] == "19" { "18".into() } else { "19".into() };
    let r = replay(&text.replacen(line, &format!("CV 300 v={}", n.join(",")), 1), &Options::default()).unwrap();
    assert!(!r.identical() && r.client.get("views that differ") == 1, "{}", r.describe());
}

#[test]
fn an_agent_shown_or_pressing_something_else_is_noticed() {
    let r = replay(&changed("bot1", "\nCO 200 o=", "\nCO 200 o=0,"), &Options::default()).unwrap();
    assert!(!r.identical() && r.client.get("observations that differ") == 1, "{}", r.describe());
    // Another choice for the first control, forwards or backwards.
    let text = changed("bot1", "\nCA 200 a=", "\nCA 200 a=");
    let line = text.lines().find(|l| l.starts_with("CA 200 ")).unwrap();
    let first = &line["CA 200 a=".len().."CA 200 a=".len() + 1];
    let other = if first == "0" { "1" } else { "0" };
    let r = replay(&text.replacen(line, &line.replacen(&format!("a={first}"), &format!("a={other}"), 1), 1), &Options::default()).unwrap();
    assert!(!r.identical() && r.client.get("keys that differ") >= 1, "{}", r.describe());
}

#[test]
fn a_small_knockback_error_is_noticed() {
    let f = traces().into_iter().find(|f| f.contains("fight1")).expect("fight1 recording");
    let o = Options { wrong_knockback: true, ..Options::default() };
    let r = replay(&read_trace(&f).unwrap(), &o).unwrap();
    assert!(r.server.bad_ticks > 50, "a 2.5% error in the knockback went unnoticed");
}
