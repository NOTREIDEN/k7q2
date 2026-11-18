//! C interface used by the Python trainer through `ctypes`.
use crate::env::{PARAM_CNAMES, STAT_NAMES, VecEnv, ACT_DIMS, ACT_HEADS, OBS_DIM, PRIV_DIM, STATS_DIM};
use std::slice;

#[no_mangle]
pub extern "C" fn cs_obs_dim() -> i32 {
    OBS_DIM as i32
}

#[no_mangle]
pub extern "C" fn cs_priv_dim() -> i32 {
    PRIV_DIM as i32
}

#[no_mangle]
pub extern "C" fn cs_stats_dim() -> i32 {
    STATS_DIM as i32
}

#[no_mangle]
pub extern "C" fn cs_act_heads() -> i32 {
    ACT_HEADS as i32
}

/// # Safety
/// `out` must point to `cs_act_heads()` writable ints.
#[no_mangle]
pub unsafe extern "C" fn cs_act_dims(out: *mut i32) {
    slice::from_raw_parts_mut(out, ACT_HEADS).copy_from_slice(&ACT_DIMS);
}

#[no_mangle]
pub extern "C" fn cs_vec_new(n: i32, seed: u64) -> *mut VecEnv {
    Box::into_raw(Box::new(VecEnv::new(n.max(1) as usize, seed)))
}

/// # Safety
/// `p` must come from `cs_vec_new` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_free(p: *mut VecEnv) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

/// # Safety
/// `p` must be a live pointer from `cs_vec_new`.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_set_param(p: *mut VecEnv, id: i32, value: f64) -> i32 {
    (*p).set_param(id, value) as i32
}

/// # Safety
/// `obs` and `privs` must hold n*obs_dim and n*priv_dim floats.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_reset(p: *mut VecEnv, obs: *mut f32, privs: *mut f32) {
    let v = &mut *p;
    let n = v.envs.len();
    v.reset(slice::from_raw_parts_mut(obs, n * OBS_DIM), slice::from_raw_parts_mut(privs, n * PRIV_DIM));
}

/// # Safety
/// Every buffer must hold one row per environment of the matching width.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_step(p: *mut VecEnv, actions: *const i32, obs: *mut f32, privs: *mut f32, rew: *mut f32, done: *mut u8, fobs: *mut f32, fpriv: *mut f32) {
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
    );
}

/// # Safety
/// `out` must hold `cs_stats_dim()` doubles.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_take_stats(p: *mut VecEnv, out: *mut f64) {
    (*p).take_stats(slice::from_raw_parts_mut(out, STATS_DIM));
}

#[no_mangle]
pub extern "C" fn cs_param_count() -> i32 {
    PARAM_CNAMES.len() as i32
}

/// Name of parameter `i`, a NUL-terminated string that lives forever.
#[no_mangle]
pub extern "C" fn cs_param_name(i: i32) -> *const u8 {
    PARAM_CNAMES[i as usize].as_ptr()
}

#[no_mangle]
pub extern "C" fn cs_stat_count() -> i32 {
    STAT_NAMES.len() as i32
}

/// # Safety
/// `out` must hold n*act_heads ints.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_teacher(p: *mut VecEnv, out: *mut i32) {
    let v = &mut *p;
    let n = v.envs.len();
    v.teacher(slice::from_raw_parts_mut(out, n * ACT_HEADS));
}

/// Environment `i`'s statistics so far (kept on, not taken).
///
/// # Safety
/// `out` must hold `cs_stats_dim()` doubles.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_env_stats(p: *mut VecEnv, i: i32, out: *mut f64) {
    let v = &*p;
    slice::from_raw_parts_mut(out, STATS_DIM).copy_from_slice(&v.envs[i as usize].stats);
}

/// The level of environment `i`'s next episode: the seed its map, chests,
/// mobs and server settings are drawn from.
///
/// # Safety
/// `p` must come from `cs_vec_new`.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_set_next_seed(p: *mut VecEnv, i: i32, seed: u64) {
    let v = &mut *p;
    v.envs[i as usize].next_seed = Some(seed);
}

/// Writes environment `i`'s map (`what` 0) or this tick's state (`what` 1)
/// as JSON into `buf`, and returns its length; if `cap` is too small,
/// nothing is written and the length needed is returned.
///
/// # Safety
/// `buf` must hold `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn cs_vec_json(p: *mut VecEnv, i: i32, what: i32, buf: *mut u8, cap: i64) -> i64 {
    let v = &*p;
    let e = &v.envs[i as usize];
    let s = if what == 0 { e.map_json() } else { e.frame_json() };
    let n = s.len() as i64;
    if n <= cap {
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
    }
    n
}
