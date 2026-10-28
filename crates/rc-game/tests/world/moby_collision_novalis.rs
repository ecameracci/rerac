//! Moby collision: Novalis (level 1) crate stacks after the load pass, a line cast onto a crate, a bolt dropped
//! above a crate settling on it, and a mine riding a Kerwan (level 3) train car. Skipped when `extracted/` (the
//! extracted game data) is absent.

use rc_formats::moby_anim::{parse_sequences, MobyAnimClass};
use rc_formats::moby_collision::MobyCollision;
use rc_formats::{collision, gameplay, level};
use rc_game::collision_query::QueryFlags;
use rc_game::hero::physics as ph;
use rc_game::moby_runtime::{mode, MobyTable, Seq0Info};
use rc_game::moby_update::classes::bolt;
use rc_game::moby_update::scheduler::{self, class_info, load_static_mobys, Groups, Scheduler};
use rc_game::moby_update::services::World;
use rc_game::moby_update::{ClassTable, Services};
use rc_game::ps2v::Pf;
use rc_game::tick::{Game, GameOptions};

struct Level {
    level: u32,
    mesh: collision::Collision,
    instances: Vec<gameplay::MobyInstance>,
    pvars: Vec<Option<Vec<u8>>>,
    gp: Vec<u8>,
    classes: ClassTable,
    spawnable: usize,
    blobs: Vec<(i32, MobyCollision)>,
}

fn load() -> Option<Level> { load_level(1) }

fn load_level(n: u32) -> Option<Level> {
    let dir = rc_formats::test_data::root().join(format!("levels/{n:02}"));
    let data = rc_formats::test_data::core_data(n)?;
    let idx = std::fs::read(dir.join("core_index.bin")).ok()?;
    let gp = rc_formats::test_data::gameplay(n)?;
    let core = level::parse_level_core(&idx, data.len()).unwrap();
    let mesh = collision::parse_collision(&core, &data).unwrap();
    let instances = gameplay::parse_moby_instances(&gp).unwrap();
    let pvars = gameplay::parse_pvars(&gp).unwrap();
    let rd = |o: usize| i32::from_le_bytes(gp[o..o + 4].try_into().unwrap());
    let spawnable = rd(rd(0x44) as usize + 4) as usize;
    let mut classes = ClassTable::default();
    for (slot, e) in core.moby_classes.iter().enumerate() {
        let oc = e.o_class as i16;
        let Some(blob) = rc_formats::test_data::core_block(n, &format!("moby_class/{:04}", e.o_class)) else { continue };
        let Ok(c) = rc_formats::moby::parse_moby_class(&blob) else { continue };
        let anim = MobyAnimClass::new(&c, parse_sequences(&blob, &c).unwrap_or_default());
        let mut info = class_info(&c, slot as u8, scheduler::port_update_fn(oc));
        info.seq0 = anim.sequence(0).map(|q| Seq0Info { frame_count: q.header.frame_count, loop_sound_bit7: q.header.loop_sound & 0x80 != 0 });
        classes.classes.insert(oc, (info, Some(anim)));
    }
    let blobs = rc_formats::moby_collision::parse_level(&core, &data).unwrap();
    Some(Level { level: n, mesh, instances, pvars, gp: gp.to_vec(), classes, spawnable, blobs })
}

/// The table after the loader, the load pass and `ticks` moby-loop ticks (camera at the 344 stack), with moby
/// collision (`with_mobys`) or without.
fn loaded(lv: &Level, with_mobys: bool, ticks: u64) -> (Game, Services, ClassTable) {
    let mut classes = ClassTable { classes: lv.classes.classes.clone() };
    let statics = load_static_mobys(&lv.instances, &mut classes, &lv.pvars);
    let hero_idx = statics.iter().position(|m| m.o_class == 0).expect("Ratchet");
    let mut table = MobyTable::new(statics, lv.spawnable);
    table.mobys[hero_idx].mode |= mode::NO_UPDATE;
    let mut game = Game::new(&lv.mesh, table, hero_idx, GameOptions::default(), -100.0);
    let mut svc = Services::new();
    svc.level = lv.level;
    svc.groups = Groups::parse(&lv.gp, &|i| (i < lv.instances.len()).then_some(i));
    if with_mobys { svc.set_moby_collision(lv.blobs.clone()); }
    svc.build_grid(&mut game.mobys);
    let hero = game.hero.clone();
    let cam = game.camera.out.pos;
    let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, 0);
    w.camera = cam;
    w.coll = Some(&lv.mesh);
    let mut sched = Scheduler::new();
    sched.load_pass(&mut w);
    let p = game.mobys.mobys[CRATE_344].position;
    let cam = [Pf::f(p[0]), Pf::f(p[1] - 4.0), Pf::f(p[2] + 2.0), Pf::ZERO];
    for counter in 1..=ticks {
        game.mobys.free_slot_pass(counter);
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, counter);
        w.camera = cam;
        w.coll = Some(&lv.mesh);
        sched.tick(&mut w);
    }
    (game, svc, classes)
}

const CRATE_344: usize = 344;
const CRATE_342: usize = 342;

/// Kerwan's train: a mine seeking from a car deck does not detonate on the deck it rides [L: the 0.35 sphere is
/// tangent to the car's mesh; the port skips the carrier it rides] and moves toward its target. Before the skip it
/// exploded on the first seek tick, usually outside the target's blast range.
#[test]
fn kerwan_train_mine_seeks_from_a_car_deck() {
    use rc_game::moby_update::classes::mine;
    let Some(lv) = load_level(3) else { eprintln!("skipped: no extracted/"); return };
    let (mut game, mut svc, classes) = loaded(&lv, true, 60);
    let car = game.mobys.mobys.iter().position(|m| m.o_class == 1210).expect("a car");
    let target = game.mobys.mobys.iter().enumerate()
        .find(|(i, m)| *i != car && m.mode & mode::TARGETABLE != 0 && m.state < 0x80)
        .map(|(i, _)| i)
        .expect("a targetable moby");
    let cpos = game.mobys.mobys[car].position;
    let hero = game.hero.clone();
    let deck = {
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, 61);
        w.coll = Some(&lv.mesh);
        w.coll_line(ph::from_f32x3([cpos[0], cpos[1], cpos[2] + 5.0]), ph::from_f32x3([cpos[0], cpos[1], cpos[2] - 5.0]), 2, None)
    }
    .expect("the car's deck");
    assert_eq!(deck.moby, Some(car));
    let mut rng = rc_game::rng::Rng::new();
    let m = {
        let hero = game.hero.clone();
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, 62);
        w.coll = Some(&lv.mesh);
        let info = classes.classes.get(&74).map(|c| &c.0);
        let id = w.table.create(74, info, 62).expect("mine");
        let at = [deck.point[0] + 0.1, deck.point[1] + 0.05, deck.point[2]];
        mine::init_held(&mut w.table.mobys[id], &mut rng, at, 0);
        mine::release(w.table, id, &mut rng, at, None);
        rc_game::moby_update::services::pvar::set_v4f(&mut w.table.mobys[id].pvars, mine::pv::VEL, [0.0; 4]);
        // Armed, landed and seeking from the deck (the search's own record of the platform and of a 600-tick life).
        w.table.mobys[id].state = mine::OUT | mine::ARMED | mine::LANDED | mine::SEEKING;
        let pv = &mut w.table.mobys[id].pvars;
        rc_game::moby_update::services::pvar::set_i32(pv, mine::pv::PLATFORM, car as i32 + 1);
        rc_game::moby_update::services::pvar::set_i32(pv, mine::pv::TARGET, target as i32 + 1);
        rc_game::moby_update::services::pvar::set_i32(pv, mine::pv::LIFE, 600);
        id
    };
    let dist = |g: &Game| {
        let (a, b) = (&g.mobys.mobys[m], &g.mobys.mobys[target]);
        ((a.position[0] - b.position[0]).powi(2) + (a.position[1] - b.position[1]).powi(2) + (a.position[2] - b.position[2]).powi(2)).sqrt()
    };
    let d0 = dist(&game);
    let mut sched = Scheduler::new();
    for counter in 63..=74u64 {
        game.mobys.free_slot_pass(counter);
        let hero = game.hero.clone();
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, counter);
        w.coll = Some(&lv.mesh);
        sched.tick(&mut w);
        assert!(!game.mobys.mobys[m].is_deleted(), "the mine does not detonate on the car deck at tick {counter}");
    }
    let d1 = dist(&game);
    assert!(d1 < d0 - 0.2, "it moves toward the target: {d0} -> {d1}");
}

/// Kerwan's train: a mine dropped on a car's deck records the carrier on its first landed tick and rides it.
/// The display bob's undo used to leave the "physical" z `amp/6` high (it used this tick's fade, not the one the
/// offset was added with), tripping `landed`'s ground-drop check (`DT2·40`): the mine fell and re-landed for a dozen
/// ticks, sitting still in world space while the train moved (the Metropolis train's slip).
#[test]
fn kerwan_train_mine_rides_a_car() {
    use rc_game::moby_update::classes::mine;
    let Some(lv) = load_level(3) else { eprintln!("skipped: no extracted/"); return };
    let (mut game, mut svc, classes) = loaded(&lv, true, 60);
    let car = game.mobys.mobys.iter().position(|m| m.o_class == 1210).expect("a car");
    let cpos = game.mobys.mobys[car].position;
    let hero = game.hero.clone();
    let deck = {
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, 61);
        w.coll = Some(&lv.mesh);
        w.coll_line(ph::from_f32x3([cpos[0], cpos[1], cpos[2] + 5.0]), ph::from_f32x3([cpos[0], cpos[1], cpos[2] - 5.0]), 2, None)
    }
    .expect("the car's deck");
    assert_eq!(deck.moby, Some(car), "the deck line hits the car");
    let mut sched = Scheduler::new();
    let mut rng = rc_game::rng::Rng::new();
    let mut id = None;
    let mut settled_at = None;
    let mut offsets: Vec<f32> = Vec::new();
    for counter in 62..=200u64 {
        game.mobys.free_slot_pass(counter);
        let hero = game.hero.clone();
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, counter);
        w.coll = Some(&lv.mesh);
        if counter == 62 {
            let info = classes.classes.get(&74).map(|c| &c.0);
            let m = w.table.create(74, info, counter).expect("mine");
            let at = [deck.point[0] + 0.1, deck.point[1] + 0.05, deck.point[2] + 1.0];
            mine::init_held(&mut w.table.mobys[m], &mut rng, at, 0);
            mine::release(w.table, m, &mut rng, at, None);
            rc_game::moby_update::services::pvar::set_v4f(&mut w.table.mobys[m].pvars, mine::pv::VEL, [0.0; 4]);
            id = Some(m);
        }
        sched.tick(&mut w);
        let plat = i32::from_le_bytes(game.mobys.mobys[id.unwrap()].pvars[0x4c..0x50].try_into().unwrap());
        if plat != 0 && settled_at.is_none() { settled_at = Some(counter); }
        if let Some(settled) = settled_at {
            // The carrier moves 0.05 a tick, with its block delta as the train's `drive` writes it.
            let c = &mut game.mobys.mobys[car];
            c.position[0] += 0.05;
            let rot = c.rotation;
            rc_game::moby_update::triggers::carry_riders(&mut c.pvars, 0x20, [0.05, 0.0, 0.0, 0.0], rot, rot);
            svc.build_matrix_in(&mut game.mobys, &classes, car);
            if counter <= settled + 30 {
                let m = &game.mobys.mobys[id.unwrap()];
                offsets.push(m.position[0] - game.mobys.mobys[car].position[0]);
            }
        }
    }
    let settled = settled_at.expect("the mine never recorded the car");
    // The drop takes ~21 ticks (62 to 83), the record the tick after. The bob mismatch delayed it by about 12.
    assert!(settled <= 86, "recorded on the landing tick ({settled})");
    let (lo, hi) = offsets.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    assert!(hi - lo < 0.02, "the mine rides the car: offset {lo}..{hi}");
}

#[test]
fn novalis_line_onto_crate_344() {
    let Some(lv) = load() else { eprintln!("skipped: no extracted/"); return };
    // After 120 ticks the stack has settled (crate 342 lands on 344 through the stacking probe, a moby line).
    let (mut game, mut svc, classes) = loaded(&lv, true, 120);
    let c500 = lv.blobs.iter().find(|(oc, _)| *oc == 500).map(|(_, b)| b).unwrap();
    let zs: Vec<i16> = c500.vertices.iter().map(|v| v[2]).collect();
    eprintln!("class 500 blob: {} vertices (z {:?}..{:?}), {} faces, prims {:?}", c500.vertices.len(), zs.iter().min(), zs.iter().max(), c500.faces.len(),
        c500.prims.iter().map(|p| (p.kind(), p.mask())).collect::<Vec<_>>());
    let (m344, m342) = (game.mobys.mobys[CRATE_344].clone(), game.mobys.mobys[CRATE_342].clone());
    assert_eq!((m344.o_class, m342.o_class), (500, 500));
    eprintln!("crate 344 at {:?} (instance z {}), crate 342 at {:?}; grid cells of 344 {:?}",
        &m344.position[..3], lv.instances[CRATE_344].position[2], &m342.position[..3], svc.grid.cells_of(CRATE_344 as u16));
    let hero = game.hero.clone();
    let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, 121);
    w.coll = Some(&lv.mesh);
    let (x, y) = (Pf::f(m344.position[0] + 0.1), Pf::f(m344.position[1] - 0.2));
    let (a, b) = ([x, y, Pf::f(75.0), Pf::ZERO], [x, y, Pf::f(55.0), Pf::ZERO]);
    // Down onto the stack: crate 342 (on top of 344) first.
    let h = w.coll_line(a, b, 2, None).expect("stack top");
    assert_eq!(h.moby, Some(CRATE_342));
    assert!(h.kind > 0x1000 && h.primitive.is_none(), "a mesh face, kind {:#x}", h.kind);
    // Ignoring 342: the top face of 344, at the height the stack snap put 342 at (344's top).
    let h = w.coll_line(a, b, 2, Some(CRATE_342)).expect("crate 344");
    eprintln!("line onto 344: {:?} kind {:#x} normal {:?}", h.point, h.kind, h.normal);
    assert_eq!(h.moby, Some(CRATE_344));
    assert_eq!(h.point[2], m344.position[2] + 1.0, "344's top face (class 500: z 0..24576 model units × scale)");
    assert_eq!(h.point[2], m342.position[2], "where crate 342 came to rest");
    assert_eq!([h.point[0], h.point[1]], [x.to_f32(), y.to_f32()]);
    // World-only (no scene), the same line reaches the ground below the stack.
    let g = rc_game::collision_query::coll_line(&lv.mesh, ph::to_f32x3(a), ph::to_f32x3(b), QueryFlags(2)).expect("ground");
    assert!(g.moby.is_none() && g.point[2] < m344.position[2] + 0.01, "{:?}", g.point);
    // coll_sphere_mobys at the stack's side lists both crates.
    let c = [Pf::f(m344.position[0] + 0.6), y, Pf::f(m344.position[2] + 1.0), Pf::ZERO];
    let n = w.sphere_mobys(Pf::f(0.3), c, 0x10, None, None);
    assert!(n >= 2, "{n} mobys");
}

/// Drops a bolt 1.5 above crate 342 (top of the 344 stack) and runs the moby loop: with moby collision it
/// settles on the crate; without, it falls through the stack.
fn drop_bolt(lv: &Level, with_mobys: bool) -> (u8, [f32; 3], f32, f32) {
    let (mut game, mut svc, classes) = loaded(lv, with_mobys, 120);
    let top = game.mobys.mobys[CRATE_342].position;
    let crate_top = top[2] + 1.0;
    let hero = game.hero.clone();
    let cam = [Pf::f(top[0]), Pf::f(top[1] - 4.0), Pf::f(top[2] + 2.0), Pf::ZERO];
    let mut sched = Scheduler::new();
    let mut id = None;
    for counter in 121..=360u64 {
        game.mobys.free_slot_pass(counter);
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &classes, &mut svc, counter);
        w.camera = cam;
        w.coll = Some(&lv.mesh);
        if counter == 121 {
            let pos = [Pf::f(top[0] + 0.1), Pf::f(top[1] + 0.05), Pf::f(crate_top + 1.5), Pf::ZERO];
            id = bolt::spawn(&mut w, CRATE_342, pos, [Pf::ZERO; 4], 0, 1, 0);
            w.mm(id.unwrap()).update_dist = 0xff;
        }
        sched.tick(&mut w);
    }
    let m = &game.mobys.mobys[id.unwrap()];
    let f = |o: usize| f32::from_le_bytes(m.pvars[o..o + 4].try_into().unwrap());
    // A bolt that settles on a carrier keeps its rest pose in the carrier's frame (+0x5c = the carrier + 1,
    // `FUN_00275528`): the crates are carriers (mode 0x20, block +0x60), so the world rest point is the local one
    // through the crate's rows (`FUN_002753b0`; triggers.md §5b).
    let on = i32::from_le_bytes(m.pvars[0x5c..0x60].try_into().unwrap());
    let rest = if on > 0 {
        assert_eq!(on as usize - 1, CRATE_342, "resting on the top crate");
        let k = rc_game::moby_update::triggers::carrier(&game.mobys.mobys[on as usize - 1]).expect("a carrier");
        let (mut l, mut lr) = ([f(0), f(4), f(8)], [f(0x10), f(0x14), f(0x18)]);
        rc_game::moby_update::triggers::from_local(&k, m.class_slot, &mut l, &mut lr).0[2]
    } else {
        f(8)
    };
    assert!(!with_mobys || on > 0, "with moby collision the bolt rests on the crate");
    assert_eq!(game.mobys.mobys[CRATE_342].position[2], top[2], "the stack did not move");
    (m.state, [m.position[0], m.position[1], m.position[2]], rest, crate_top)
}

#[test]
fn novalis_bolt_settles_on_a_crate() {
    let Some(lv) = load() else { eprintln!("skipped: no extracted/"); return };
    let (state, pos, rest, crate_top) = drop_bolt(&lv, true);
    eprintln!("with moby collision: bolt state {state} at {pos:?}, rest z {rest}, crate top {crate_top}");
    assert!(state == 2 || state == 3, "settled (state {state})");
    assert!((rest - crate_top).abs() < 0.01, "rest z {rest} on the crate top {crate_top}");
    let (state, pos, rest, crate_top) = drop_bolt(&lv, false);
    eprintln!("world only: bolt state {state} at {pos:?}, rest z {rest} (crate top {crate_top})");
    // World only, the stacking probe finds no crate either: the crates sink onto the ground and the bolt
    // passes through them to the ground.
    assert!(rest < crate_top - 0.9 || state == 4 || state >= 0x80, "falls through without moby collision");
}
