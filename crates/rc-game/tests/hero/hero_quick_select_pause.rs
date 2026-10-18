//! Port-only Quick Select: Modern (docs/plan/quick_select_pause.md), headless on Novalis: the level as the engine ticks
//! it (the moby loop, the particles, the moby collision, the hero with his hand items, the camera) and the quick-select
//! ring after every main-loop frame as crate::menu_render runs it (`QuickSelect::hero` / `update`, the pad lock, the hand
//! request through the session), with the engine's hold rule (`tick::quick_select_hold` → `Game::hold_tick`). The ring
//! script of docs/plan/player_controller.md §12 (`60-100:press TRIANGLE,70-100:stick 0 -1`: the Bomb Glove into the
//! hand). Classic ticks the world under the ring as today; Modern holds it while △ is held and resumes it after the
//! release, with the ring and the swap unchanged. Skipped when `extracted/` is absent.

use rc_formats::moby_anim::{parse_sequence, parse_sequences, MobyAnimClass, MobySequence};
use rc_formats::save_game::{ChunkTables, ItemTables, SaveGameLump};
use rc_formats::{collision, gadget, gameplay, level, moby_spawn};
use rc_game::game_state::{GameState, SessionState};
use rc_game::hero::anim::RatchetAnim;
use rc_game::hero::items::{ItemClass, ItemData, ItemDef, ItemGlobals, WeaponDef, HERO_LISTS};
use rc_game::hero::Hero;
use rc_game::menus::quick_select::{HeroGate, QuickSelect};
use rc_game::menus::{MenuInput, Overlay};
use rc_game::moby_runtime::{mode, MobyTable, Seq0Info};
use rc_game::moby_update::scheduler::{self, class_info, load_level_mobys, Scheduler};
use rc_game::moby_update::services::{ServiceHits, SharedServices, World};
use rc_game::moby_update::{ClassTable, Services};
use rc_game::pad::{button, PadInput};
use rc_game::particles::Particles;
use rc_game::rng::Rng;
use rc_game::tick::{quick_select_hold, Game, GameOptions, TickHooks};
use std::sync::Arc;

struct Lv {
    mesh: collision::Collision,
    instances: Vec<gameplay::MobyInstance>,
    pvars: Vec<Option<Vec<u8>>>,
    tests: Vec<moby_spawn::SpawnTest>,
    splines: Vec<Vec<[f32; 4]>>,
    gp: Vec<u8>,
    classes: ClassTable,
    spawnable: usize,
    death_z: f32,
    coll_blobs: Vec<(i32, rc_formats::moby_collision::MobyCollision)>,
    ratchet: MobyAnimClass,
    seconds: Vec<Vec<u8>>,
    items: ItemData,
    weapon_defs: Vec<WeaponDef>,
    tables: ItemTables,
    ring: QuickSelect,
    state: GameState,
    session: SessionState,
}

/// Novalis as `weapons/hero_gloves.rs` loads it, plus the ring from the overlay (`QuickSelect::load`).
fn load() -> Option<Lv> {
    let root = rc_formats::test_data::root();
    let dir = root.join("levels/01");
    let data = rc_formats::test_data::core_data(1)?;
    let idx = std::fs::read(dir.join("core_index.bin")).ok()?;
    let gp = rc_formats::test_data::gameplay(1)?;
    let settings = rc_formats::test_data::gameplay_section(1, "level_settings")?;
    let core = level::parse_level_core(&idx, data.len()).unwrap();
    let mesh = collision::parse_collision(&core, &data).unwrap();
    let instances = gameplay::parse_moby_instances(&gp).unwrap();
    let tests = moby_spawn::loader_spawns(&instances, &mut moby_spawn::SpawnSave::default());
    let spawned: Vec<bool> = tests.iter().map(|t| t.spawn).collect();
    let pvars = gameplay::parse_pvars_spawned(&gp, &spawned).unwrap();
    let splines = gameplay::parse_splines(&gp).unwrap();
    let rd = |o: usize| i32::from_le_bytes(gp[o..o + 4].try_into().unwrap());
    let spawnable = rd(rd(0x44) as usize + 4) as usize;
    let death_z = f32::from_le_bytes(settings[0x28..0x2c].try_into().unwrap());
    let mut classes = ClassTable::default();
    for (slot, e) in core.moby_classes.iter().enumerate() {
        let oc = e.o_class as i16;
        let parsed = rc_formats::test_data::core_block(1, &format!("moby_class/{:04}", e.o_class)).and_then(|b| rc_formats::moby::parse_moby_class(&b).ok().map(|c| (b, c)));
        if let Some((blob, c)) = parsed {
            let anim = MobyAnimClass::new(&c, parse_sequences(&blob, &c).unwrap_or_default());
            let mut info = class_info(&c, slot as u8, scheduler::port_update_fn(oc));
            info.seq0 = anim.sequence(0).map(|q| Seq0Info { frame_count: q.header.frame_count, loop_sound_bit7: q.header.loop_sound & 0x80 != 0 });
            classes.classes.insert(oc, (info, Some(anim)));
        } else {
            let info = rc_game::moby_runtime::ClassInfo { slot: slot as u8, no_header: true, update_fn: scheduler::port_update_fn(oc), ..Default::default() };
            classes.classes.entry(oc).or_insert((info, None));
        }
    }
    let coll_blobs = rc_formats::moby_collision::parse_level(&core, &data).unwrap();
    let ratchet_blob = rc_formats::test_data::core_block(1, "moby_class/0000")?;
    let class = rc_formats::moby::parse_moby_class(&ratchet_blob).unwrap();
    let seqs: Vec<Option<MobySequence>> = (0..256)
        .map(|i| rc_formats::test_data::core_block(1, &format!("ratchet_seq/{i:03}")).and_then(|b| parse_sequence(&b, 0).ok()))
        .collect();
    let ratchet = MobyAnimClass::new(&class, seqs);
    let seconds = (0..64).map(|l| gadget::joint_list(&ratchet_blob, &class.header, l).map(|(_, b)| b).unwrap_or_default()).collect();
    let elf = std::fs::read(root.join("boot/SCUS_971.99")).ok()?;
    let ov0 = std::fs::read(root.join("levels/00/overlay.bin")).ok()?;
    let ov1 = std::fs::read(dir.join("overlay.bin")).ok()?;
    let tables = ItemTables::load(&elf, &ov1).unwrap();
    let ring = QuickSelect::load(&Overlay::parse(&ov1).unwrap(), &tables).expect("the ring's tables");
    let sections = rc_formats::font::parse_overlay_sections(&ov1).unwrap();
    let (n, sz) = (rc_formats::save_game::ITEM_COUNT, rc_formats::save_game::ITEM_DEF_SIZE);
    let raw = rc_formats::font::read_overlay(&sections, tables.item_defs_addr, n * sz).unwrap().to_vec();
    let w = |i: usize, o: usize| i32::from_le_bytes(raw[i * sz + o..i * sz + o + 4].try_into().unwrap());
    let defs = (0..n).map(|i| ItemDef { slot: w(i, 8), attach: w(i, 0xc), o_class: w(i, 0x10), b18: raw[i * sz + 0x18] }).collect();
    let weapon_defs = (0..n).map(|i| WeaponDef { w18: w(i, 0x18), anims: [w(i, 0x24), w(i, 0x28), w(i, 0x2c)], w30: w(i, 0x30) }).collect();
    let hero_chains = HERO_LISTS.iter().map(|&l| gadget::joint_list(&ratchet_blob, &class.header, l).map(|(a, _)| a).unwrap_or_default()).collect();
    let item_classes = gadget::parse_gadget_classes(&core, &data)
        .unwrap()
        .iter()
        .map(|g| {
            let c = &g.moby.class;
            let chains = (0..16).map_while(|l| gadget::joint_list(&g.blob, &c.header, l).ok().map(|(a, _)| a)).collect();
            ItemClass { o_class: g.moby.o_class as i16, anim: MobyAnimClass::new(c, parse_sequences(&g.blob, c).unwrap_or_default()), scale: c.header.scale, chains }
        })
        .collect();
    let items = ItemData { defs, hero_chains, classes: item_classes };
    let lump = SaveGameLump::parse(&std::fs::read(root.join("global/save_game.bin")).ok()?).unwrap();
    let ct = ChunkTables::from_boot_elf(&elf).unwrap();
    let mut state = GameState::new_game(ct, &lump.template).unwrap();
    let mut session = SessionState::default();
    state.apply_level_start(0, &ItemTables::load(&elf, &ov0).unwrap(), &mut session);
    state.on_veldin_clank_init(&mut session);
    state.apply_transition(1);
    state.apply_level_start(1, &tables, &mut session);
    Some(Lv { mesh, instances, pvars, tests, splines, gp: gp.to_vec(), classes, spawnable, death_z, coll_blobs, ratchet, seconds, items, weapon_defs, tables, ring, state, session })
}

/// The hero's hit sink over the moby loop's services (the engine's `CellHits`).
struct CellHits<'a, 'b> {
    svc: &'a std::cell::RefCell<&'b mut Services>,
    classes: &'a ClassTable,
    coll: &'a collision::Collision,
}

type V4 = rc_game::hero::physics::V4;
type Tmpl = rc_game::moby_update::services::HitTemplate;

impl rc_game::hero::items::HitSink for CellHits<'_, '_> {
    fn sphere(&mut self, table: &mut MobyTable, r: rc_game::ps2v::Pf, centre: V4, flags: u32, ignore: Option<usize>, tmpl: &Tmpl) -> Option<usize> {
        let mut s = self.svc.borrow_mut();
        ServiceHits { svc: &mut s, classes: self.classes, coll: Some(self.coll) }.sphere(table, r, centre, flags, ignore, tmpl)
    }
    fn line(&mut self, table: &mut MobyTable, a: V4, b: V4, flags: u32, ignore: Option<usize>, tmpl: &Tmpl) -> Option<Option<usize>> {
        let mut s = self.svc.borrow_mut();
        ServiceHits { svc: &mut s, classes: self.classes, coll: Some(self.coll) }.line(table, a, b, flags, ignore, tmpl)
    }
    fn deliver(&mut self, table: &mut MobyTable, target: usize, tmpl: &Tmpl) {
        let mut s = self.svc.borrow_mut();
        rc_game::moby_update::services::deliver_hit_in(table, &mut s.hits, target, tmpl);
    }
}

/// The ring's fields after a frame (what `QuickSelect::update` and the draw read).
type RingRow = (bool, i32, i32, u8, u8, i16, i16, i16);

/// One main-loop frame.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    /// The frame's tick was the held world's ([`Game::hold_tick`]).
    held: bool,
    counter: u64,
    rng: u32,
    state: i32,
    pos: [f32; 3],
    cam: [f32; 3],
    /// The hand item (0x140408) and the request 0x141408 after the frame.
    hand: i32,
    request: i32,
    /// Every moby's (state, position).
    mobys: Vec<(u8, [f32; 3])>,
    parts: u32,
    ring: RingRow,
}

/// The ring script of player_controller.md §12: △ held over frames 60..=100, the stick up over 70..=100 (slot 0).
fn ring_script(t: u32) -> PadInput {
    let p = if (60..=100).contains(&t) { PadInput::neutral().press(button::TRIANGLE) } else { PadInput::neutral() };
    if (70..=100).contains(&t) { p.stick(0.0, -1.0) } else { p }
}

fn run(lv: &Lv, modern: bool, input: &dyn Fn(u32) -> PadInput, frames: u32) -> Vec<Row> {
    let (mut gs, mut session) = (lv.state.clone(), lv.session);
    let classes = Arc::new(ClassTable { classes: lv.classes.classes.clone() });
    let mut ct = ClassTable { classes: lv.classes.classes.clone() };
    let statics = load_level_mobys(&lv.instances, &mut ct, &lv.pvars, &lv.tests);
    let mut table = MobyTable::new(statics.mobys.clone(), lv.spawnable);
    let hero_idx = table.mobys.iter().position(|m| m.o_class == 0).expect("Ratchet");
    table.mobys[hero_idx].mode |= mode::NO_UPDATE;
    let mut game = Game::new(&lv.mesh, table, hero_idx, GameOptions::default(), lv.death_z);
    game.hero.idle.level = 1;
    game.hero.set_joint_targets(&lv.seconds);
    game.item_data = Some(lv.items.clone());
    game.hero.weapons.defs = lv.weapon_defs.clone();
    game.hero.owned.0 = gs.global.owned;
    game.hero.weapons.ammo = gs.global.ammo;
    for (i, r) in lv.tables.records.iter().enumerate() { game.hero.weapons.uses_ammo[i] = r.has_ammo(); }
    let mut svc = Services::new();
    svc.level = 1;
    svc.set_splines(&lv.splines);
    svc.groups = statics.groups(&lv.gp);
    svc.set_moby_collision(lv.coll_blobs.clone());
    svc.build_grid(&mut game.mobys);
    let mut particles = Particles::new(None, Vec::new());
    let mut sched = Scheduler::new();
    {
        let hero: Hero = game.hero.clone();
        let mut w = World::new(&mut game.mobys, &hero, &mut game.rng, &*classes, &mut svc, 0);
        w.camera = game.camera.out.pos;
        w.coll = Some(&lv.mesh);
        w.particles = Some(&mut particles);
        sched.load_pass(&mut w);
    }
    game.finish_load();
    let mut anim = RatchetAnim::new(&lv.ratchet);
    anim.layer_joints = rc_game::hero::anim::LAYER_LISTS.map(|l| lv.seconds[l as usize].clone());
    let mut ring = lv.ring.clone();
    let svc_cell = std::cell::RefCell::new(&mut svc);
    let parts_cell = std::cell::RefCell::new(&mut particles);
    let mut rows = Vec::new();
    for t in 0..frames {
        let bytes = input(t).bytes();
        // crate::gameplay::tick: the held world, or the tick between the engine's syncs of the hand-swap globals.
        let held = quick_select_hold(modern, ring.open, Some(&bytes));
        if held {
            game.hold_tick(Some(&bytes));
        } else {
            let g = &gs.global;
            game.item_globals = ItemGlobals { request: session.temp_hand, saved: g.equipped[0], previous: g.last_hand_item, wrench_flag: g.wrench_held, drone: session.drone };
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
            let mut parts = |hero: &Hero, _: &rc_game::follow_camera::CameraView, rng: &mut Rng, _: u64| {
                let mut p = parts_cell.borrow_mut();
                rc_game::hero::fx::create_particles(hero, &mut p);
                p.update_parts(rng);
            };
            let mut world = SharedServices { svc: &svc_cell, classes: classes.clone() };
            let mut hooks = TickHooks { mobys: &mut mobys, particles: &mut parts, world: Some(&mut world) };
            game.hero.idle.counter = game.counter as i32;
            let mut hits = CellHits { svc: &svc_cell, classes: &classes, coll: &lv.mesh };
            let r = game.tick_with_hits(Some(&bytes), &lv.mesh, &mut anim.ctl(&lv.ratchet), &mut hooks, &mut hits);
            assert_eq!(r.hero, rc_game::hero::HeroTick::Ran, "hero stopped in state {:#x} at frame {t}", game.hero.state);
            let g = game.item_globals;
            (gs.global.equipped[0], gs.global.last_hand_item, gs.global.wrench_held) = (g.saved, g.previous, g.wrench_flag);
            (session.temp_hand, session.drone) = (g.request, g.drone);
        }
        // crate::menu_render's mode-0 frame: the ring's hero part and its HUD update on this frame's pad.
        let inp = MenuInput::from_pad(&game.pad, true);
        let h = &game.hero;
        let gate = HeroGate {
            early_exit: h.health < 1,
            swap_state: h.items.slot.swap,
            f594: 0,
            b13fc: h.items.f13fc,
            b13f7: h.items.f13f7,
            b13f4: h.mode,
            s3f502: h.f502,
            held_item: if gs.global.wrench_held != 0 { 8 } else { gs.global.equipped[0] },
        };
        ring.hero(&inp, &gate, &mut gs.global, &mut session);
        if ring.update(&inp, &gate, &mut gs.global, &mut session, t, 0).lock_pad { game.pad.lock = 2; }
        let h = &game.hero;
        rows.push(Row {
            held,
            counter: game.counter,
            rng: game.rng.state,
            state: h.state,
            pos: h.position(),
            cam: game.camera.out.pos_f32(),
            hand: h.items.slot.id,
            request: session.temp_hand,
            mobys: game.mobys.mobys.iter().map(|m| (m.state, [m.position[0], m.position[1], m.position[2]])).collect(),
            parts: parts_cell.borrow().live_by_type().iter().sum(),
            ring: (ring.open, ring.sel, ring.arm, ring.fade, ring.text_fade, ring.lock_timer, ring.tap_timer, ring.double_timer),
        });
    }
    rows
}

fn dump(name: &str, rows: &[Row]) {
    if std::env::var("RC_QS_PAUSE_DUMP").is_err() { return; }
    for (t, r) in rows.iter().enumerate() {
        eprintln!("{name} {t:4} held {} counter {} rng {:#010x} state {:#x} pos {:.3?} hand {} req {} parts {} ring {:?}", r.held as u8, r.counter, r.rng, r.state, r.pos, r.hand, r.request, r.parts, r.ring);
    }
}

/// The frames the hand item changed after `from` (frame, new id).
fn hand_changes(rows: &[Row], from: usize) -> Vec<(usize, i32)> {
    rows.windows(2).enumerate().skip(from).filter(|(_, w)| w[0].hand != w[1].hand).map(|(i, w)| (i + 1, w[1].hand)).collect()
}

/// Classic: the world ticks under the ring (the counter every frame, Ratchet's walk until the ring's pad lock, a moving
/// moby), and the release swaps to the Bomb Glove as player_controller.md §12 records (8 → none at 103 → 10 at 110).
/// Modern, the same script: the world holds from the frame after the ring opened to the release (61..=100: no counter,
/// no `rand` draw, Ratchet, the camera, every moby and the particles as on frame 60), the ring's fields are frame for
/// frame Classic's, and the swap comes on the same frames after the release.
#[test]
fn novalis_modern_quick_select_holds_the_world_while_triangle_is_held() {
    let Some(lv) = load() else { eprintln!("skipped: no extracted/"); return };
    assert_eq!(lv.state.global.quick_select[0], 10, "the Bomb Glove in slot 0");
    let classic = run(&lv, false, &ring_script, 140);
    let modern = run(&lv, true, &ring_script, 140);
    dump("classic", &classic);
    dump("modern", &modern);
    // Classic as today.
    assert!(classic.iter().all(|r| !r.held));
    assert!(classic.windows(2).all(|w| w[1].counter == w[0].counter + 1), "the counter every frame");
    assert!(classic[60].ring.0 && classic[100].ring.0 && classic[100].ring.1 == 0, "the ring open on slot 0");
    assert_ne!(classic[100].pos, classic[60].pos, "Ratchet walks under the ring (before its lock)");
    let moving = (0..classic[60].mobys.len()).find(|&i| i != 0 && classic[61].mobys[i] != classic[100].mobys[i] && classic[61].mobys[i].0 < 0x80);
    let moving = moving.expect("a moby moving under the ring in Classic");
    assert_eq!(hand_changes(&classic, 60), [(103, 0), (110, 10)], "the swap to the Bomb Glove");
    // Modern: held exactly while △ is held with the ring open.
    let held: Vec<usize> = modern.iter().enumerate().filter(|(_, r)| r.held).map(|(i, _)| i).collect();
    assert_eq!(held, (61..=100).collect::<Vec<_>>());
    for r in &modern[61..=100] {
        let f = &modern[60];
        assert_eq!((r.counter, r.rng, r.state, r.pos, r.cam, r.hand, r.parts), (f.counter, f.rng, f.state, f.pos, f.cam, f.hand, f.parts), "the world held");
        assert!(r.mobys == f.mobys, "every moby held");
    }
    assert_ne!(classic[100].mobys[moving], modern[100].mobys[moving], "moby {moving} moved in Classic, held in Modern");
    assert_eq!(modern[101].counter, modern[60].counter + 1, "the world resumes on the release");
    // The ring and the swap as in Classic.
    let ring = |rows: &[Row]| rows.iter().map(|r| (r.ring, r.request)).collect::<Vec<_>>();
    assert_eq!(ring(&modern), ring(&classic), "the ring frame for frame");
    assert_eq!(hand_changes(&modern, 60), hand_changes(&classic, 60), "the swap on the same frames after the release");
    assert!(modern[100..].windows(2).all(|w| w[1].counter == w[0].counter + 1), "ticking again after the release");
    assert_eq!(modern, run(&lv, true, &ring_script, 140), "deterministic");
}

/// Modern before the ring is open: a one-frame △ tap (the press that opens the ring is a normal tick, and △ is up while
/// the ring fades) holds nothing and runs as Classic; with the ring script, the frames up to the ring's open
/// (0..=60) are Classic's.
#[test]
fn novalis_modern_holds_nothing_before_the_ring_opens() {
    let Some(lv) = load() else { eprintln!("skipped: no extracted/"); return };
    let tap = |t: u32| if t == 30 { PadInput::neutral().press(button::TRIANGLE) } else { PadInput::neutral() };
    let modern = run(&lv, true, &tap, 60);
    assert!(modern.iter().all(|r| !r.held), "a one-frame tap opens the ring on the frame it is pressed: nothing held");
    assert_eq!(modern, run(&lv, false, &tap, 60), "a tap is Classic's");
    let (c, m) = (run(&lv, false, &ring_script, 61), run(&lv, true, &ring_script, 61));
    assert_eq!(c[..=60], m[..=60], "identical up to the ring's open");
}
