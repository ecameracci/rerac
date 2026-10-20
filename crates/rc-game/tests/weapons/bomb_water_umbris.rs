//! The Bomb Glove in Umbris's Hydrodisplacer pools (issue #36), headless: level 07 as the engine loads it (the class
//! table through `LevelPorts`, the water data, the item definitions), Ratchet placed in a pool, the glove in his hand,
//! one bomb tossed with ○. Each tick records the live bombs and, for a bomb that left the flight, what its last path
//! segment hits (`CollLine_Fix(old, new, 0x10, bomb)` against the tick's moby scene). Skipped without `extracted/`.
//!
//! The pools' water is the collision of the 902 / 941 … 971 patch mobys (`water::managers`, port 3: surface-0 faces
//! at the moby's z), not the world mesh (Umbris has no shallow world-mesh water). The bomb's hit test (level07
//! `0x2d72d0`, level01 `0x2c3990`) takes a hit with `CollType` 0 (level07 `0x21b3c0`, +0x1c only) for water whatever
//! +0x18 holds, so a moby's water face is water as a world one is.

use rc_formats::moby_anim::{parse_sequence, parse_sequences, MobyAnimClass, MobySequence};
use rc_formats::{collision, gadget, gameplay, moby_spawn};
use rc_game::hero::anim::RatchetAnim;
use rc_game::hero::items::{ItemClass, ItemData, ItemDef, HERO_LISTS};
use rc_game::hero::Hero;
use rc_game::moby_runtime::{mode, ClassInfo, MobyTable, Seq0Info};
use rc_game::moby_update::classes::bomb;
use rc_game::moby_update::scheduler::{class_info, load_level_mobys, Scheduler};
use rc_game::moby_update::services::{pvar, SharedServices, World};
use rc_game::moby_update::{ClassTable, Services};
use rc_game::pad::{button, PadInput};
use rc_game::particles::Particles;
use rc_game::rng::Rng;
use rc_game::tick::{Game, GameOptions, TickHooks};
use rc_game::water::world::WaterWorld;
use std::sync::Arc;

const LEVEL: u32 = 7;

struct CellHits<'a, 'b> {
    svc: &'a std::cell::RefCell<&'b mut Services>,
    classes: &'a ClassTable,
    coll: &'a collision::Collision,
}

impl rc_game::hero::items::HitSink for CellHits<'_, '_> {
    fn sphere(&mut self, table: &mut MobyTable, r: rc_game::ps2v::Pf, centre: rc_game::hero::physics::V4, flags: u32, ignore: Option<usize>, tmpl: &rc_game::moby_update::services::HitTemplate) -> Option<usize> {
        let mut s = self.svc.borrow_mut();
        rc_game::moby_update::services::ServiceHits { svc: &mut s, classes: self.classes, coll: Some(self.coll) }.sphere(table, r, centre, flags, ignore, tmpl)
    }
    fn line(&mut self, table: &mut MobyTable, a: rc_game::hero::physics::V4, b: rc_game::hero::physics::V4, flags: u32, ignore: Option<usize>, tmpl: &rc_game::moby_update::services::HitTemplate) -> Option<Option<usize>> {
        let mut s = self.svc.borrow_mut();
        rc_game::moby_update::services::ServiceHits { svc: &mut s, classes: self.classes, coll: Some(self.coll) }.line(table, a, b, flags, ignore, tmpl)
    }
    fn deliver(&mut self, table: &mut MobyTable, target: usize, tmpl: &rc_game::moby_update::services::HitTemplate) {
        let mut s = self.svc.borrow_mut();
        rc_game::moby_update::services::deliver_hit_in(table, &mut s.hits, target, tmpl);
    }
    fn create_moby(&mut self, table: &mut MobyTable, o_class: i16, counter: u64) -> Option<usize> {
        let mut s = self.svc.borrow_mut();
        bomb::create_from_hero(table, &mut s, self.classes, o_class, counter)
    }
    fn delete_moby(&mut self, table: &mut MobyTable, id: usize, counter: u64) {
        let mut s = self.svc.borrow_mut();
        bomb::delete_from_hero(table, &mut s, id, counter);
    }
}

/// The level's item definitions (the overlay's table), gadget classes and Ratchet's joint chains, as the engine
/// builds them.
fn items(core: &rc_formats::test_data::Core, ratchet_blob: &[u8], ratchet: &rc_formats::moby::MobyClass) -> ItemData {
    let root = rc_formats::test_data::root();
    let elf = std::fs::read(root.join("boot/SCUS_971.99")).expect("boot ELF");
    let ov_bytes = std::fs::read(rc_formats::test_data::level_dir(LEVEL).join("overlay.bin")).unwrap();
    let tables = rc_formats::save_game::ItemTables::load(&elf, &ov_bytes).unwrap();
    let sections = rc_formats::font::parse_overlay_sections(&ov_bytes).unwrap();
    let (n, sz) = (rc_formats::save_game::ITEM_COUNT, rc_formats::save_game::ITEM_DEF_SIZE);
    let raw = rc_formats::font::read_overlay(&sections, tables.item_defs_addr, n * sz).expect("item definitions");
    let w = |i: usize, o: usize| i32::from_le_bytes(raw[i * sz + o..i * sz + o + 4].try_into().unwrap());
    let defs = (0..n).map(|i| ItemDef { slot: w(i, 8), attach: w(i, 0xc), o_class: w(i, 0x10), b18: raw[i * sz + 0x18] }).collect();
    let hero_chains = HERO_LISTS.iter().map(|&l| gadget::joint_list(ratchet_blob, &ratchet.header, l).map(|(a, _)| a).unwrap_or_default()).collect();
    let classes = gadget::parse_gadget_classes(&core.core, &core.data)
        .unwrap()
        .iter()
        .map(|g| {
            let c = &g.moby.class;
            let chains = (0..16).map_while(|l| gadget::joint_list(&g.blob, &c.header, l).ok().map(|(a, _)| a)).collect();
            ItemClass { o_class: g.moby.o_class as i16, anim: MobyAnimClass::new(c, parse_sequences(&g.blob, c).unwrap_or_default()), scale: c.header.scale, chains }
        })
        .collect();
    ItemData { defs, hero_chains, classes }
}

/// What a bomb's last path segment met: the moby (index, class) or the world, the CollOutput kind and surface.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Met {
    moby: Option<(usize, i16)>,
    kind: i32,
    surface: i32,
    point: [f32; 3],
}

/// One tick: Ratchet's state, position and wading flag; the live bombs (id, state, position, +0x68 water ticks).
#[derive(Clone, Debug, PartialEq)]
struct Row {
    state: i32,
    pos: [f32; 3],
    wading: u8,
    bombs: Vec<(usize, u8, [f32; 3], i16)>,
    /// The bombs flying into this tick: (id, segment start, vz, what the segment met).
    segs: Vec<(usize, [f32; 3], f32, Option<Met>)>,
}

/// The pool managers' classes (port 3, level07 `0x30c508`).
fn pool_classes() -> &'static [i16] { rc_game::water::managers::PORTS.iter().find(|p| p.func == 0x30_c508).unwrap().classes }

/// Umbris with its mobys and water, Ratchet at `at` facing `yaw`, the Bomb Glove requested into the hand (10 bombs),
/// ○ pressed at tick [`THROW`]; `ticks` ticks. `drained`: every pool moby starts at its drained height (+0x04), as
/// after the Hydrodisplacer or the start-drained pools' first second.
fn run(at: [f32; 3], yaw: f32, drained: bool, ticks: u32) -> Option<Vec<Row>> {
    let core = rc_formats::test_data::core(LEVEL)?;
    let gp = rc_formats::test_data::gameplay(LEVEL)?;
    let ports = crate::common::ports(LEVEL, &[])?;
    let settings = rc_formats::test_data::gameplay_section(LEVEL, "level_settings")?;
    let mesh = collision::parse_collision(&core.core, &core.data).unwrap();
    let instances = gameplay::parse_moby_instances(&gp).unwrap();
    let tests = moby_spawn::loader_spawns(&instances, &mut moby_spawn::SpawnSave::default());
    let ok: Vec<bool> = tests.iter().map(|t| t.spawn).collect();
    let pvars = gameplay::parse_pvars_spawned(&gp, &ok).unwrap();
    let splines = gameplay::parse_splines(&gp).unwrap();
    let rd = |o: usize| i32::from_le_bytes(gp[o..o + 4].try_into().unwrap());
    let spawnable = rd(rd(0x44) as usize + 4).max(1) as usize;
    let death_z = f32::from_le_bytes(settings[0x28..0x2c].try_into().unwrap());
    let mut classes = ClassTable::default();
    let mut spheres = std::collections::HashMap::new();
    for (slot, e) in core.core.moby_classes.iter().enumerate() {
        let oc = e.o_class as i16;
        let parsed = core.block(&format!("moby_class/{:04}", e.o_class)).and_then(|b| rc_formats::moby::parse_moby_class(b).ok().map(|c| (b, c)));
        if let Some((blob, c)) = parsed {
            let anim = MobyAnimClass::new(&c, parse_sequences(blob, &c).unwrap_or_default());
            let mut info = class_info(&c, slot as u8, ports.update_fn(oc));
            info.seq0 = anim.sequence(0).map(|q| Seq0Info { frame_count: q.header.frame_count, loop_sound_bit7: q.header.loop_sound & 0x80 != 0 });
            spheres.insert(oc, c.header.bsphere);
            classes.classes.insert(oc, (info, Some(anim)));
        } else {
            let info = ClassInfo { slot: slot as u8, no_header: true, update_fn: ports.update_fn(oc), ..Default::default() };
            classes.classes.entry(oc).or_insert((info, None));
        }
    }
    let ratchet_blob = core.block("moby_class/0000")?.to_vec();
    let rclass = rc_formats::moby::parse_moby_class(&ratchet_blob).unwrap();
    let seqs: Vec<Option<MobySequence>> = (0..256).map(|i| core.block(&format!("ratchet_seq/{i:03}")).and_then(|b| parse_sequence(b, 0).ok())).collect();
    let ratchet = MobyAnimClass::new(&rclass, seqs);
    let items = items(&core, &ratchet_blob, &rclass);
    let classes = Arc::new(classes);
    let mut ct = ClassTable { classes: classes.classes.clone() };
    let statics = load_level_mobys(&instances, &mut ct, &pvars, &tests);
    let hero_idx = statics.mobys.iter().position(|m| m.o_class == 0)?;
    let mut table = MobyTable::new(statics.mobys.clone(), spawnable);
    table.mobys[hero_idx].mode |= mode::NO_UPDATE;
    table.mobys[hero_idx].position = [at[0], at[1], at[2], 1.0];
    table.mobys[hero_idx].rotation = [0.0, 0.0, yaw, 0.0];
    if drained {
        for m in table.mobys.iter_mut().filter(|m| pool_classes().contains(&m.o_class) && m.pvars.len() >= 8) { 
            m.position[2] = pvar::ff(&m.pvars, 4);
            pvar::set_i32(&mut m.pvars, 8, 0);
        }
    }
    let mut game = Game::new(&mesh, table, hero_idx, GameOptions::default(), death_z);
    game.hero.idle.level = LEVEL as i32;
    game.item_data = Some(items);
    game.item_globals.wrench_flag = 1;
    game.hero.grant_items(&[10]);
    game.hero.weapons.uses_ammo[10] = true;
    game.hero.weapons.ammo[10] = 10;
    game.item_globals.request = 10;
    game.item_globals.saved = 10;
    let mut svc = Services::new();
    svc.level = LEVEL;
    svc.set_splines(&splines);
    svc.pvar_shared = gameplay::parse_pvar_shared_data(&gp).unwrap();
    svc.groups = statics.groups(&gp);
    svc.set_moby_collision(rc_formats::moby_collision::parse_level(&core.core, &core.data).unwrap());
    svc.set_volumes(rc_formats::volumes::parse_volumes(&gp).unwrap());
    svc.build_grid(&mut game.mobys);
    svc.creatures.class_spheres = spheres;
    svc.water = WaterWorld::new(crate::common::water_data(LEVEL)?);
    let mut particles = Particles::new(None, Vec::new());
    let mut sched = Scheduler::new();
    {
        let hero: Hero = game.hero.clone();
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &*classes, &mut svc, 0);
        w.camera = game.camera.out.pos;
        w.coll = Some(&mesh);
        w.particles = Some(&mut particles);
        sched.load_pass(&mut w);
    }
    game.finish_load();
    let mut anim = RatchetAnim::new(&ratchet);
    let svc_cell = std::cell::RefCell::new(&mut svc);
    let parts_cell = std::cell::RefCell::new(&mut particles);
    let mut rows = Vec::new();
    for t in 0..ticks {
        // The flying bombs' path segments this tick (position + velocity before the update), against the scene
        // before it.
        let segs = {
            let scene = svc_cell.borrow().hero_scene(&game.mobys, classes.clone(), None);
            let mobys = &game.mobys.mobys;
            mobys
                .iter()
                .enumerate()
                .filter(|(_, m)| m.o_class == bomb::BOMB_CLASS && m.state == bomb::FLYING)
                .map(|(id, m)| {
                    let v = pvar::v4f(&m.pvars, bomb::pv::VEL);
                    let p = [m.position[0], m.position[1], m.position[2]];
                    let to = [p[0] + v[0], p[1] + v[1], p[2] + v[2]];
                    let met = rc_game::collision_query::coll_line_m(&mesh, Some(&scene.scene()), p, to, rc_game::collision_query::QueryFlags(0x10), Some(id))
                        .map(|o| Met { moby: o.moby.map(|i| (i, mobys[i].o_class)), kind: o.kind, surface: o.surface_id(), point: o.point });
                    (id, p, v[2], met)
                })
                .collect::<Vec<_>>()
        };
        let pad = if (THROW..=THROW + 1).contains(&t) { PadInput::neutral().press(button::CIRCLE) } else { PadInput::neutral() };
        let classes_ref: &ClassTable = &classes;
        let mut mobys = |table: &mut MobyTable, hero: &Hero, rng: &mut Rng, cam: &rc_game::follow_camera::CameraView, coll: &collision::Collision, counter: u64| {
            let mut s = svc_cell.borrow_mut();
            let mut p = parts_cell.borrow_mut();
            let mut w = World::new(table, hero, rng, classes_ref, &mut s, counter);
            w.camera = cam.pos;
            w.coll = Some(coll);
            w.particles = Some(&mut **p);
            sched.tick(&mut w);
        };
        let mut parts = |_: &Hero, _: &rc_game::follow_camera::CameraView, rng: &mut Rng, _: u64| { parts_cell.borrow_mut().update_parts(rng); };
        let mut world = SharedServices { svc: &svc_cell, classes: classes.clone() };
        let mut hooks = TickHooks { mobys: &mut mobys, particles: &mut parts, world: Some(&mut world) };
        game.hero.idle.counter = game.counter as i32;
        let mut hits = CellHits { svc: &svc_cell, classes: &classes, coll: &mesh };
        let r = game.tick_with_hits(Some(&pad.bytes()), &mesh, &mut anim.ctl(&ratchet), &mut hooks, &mut hits);
        assert_eq!(r.hero, rc_game::hero::HeroTick::Ran, "hero stopped in state {:#x} at tick {t}", game.hero.state);
        let bombs = game
            .mobys
            .mobys
            .iter()
            .enumerate()
            .filter(|(_, m)| m.o_class == bomb::BOMB_CLASS && m.state < 0xfd)
            .map(|(i, m)| (i, m.state, [m.position[0], m.position[1], m.position[2]], pvar::i16(&m.pvars, bomb::pv::WATER)))
            .collect();
        let h = &game.hero;
        rows.push(Row { state: h.state, pos: h.position(), wading: h.f13f9, bombs, segs });
    }
    Some(rows)
}

const THROW: u32 = 60;

fn dump(name: &str, rows: &[Row]) {
    for (t, r) in rows.iter().enumerate() {
        if (r.bombs.iter().all(|b| b.1 == bomb::HELD) && r.segs.is_empty() && t % 20 != 0) || std::env::var("RC_WEAPONS_DUMP").is_err() { continue; }
        eprintln!("{name} {t:4} state {:#x} pos {:?} wading {} bombs {:?} segs {:?}", r.state, r.pos, r.wading, r.bombs, r.segs);
    }
}

/// Where a run's bomb went: the first flight segments, where it exploded (or the tick it entered the water).
struct Throw {
    before: Row,
    first: Vec<(usize, [f32; 3], f32, Option<Met>)>,
    flew: usize,
    boom: Option<[f32; 3]>,
    sank: bool,
}

fn throw(rows: &[Row]) -> Throw {
    let first = rows.iter().flat_map(|r| r.segs.iter().cloned()).take(4).collect();
    let flew = rows.iter().filter(|r| !r.segs.is_empty()).count();
    let boom = rows.iter().find_map(|r| r.bombs.iter().find(|b| b.1 == bomb::EXPLODING).map(|b| b.2));
    let sank = rows.iter().any(|r| r.bombs.iter().any(|b| b.3 > 0));
    Throw { before: rows[THROW as usize - 1].clone(), first, flew, boom, sank }
}

/// The pools of groups 0 and 1 (patch mobys 943 / 955), drained: Ratchet wades in water 0.5 deep, so the glove
/// releases the bomb 0.49 above his feet, just under the surface at 66.5. Its first path segment rises through the
/// pool moby's water face (`CollType` 0, kind 0x1000); the game's hit test (level07 0x2d72d0..0x2d745c, the same
/// code as level01 0x2c3990) lets it fly on. Before the fix the port took a moby's water face for a solid hit and
/// the bomb went off at Ratchet's feet (issue #36).
#[test]
fn drained_pool_bomb_rises_through_the_water() {
    if crate::common::overlay(LEVEL).is_none() { eprintln!("skipped: no extracted/"); return; }
    for (name, at, pool) in [("group 1", [317.7, 319.3, 72.0], 955i16), ("group 0", [255.6, 311.0, 72.0], 943)] {
        let rows = run(at, 0.0, true, 200).unwrap();
        dump(name, &rows);
        let t = throw(&rows);
        let feet = t.before.pos;
        eprintln!("{name}: Ratchet {feet:?} wading {}; first segments {:?}; flew {} ticks, exploded at {:?}, sank {}", t.before.wading, t.first, t.flew, t.boom, t.sank);
        assert_ne!(t.before.wading, 0, "{name}: Ratchet wades before the throw");
        let (_, start, vz, met) = t.first.first().cloned().expect("the bomb flew");
        let met = met.expect("the first segment meets the water");
        assert!(vz > 0.0 && met.surface == 0 && met.kind == 0x1000 && met.moby.is_some_and(|(_, c)| c == pool), "{name}: {met:?}");
        assert!(start[2] < met.point[2], "{name}: released under the surface");
        assert!(t.flew > 10, "{name}: the bomb flies on ({} ticks)", t.flew);
        let at = t.boom.expect("the bomb went off");
        let d = ((at[0] - feet[0]).powi(2) + (at[1] - feet[1]).powi(2)).sqrt();
        assert!(d > 3.0, "{name}: exploded {d} from Ratchet");
    }
}

/// The control: group 3's drained pool (patch mobys 902 / 968 …) is 0.42 deep, so the bomb leaves the glove above
/// its surface; its path meets nothing on the first ticks and it lands away from Ratchet (as before the fix).
#[test]
fn shallower_pool_bomb_starts_above_the_water() {
    if crate::common::overlay(LEVEL).is_none() { eprintln!("skipped: no extracted/"); return; }
    let rows = run([285.0, 364.7, 72.0], 0.0, true, 200).unwrap();
    dump("group 3", &rows);
    let t = throw(&rows);
    let feet = t.before.pos;
    eprintln!("group 3: Ratchet {feet:?} wading {}; first segments {:?}; flew {} ticks, exploded at {:?}", t.before.wading, t.first, t.flew, t.boom);
    assert_ne!(t.before.wading, 0, "Ratchet wades before the throw");
    assert!(t.first.len() == 4 && t.first.iter().all(|s| s.3.is_none()), "the first segments meet nothing: {:?}", t.first);
    let at = t.boom.expect("the bomb went off");
    assert!(((at[0] - feet[0]).powi(2) + (at[1] - feet[1]).powi(2)).sqrt() > 3.0, "exploded at {at:?}");
}

/// Survey: every pool group drained, Ratchet in it, one throw
/// (`cargo xtask test-job --test weapons --filter bomb_water_umbris::survey --ignored --nocapture`).
#[test]
#[ignore]
fn survey() {
    if crate::common::overlay(LEVEL).is_none() { eprintln!("skipped: no extracted/"); return; }
    let core = rc_formats::test_data::core(LEVEL).unwrap();
    let mesh = collision::parse_collision(&core.core, &core.data).unwrap();
    for (name, at) in [("g0", [255.6, 311.0, 72.0]), ("g1", [317.7, 319.3, 72.0]), ("g2", [327.0, 343.0, 72.0]), ("g3", [285.0, 364.7, 72.0]), ("g17", [277.8, 332.6, 72.0])] {
        let Some(rows) = run(at, 0.0, true, 200) else { return };
        let t = throw(&rows);
        eprintln!("{name}: Ratchet {:?} state {:#x} wading {}; first segments {:?}; flew {}, exploded at {:?}, sank {}", t.before.pos, t.before.state, t.before.wading, t.first, t.flew, t.boom, t.sank);
        // The world faces through the release point, top down.
        if let Some(&(_, p, _, _)) = t.first.first() {
            let mut z = p[2] + 3.0;
            while let Some(o) = rc_game::collision_query::coll_line_m(&mesh, None, [p[0], p[1], z], [p[0], p[1], p[2] - 2.0], rc_game::collision_query::QueryFlags(0x10), None) {
                eprintln!("    world face at z {} kind {:#x}", o.point[2], o.kind);
                z = o.point[2] - 0.001;
            }
        }
        dump(name, &rows);
    }
}
