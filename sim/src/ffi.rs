//! C interface used by the Python trainer through `ctypes`.
use crate::env::{Mode, VecEnv, ACT_DIMS, ACT_HEADS, OBS_DIM, PRIV_DIM, STATS_DIM, VIEW_DIM};
use std::slice;

#[no_mangle]
pub extern "C" fn pvp_obs_dim() -> i32 {
    OBS_DIM as i32
}

#[no_mangle]
pub extern "C" fn pvp_priv_dim() -> i32 {
    PRIV_DIM as i32
}

#[no_mangle]
pub extern "C" fn pvp_stats_dim() -> i32 {
    STATS_DIM as i32
}

#[no_mangle]
pub extern "C" fn pvp_act_heads() -> i32 {
    ACT_HEADS as i32
}

/// # Safety
/// `out` must point to `pvp_act_heads()` writable ints.
#[no_mangle]
pub unsafe extern "C" fn pvp_act_dims(out: *mut i32) {
    slice::from_raw_parts_mut(out, ACT_HEADS).copy_from_slice(&ACT_DIMS);
}

/// `mode` 0 is self-play, 1 is against scripted bots.
#[no_mangle]
pub extern "C" fn pvp_vec_new(n: i32, mode: i32, seed: u64) -> *mut VecEnv {
    let mode = if mode == 1 { Mode::Bot } else { Mode::SelfPlay };
    Box::into_raw(Box::new(VecEnv::new(n.max(1) as usize, mode, seed)))
}

/// # Safety
/// `p` must come from `pvp_vec_new` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_free(p: *mut VecEnv) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

/// # Safety
/// `p` must be a live pointer from `pvp_vec_new`.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_set_param(p: *mut VecEnv, id: i32, value: f64) -> i32 {
    (*p).set_param(id, value) as i32
}

/// # Safety
/// `obs` and `privs` must hold n*obs_dim and n*priv_dim floats.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_reset(p: *mut VecEnv, obs: *mut f32, privs: *mut f32) {
    let v = &mut *p;
    let n = v.envs.len();
    v.reset(slice::from_raw_parts_mut(obs, n * OBS_DIM), slice::from_raw_parts_mut(privs, n * PRIV_DIM));
}

/// # Safety
/// Every buffer must hold one row per environment of the matching width.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_step(
    p: *mut VecEnv,
    actions: *const i32,
    obs: *mut f32,
    privs: *mut f32,
    rew: *mut f32,
    done: *mut u8,
    fobs: *mut f32,
    fpriv: *mut f32,
) -> i32 {
    let v = &mut *p;
    let n = v.envs.len();
    v.step(
        slice::from_raw_parts(actions, n * ACT_HEADS),
        slice::from_raw_parts_mut(obs, n * OBS_DIM),
        slice::from_raw_parts_mut(privs, n * PRIV_DIM),
        slice::from_raw_parts_mut(rew, n),
        slice::from_raw_parts_mut(done, n),
        slice::from_raw_parts_mut(fobs, n * OBS_DIM),
        slice::from_raw_parts_mut(fpriv, n * PRIV_DIM),
    ) as i32
}

/// # Safety
/// `out` must hold one byte per environment.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_overridden(p: *mut VecEnv, out: *mut u8) {
    let v = &*p;
    v.overridden(slice::from_raw_parts_mut(out, v.envs.len()));
}

/// # Safety
/// `out` must hold one i32 per control for each environment.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_teach(p: *mut VecEnv, out: *mut i32) {
    let v = &*p;
    v.teach(slice::from_raw_parts_mut(out, v.envs.len() * crate::agent::ACT_HEADS));
}

/// # Safety
/// `out` must hold `pvp_stats_dim()` doubles.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_take_stats(p: *mut VecEnv, out: *mut f64) {
    (*p).take_stats(slice::from_raw_parts_mut(out, STATS_DIM));
}

#[no_mangle]
pub extern "C" fn pvp_view_dim() -> i32 {
    VIEW_DIM as i32
}

/// Snapshot of duel `index` for drawing it; see `Env::write_view`.
///
/// # Safety
/// `out` must hold `pvp_view_dim()` floats and `index` must be a valid duel.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_view(p: *mut VecEnv, index: i32, out: *mut f32) {
    let v = &*p;
    v.envs[index as usize].write_view(slice::from_raw_parts_mut(out, VIEW_DIM));
}

/// The first player of duel `index` loses `amount` health at once at the
/// start of the next server tick, knocked back along (dx, dz): a sudden
/// hit as `Params::drop_prob` deals them, at a moment of the caller's
/// choosing (for measuring what the agent does after one).
///
/// # Safety
/// `index` must be a valid duel.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_sudden(p: *mut VecEnv, index: i32, amount: f32, dx: f64, dz: f64) {
    let v = &mut *p;
    v.envs[index as usize].m.s.sudden = Some((0, amount, [dx, dz]));
}

/// Prototype counters of `fullheal` (see `fullheal::COUNTS`), into `out`
/// (`fullheal::NCOUNTS` numbers).
#[no_mangle]
pub extern "C" fn pvp_fullheal_counts(out: *mut u64) {
    for i in 0..crate::fullheal::NCOUNTS {
        unsafe { *out.add(i) = crate::fullheal::COUNTS[i].load(std::sync::atomic::Ordering::Relaxed) };
    }
}

/// Copy duel `src` of one vector over duel `dst` of another (or the same),
/// random streams included, for rollouts from the same moment.
/// # Safety
/// `from` and `to` must come from `pvp_vec_new`; the indices in range.
#[no_mangle]
pub unsafe extern "C" fn pvp_vec_clone_env(from: *const VecEnv, src: i32, to: *mut VecEnv, dst: i32) {
    let e = (&(*from).envs)[src as usize].clone();
    (&mut (*to).envs)[dst as usize] = e;
}
