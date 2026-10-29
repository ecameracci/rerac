//! **Port-only (Water Enhanced): the water sheet of the flow chutes.** The game's chutes (class 679,
//! `rc_game::moby_update::classes::flow`) carry Ratchet down their splines but draw nothing of their own: on Rilgar
//! and the other levels without strip water the chute floor shows bare. With Water: Enhanced, crate::water_render
//! draws a shallow sheet along every chute spline (kind 4 in `water.wgsl`), flowing at the chute's speed, on the
//! levels with clear water of their own (Rilgar, Batalia; Quartu has none: its chute is a dry metal slide, left
//! bare, `water_render::CHUTE_FX`). Drawing only: nothing here reaches the game state.
//!
//! **Shape.** The spline is resampled every [`ROW_STEP`]; each row probes the world collision straight down at
//! [`COL_STEP`] across the flow (up to [`HALF_WIDTH`] each side, stopping at a wall or a [`STEP`] up: a row never
//! climbs out of a chute sunk in a floor onto that floor; a row with floor out to [`HALF_WIDTH`] on both sides is
//! open ground, not the chute, and is left out). A row spans the chute's bed
//! between the walls: level across at [`DEPTH`] above the row's lowest floor (the levels smoothed along the chute), and
//! where the bed rises above that a film [`FILM`] over it (over the higher neighbouring sample, so the curved bed does
//! not show through between the samples). The sheet fades out over [`FADE`] columns / rows at its
//! edges. Rows where the level has water already (Novalis's strips, Batalia's river) and rows without floor are left
//! out.

use rc_formats::gameplay;

/// Row spacing along the spline, column spacing across it, the widest half-width probed (world units).
pub const ROW_STEP: f32 = 0.5;
pub const COL_STEP: f32 = 0.25;
pub const HALF_WIDTH: f32 = 5.0;
/// The water's depth over the lowest floor of a row.
pub const DEPTH: f32 = 0.25;
/// The film over a bed above the water level.
pub const FILM: f32 = 0.08;
/// Columns / rows over which the sheet fades in from its edges.
pub const FADE: f32 = 2.0;
/// The chute class (`flow::CLASSES`).
const FLOW_CLASS: i32 = 679;
/// A floor this far over the row's start ends the row's walk outward (a wall, not the chute's bed).
const WALL_RISE: f32 = 1.0;
/// A floor this far over the previous column's ends it too: a step, not the bed's curve (Rilgar's slide starts sunk
/// 0.8 under the room's floor).
const STEP: f32 = 0.35;

/// One chute spline: its points and the flow speed (u/s).
#[derive(Clone, Debug, PartialEq)]
pub struct ChuteSpline {
    pub points: Vec<[f32; 3]>,
    pub speed: f32,
}

/// The splines of every active class-679 instance (pvars +0x00.. the spline indices, +0x70 their count, +0x60 the
/// speed per tick; level 1 recomputes it by the length left, `flow::update` step 2: its middle value 5 u/s here).
pub fn chutes(gameplay_file: &[u8], level: u32) -> Vec<ChuteSpline> {
    let (Ok(instances), Ok(pvars), Ok(splines)) =
        (gameplay::parse_moby_instances(gameplay_file), gameplay::parse_pvars(gameplay_file), gameplay::parse_splines(gameplay_file))
    else {
        return Vec::new();
    };
    let word = |pv: &[u8], at: usize| pv.get(at..at + 4).map_or(0, |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let mut out = Vec::new();
    for m in instances.iter().filter(|m| m.o_class == FLOW_CLASS) {
        let Some(pv) = m.pvar(&pvars).filter(|pv| pv.len() >= 0x74) else { continue };
        let speed = if level == 1 { 5.0 } else { f32::from_bits(word(pv, 0x60) as u32) * 60.0 };
        for k in 0..word(pv, 0x70).clamp(0, 0x18) as usize {
            let Some(s) = usize::try_from(word(pv, 4 * k)).ok().and_then(|i| splines.get(i)) else { continue };
            let points: Vec<[f32; 3]> = s.iter().map(|p| [p[0], p[1], p[2]]).collect();
            if points.len() >= 2 { out.push(ChuteSpline { points, speed: speed.max(1.0) }); }
        }
    }
    out
}

/// A sheet mesh: game-space positions, per vertex (across, along, flow dir x, flow dir y) and the edge fade (0 at
/// the edge .. 1), triangle indices.
#[derive(Debug, Default)]
pub struct Sheet {
    pub pos: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 4]>,
    pub fade: Vec<f32>,
    pub idx: Vec<u32>,
}

struct Row {
    at: [f32; 3],
    along: f32,
    dir: [f32; 2],
    level: f32,
    /// Columns of the sheet (inclusive, in [`COL_STEP`]s from the spline) and their floors.
    cols: (i32, i32),
    floor: Vec<f32>,
}

/// The point at arc length `s` (xyz chords).
fn point_at(pts: &[[f32; 3]], cum: &[f32], s: f32) -> [f32; 3] {
    let k = cum.partition_point(|&c| c <= s).clamp(1, pts.len() - 1);
    let (a, b) = (pts[k - 1], pts[k]);
    let t = ((s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-6)).clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The sheet of one chute. `floor(top, bottom)` is the first world hit's z on the line down; `covered(p)` is true
/// where the level already draws water.
pub fn sheet(c: &ChuteSpline, floor: impl Fn([f32; 3], [f32; 3]) -> Option<f32>, covered: impl Fn([f32; 3]) -> bool) -> Sheet {
    let pts = &c.points;
    let mut cum = vec![0.0f32];
    for w in pts.windows(2) {
        let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2) + (w[1][2] - w[0][2]).powi(2)).sqrt();
        cum.push(cum.last().copied().unwrap_or(0.0) + d);
    }
    let total = cum.last().copied().unwrap_or(0.0);
    let n_cols = (HALF_WIDTH / COL_STEP) as i32;
    let mut rows: Vec<Option<Row>> = Vec::new();
    for r in 0..=(total / ROW_STEP) as usize {
        let s = r as f32 * ROW_STEP;
        let at = point_at(pts, &cum, s);
        let (a, b) = (point_at(pts, &cum, (s - 1.0).max(0.0)), point_at(pts, &cum, (s + 1.0).min(total)));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt();
        if l < 1e-3 || covered(at) {
            rows.push(None);
            continue;
        }
        let dir = [dx / l, dy / l];
        let Some(zc) = floor([at[0], at[1], at[2] + 1.5], [at[0], at[1], at[2] - 4.0]) else {
            rows.push(None);
            continue;
        };
        let probe = |k: i32| {
            let (x, y) = (at[0] - dir[1] * k as f32 * COL_STEP, at[1] + dir[0] * k as f32 * COL_STEP);
            floor([x, y, zc + WALL_RISE], [x, y, zc - 2.0])
        };
        let z: Vec<Option<f32>> = (-n_cols..=n_cols).map(probe).collect();
        let zi = |k: i32| z[(k + n_cols) as usize];
        // The bed: out from the spline until a wall, a step up (the rim of a chute sunk in a floor) or no floor.
        let bed = |k: i32| zi(k).filter(|&h| h < zc + WALL_RISE - 0.05);
        let next = |from: i32, k: i32| bed(k).filter(|&h| bed(from).is_none_or(|p| h - p <= STEP));
        let (mut lo, mut hi) = (0, 0);
        while lo > -n_cols && next(lo, lo - 1).is_some() { lo -= 1; }
        while hi < n_cols && next(hi, hi + 1).is_some() { hi += 1; }
        // Floor out to the widest probe on both sides: open ground (a room's floor, a pool's bed), not a chute.
        if lo == -n_cols && hi == n_cols {
            rows.push(None);
            continue;
        }
        let low = (lo..=hi).filter_map(bed).fold(zc, f32::min);
        let floor = (lo..=hi).map(|k| bed(k).unwrap_or(low)).collect();
        rows.push(Some(Row { at, along: s, dir, level: low + DEPTH, cols: (lo, hi), floor }));
    }
    // Two passes of a three-row average of the levels (rows without floor break the run).
    for _ in 0..2 {
        let levels: Vec<Option<f32>> = rows.iter().map(|r| r.as_ref().map(|r| r.level)).collect();
        for (i, r) in rows.iter_mut().enumerate() {
            let Some(r) = r.as_mut() else { continue };
            let near: Vec<f32> = [i.checked_sub(1), Some(i), Some(i + 1)].into_iter().flatten().filter_map(|j| levels.get(j).copied().flatten()).collect();
            r.level = near.iter().sum::<f32>() / near.len() as f32;
        }
    }
    // Rows from the nearest end of their run (a missing row or the spline's end).
    let mut run = vec![0usize; rows.len()];
    let mut n = 0;
    for (i, r) in rows.iter().enumerate() {
        n = if r.is_some() { n + 1 } else { 0 };
        run[i] = n;
    }
    n = 0;
    for i in (0..rows.len()).rev() {
        n = if rows[i].is_some() { n + 1 } else { 0 };
        run[i] = run[i].min(n);
    }
    let mut out = Sheet::default();
    let mut first: Vec<Option<u32>> = Vec::with_capacity(rows.len());
    for (r, &from_end) in rows.iter().zip(&run) {
        let Some(r) = r else { first.push(None); continue };
        first.push(Some(out.pos.len() as u32));
        let row_fade = ((from_end as f32 - 1.0) / FADE).clamp(0.0, 1.0);
        for (j, k) in (r.cols.0..=r.cols.1).enumerate() {
            let (x, y) = (r.at[0] - r.dir[1] * k as f32 * COL_STEP, r.at[1] + r.dir[0] * k as f32 * COL_STEP);
            // Over the higher neighbour too: the bed curves up between the samples.
            let h = r.floor[j.saturating_sub(1)..(j + 2).min(r.floor.len())].iter().copied().fold(f32::MIN, f32::max);
            out.pos.push([x, y, r.level.max(h + FILM)]);
            out.uv.push([k as f32 * COL_STEP, r.along, r.dir[0], r.dir[1]]);
            out.fade.push((((k - r.cols.0).min(r.cols.1 - k)) as f32 / FADE).min(1.0) * row_fade);
        }
    }
    for i in 1..rows.len() {
        let (Some(a), Some(b), Some(fa), Some(fb)) = (rows[i - 1].as_ref(), rows[i].as_ref(), first[i - 1], first[i]) else { continue };
        let v = |row: &Row, f: u32, k: i32| f + (k - row.cols.0) as u32;
        for k in a.cols.0.max(b.cols.0)..a.cols.1.min(b.cols.1) {
            let (p, q, r, s) = (v(a, fa, k), v(a, fa, k + 1), v(b, fb, k), v(b, fb, k + 1));
            out.idx.extend([p, q, r, q, s, r]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight V trough along +x, sloping down: floor z = 10 − 0.25·x + |y|/2, walls (z + 3) beyond |y| = 2.
    fn trough(top: [f32; 3], bottom: [f32; 3]) -> Option<f32> {
        let (x, y) = (top[0], top[1].abs());
        let z = 10.0 - 0.25 * x + if y > 2.0 { 3.0 } else { y / 2.0 };
        (bottom[2] <= z && z <= top[2]).then_some(z)
    }

    fn line() -> ChuteSpline { ChuteSpline { points: (0..=10).map(|k| [2.0 * k as f32, 0.0, 10.3 - 0.5 * k as f32]).collect(), speed: 7.0 } }

    /// The sheet fills the trough between its walls: level at the bed + DEPTH, a FILM over the higher bed, faded at
    /// the walls; flowing along +x.
    #[test]
    fn sheet_fills_the_trough() {
        let s = sheet(&line(), trough, |_| false);
        assert!(!s.idx.is_empty());
        let row: Vec<(&[f32; 3], &[f32; 4], f32)> = (0..s.pos.len()).filter(|&i| s.uv[i][1] == 5.0).map(|i| (&s.pos[i], &s.uv[i], s.fade[i])).collect();
        // The bed while |y|/2 is under the wall rise: |y| < 1.9, columns −7..7.
        assert_eq!(row.len(), 15, "{row:?}");
        for (p, u, fade) in &row {
            let bed = 10.0 - 0.25 * p[0];
            let k = (u[0] / COL_STEP).abs();
            let higher = (k + 1.0).min(7.0) * COL_STEP;
            let want = (bed + DEPTH).max(bed + higher / 2.0 + FILM);
            assert!((p[2] - want).abs() < 0.02, "{p:?} {want}");
            let from_wall = 7.0 - (u[0] / COL_STEP).abs();
            assert!((fade - (from_wall / FADE).min(1.0)).abs() < 1e-4, "{u:?} {fade}");
        }
        assert!(s.uv.iter().all(|u| (u[2] - 1.0).abs() < 1e-4 && u[3].abs() < 1e-4));
        assert_eq!(s.idx.len() % 3, 0);
        assert!(s.idx.iter().all(|&i| (i as usize) < s.pos.len()));
    }

    /// Covered rows and rows without floor are dropped, and no quad bridges them.
    #[test]
    fn covered_rows_are_skipped() {
        let full = sheet(&line(), trough, |_| false);
        let part = sheet(&line(), trough, |p| p[0] > 10.0);
        assert!(part.pos.iter().all(|p| p[0] <= 10.0 + 1e-3));
        assert!(part.idx.len() < full.idx.len());
        let none = sheet(&line(), |_, _| None, |_| false);
        assert!(none.pos.is_empty() && none.idx.is_empty());
    }

    /// A chute sunk 0.8 into a floor (under the wall rise): the sheet stops at the rim and never covers the floor;
    /// rows on open ground (floor out to the widest probe on both sides) are left out.
    #[test]
    fn sheet_stays_in_a_sunk_chute() {
        let flat = ChuteSpline { points: (0..=5).map(|k| [2.0 * k as f32, 0.0, 10.0]).collect(), speed: 5.0 };
        let sunk = |top: [f32; 3], bottom: [f32; 3]| {
            let z = if top[1].abs() < 1.4 { 10.0 } else { 10.8 };
            (bottom[2] <= z && z <= top[2]).then_some(z)
        };
        let s = sheet(&flat, sunk, |_| false);
        assert!(!s.idx.is_empty());
        assert!(s.pos.iter().all(|p| p[1].abs() < 1.4 && p[2] < 10.5), "{:?}", s.pos);
        let open = sheet(&flat, |top: [f32; 3], bottom: [f32; 3]| (bottom[2] <= 10.0 && 10.0 <= top[2]).then_some(10.0), |_| false);
        assert!(open.pos.is_empty() && open.idx.is_empty());
    }
}
