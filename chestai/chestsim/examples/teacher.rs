//! How well the teacher does, by stage-like settings.
use chestsim::env::{Env, Params, STAT_NAMES};

fn run(name: &str, p: &Params, n: u64) {
    let mut tot = [0.0f64; 20];
    for seed in 0..n {
        let mut env = Env::new(p.clone(), seed * 31 + 5);
        loop {
            let a = env.teacher();
            let (_, done) = env.step(&a);
            if done != 0 {
                break;
            }
        }
        for i in 0..20 {
            tot[i] = if i == 19 { tot[i].max(env.stats[i]) } else { tot[i] + env.stats[i] };
        }
    }
    let mins = tot[1] / 1200.0;
    print!("{name}: claims/min {:.2} kills/min {:.1} reset share {:.2} deaths/ep {:.2} | ", tot[2] / mins, tot[3] / mins, tot[5] / tot[4].max(1.0), tot[6] / n as f64);
    for (k, x) in STAT_NAMES.iter().zip(tot.iter()) {
        print!("{k}={x:.0} ");
    }
    println!();
}

fn main() {
    let mut easy = Params::default();
    easy.extreme_share = 0.0;
    easy.chests_min = 2.0;
    easy.chests_max = 4.0;
    easy.cluster_gap_max = 14.0;
    easy.relief = 5.0;
    easy.w_horseman = 0.0;
    easy.w_golem = 0.0;
    easy.w_rider = 0.0;
    easy.mob_hp_min = 16.0;
    easy.mob_hp_max = 30.0;
    easy.mob_dmg_max = 1.0;
    easy.slow_prob = 0.0;
    easy.blind_prob = 0.0;
    run("easy   ", &easy, 30);
    run("default", &Params::default(), 30);
    let mut ext = Params::default();
    ext.extreme_share = 1.0;
    run("extreme", &ext, 30);
}
