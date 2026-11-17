//! Recordings of the real 1.8.9 game, replayed against the simulator.
//!
//! Each file in `tests/traces` was written by the rig in `parity/` while
//! the real client and its integrated server ran a scripted scene. The
//! simulator has to reproduce every one of them bit for bit: positions,
//! motion, angles, every packet sent in either direction and the state of
//! the server's entity tracker.
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
    assert!(files.len() >= 27, "expected the recordings in tests/traces, found {}", files.len());
    let (mut ticks, mut hits, mut sprint_hits, mut crits, mut attacks, mut teleports, mut kb) = (0, 0, 0, 0, 0, 0, 0);
    let (mut blocked, mut right_clicks, mut lost, mut silent, mut swords) = (0, 0, 0, 0, 0);
    let (mut thrown, mut landed, mut on_player, mut slots, mut bare, mut seen, mut flying, mut outside) = (0, 0, 0, 0, 0, 0, 0, 0);
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
}

#[test]
fn a_small_knockback_error_is_noticed() {
    let f = traces().into_iter().find(|f| f.contains("fight1")).expect("fight1 recording");
    let o = Options { wrong_knockback: true, ..Options::default() };
    let r = replay(&read_trace(&f).unwrap(), &o).unwrap();
    assert!(r.server.bad_ticks > 50, "a 2.5% error in the knockback went unnoticed");
}
