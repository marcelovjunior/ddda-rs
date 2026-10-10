//! Installed character-level notice. The host remains the progression authority.
//! Native queue/identity/text/hold sources are marked below. This small presenter
//! handles the enter/exit tracks and settled panel; local flashes, font filters,
//! general GUI sequencing and other key interpolation modes remain unported.
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use ddhost::ddprogress::{Progression, Role};
use std::collections::{HashMap, VecDeque};

const LAYOUT: &str = "id/cockpit/level_info"; // 0x00676E70
const COCKPIT: &str = "id/message/cockpit/cockpit_eng";
const COMMON: &str = "id/DDN/message/common/common_message_eng";
// Translated from 0x00676D20 (DDDA.exe 2017-12-13 Steam).
const HOLD_FRAMES: f32 = 120.0;

pub(crate) struct LevelUpPlugin;
impl Plugin for LevelUpPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, update.after(crate::opening::step));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Notice {
    owner: u8,
    pawn: bool,
    level: u32,
    name: String,
}

#[derive(Default)]
struct Observer {
    seen: HashMap<(u8, bool), (usize, u32)>,
    queue: VecDeque<Notice>,
}

impl Observer {
    /// Co-op presentation policy: each local Arisen has its own queue entries;
    /// the main pawn's entries belong to its progression owner. Remote seats and
    /// support pawns are excluded. Source B5BD20 only notifies the Arisen or a
    /// pawn with +5854 == 0; support pawns do not earn levels (ddprogress).
    fn observe(&mut self, progression: &Progression, locals: &[u8], pawn_name: &str) {
        let mut changes = Vec::new();
        let mut present = Vec::new();
        for member in progression.party.iter().flatten() {
            let (owner, pawn) = match member.role {
                Role::Arisen(owner) => (owner, false),
                Role::MainPawn { owner } => (owner, true),
                Role::Support { .. } => continue,
            };
            if !locals.contains(&owner) {
                continue;
            }
            let Some(character) = progression.characters.get(member.character) else {
                continue;
            };
            let level = character.progress.level;
            let key = (owner, pawn);
            present.push(key);
            if let Some((old_character, old_level)) = self.seen.insert(key, (member.character, level)) {
                // A restored/rebound record establishes a baseline; it is not a
                // gameplay level-up. Several host ticks may fit one render frame.
                if old_character == member.character && old_level < level {
                    for level in old_level + 1..=level.min(200) {
                        changes.push(Notice {
                            owner,
                            pawn,
                            level,
                            name: if pawn { pawn_name.to_owned() } else { String::new() },
                        });
                    }
                }
            }
        }
        self.seen.retain(|key, _| present.contains(key));
        // Source 0x00452CE0 / 0x00454B90: Arisen category 0 precedes pawn
        // category 1, with a 16-record cap. Co-op queue policy preserves the
        // supported kinds in that order; native per-category eviction is unported.
        changes.sort_by_key(|notice| notice.pawn);
        for notice in changes {
            let at = self
                .queue
                .iter()
                .position(|queued| queued.pawn > notice.pawn)
                .unwrap_or(self.queue.len());
            self.queue.insert(at, notice);
            self.queue.truncate(16);
        }
    }
}

struct Texts {
    title: String,
    prefix: String,
}
impl Texts {
    fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let cockpit = vfs.read(COCKPIT, mtarc::type_hash("rGUIMessage"))?;
        let common = vfs.read(COMMON, mtarc::type_hash("rGUIMessage"))?;
        let cockpit = mtgmd::MessageFile::parse(&cockpit).map_err(|e| e.to_string())?;
        let common = mtgmd::MessageFile::parse(&common).map_err(|e| e.to_string())?;
        // 0x006774DE / 0x00677509 bind common 0x58 and cockpit 4.
        Ok(Self {
            title: cockpit.message(4).ok_or("level title absent")?.to_owned(),
            prefix: common.message(0x58).ok_or("level prefix absent")?.to_owned(),
        })
    }
    fn fields(&self, notice: &Notice) -> Result<[String; 4], String> {
        // 0x00677334 uses installed "%3d" at 0x0157D654 below 200.
        // 0x00677319 selects global 0x018D9230 at the cap: its value was
        // read from the original as <ICON INFINITY> (board6344). Runtime icon
        // routing remains unported; don't replace it with a made-up label.
        if notice.level >= 200 {
            return Err("level 200 needs the native INFINITY icon renderer".into());
        }
        Ok([
            self.title.clone(),
            self.prefix.clone(),
            format!("{:3}", notice.level),
            notice.name.clone(),
        ])
    }
}

#[derive(Clone)]
struct Track {
    name: String,
    vector: bool,
    keys: Vec<(f32, [f32; 4])>,
}
impl Track {
    fn sample(&self, frame: f32) -> [f32; 4] {
        let Some(first) = self.keys.first() else {
            return [0.0; 4];
        };
        if frame <= first.0 {
            return first.1;
        }
        for pair in self.keys.windows(2) {
            if frame <= pair[1].0 {
                // Translated from 0x011511C0, float/vector key mode 3
                // (DDDA.exe 2017-12-13 Steam). Scalar uses x87 weighted
                // interpolation; vector stores t to f32 before component math.
                let t =
                    (f64::from(frame) - f64::from(pair[0].0)) / (f64::from(pair[1].0) - f64::from(pair[0].0));
                return if self.vector {
                    let t = t as f32;
                    std::array::from_fn(|i| (1.0 - t) * pair[0].1[i] + pair[1].1[i] * t)
                } else {
                    std::array::from_fn(|i| {
                        ((1.0 - t) * f64::from(pair[0].1[i]) + f64::from(pair[1].1[i]) * t) as f32
                    })
                };
            }
        }
        self.keys.last().unwrap().1
    }
}

struct Sequence {
    tracks: Vec<Track>,
    frames: f32,
}
fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("level track truncated")?
            .try_into()
            .unwrap(),
    ))
}
fn sequence(gui: &mtgui::Gui<'_>, id: u32, node: u32) -> Result<Sequence, String> {
    // uGUI::vf25 0x00E0B840 constructs global tracks from b8 -> bc -> b0.
    // uGUI::vf38 0x00E0A620 selects the descriptor by id (0x00E0A4E0).
    let section = |offset| {
        gui.section(offset)
            .map(|s| s.bytes)
            .ok_or("level GUI section absent")
    };
    let descriptors = section(0xb8)?;
    let entry = descriptors
        .chunks_exact(16)
        .find(|r| word(r, 0).ok() == Some(id))
        .ok_or("level sequence absent")?;
    let index = word(entry, 12)? as usize;
    let rows = section(0xbc)?;
    let row = rows
        .get(index * 36..index * 36 + 36)
        .ok_or("level descriptor range")?;
    let first = word(row, 12)? as usize;
    let count = word(row, 8)? as usize;
    let mut out = Sequence {
        tracks: vec![],
        frames: 0.0,
    };
    let tracks = section(0xb0)?;
    let keys = section(0xf4)?;
    let strings = section(0xf0)?;
    for index in first..first + count {
        let row = tracks
            .get(index * 20..index * 20 + 20)
            .ok_or("level global track range")?;
        if word(row, 4)? != node {
            continue;
        }
        let flags = word(row, 0)?;
        let kind = flags & 255;
        if kind != 2 && kind != 4 {
            return Err("unsupported level global track type".into());
        }
        let start = word(row, 8)? as usize;
        let name = strings.get(start..).ok_or("level track name range")?;
        let length = name
            .iter()
            .position(|&v| v == 0)
            .ok_or("level track name terminator")?;
        let name = std::str::from_utf8(&name[..length])
            .map_err(|e| e.to_string())?
            .to_owned();
        let first_key = word(row, 12)? as usize;
        let first_value = word(row, 16)? as usize;
        let mut track = Track {
            name,
            vector: kind == 4,
            keys: vec![],
        };
        for index in 0..((flags >> 8) & 65535) as usize {
            let packed_key = word(keys, (first_key + index) * 8)?;
            // All the selected installed panel tracks use mode 3. Refuse
            // other modes instead of silently substituting linear motion.
            if packed_key >> 24 != 3 {
                return Err("unsupported level panel interpolation mode".into());
            }
            let frame = (packed_key & 0xffffff) as f32;
            if track.keys.last().is_some_and(|&(last, _)| last >= frame) {
                return Err("unordered level panel keys".into());
            }
            let mut value = [0.0; 4];
            for (component, v) in value.iter_mut().enumerate().take(if kind == 4 { 4 } else { 1 }) {
                *v = f32::from_bits(word(
                    section(if kind == 4 { 0x100 } else { 0xfc })?,
                    first_value + index * (if kind == 4 { 16 } else { 4 }) + component * 4,
                )?);
            }
            if !value.iter().all(|v| v.is_finite()) {
                return Err("non-finite level key".into());
            }
            out.frames = out.frames.max(frame);
            track.keys.push((frame, value));
        }
        out.tracks.push(track);
    }
    if out.tracks.is_empty() {
        return Err("level sequence has no panel tracks".into());
    }
    Ok(out)
}

struct Panel {
    node: usize,
    position: Vec2,
    enter: Sequence,
    exit: Sequence,
}
fn panel(gui: &mtgui::Gui<'_>, pawn: bool) -> Result<Panel, String> {
    // 0x00677342 selects node index 1 for Arisen or 15 for pawn; the
    // original performs pointer-array lookup, not lookup of node.id.
    let node = if pawn { 15 } else { 1 };
    let id = gui.nodes.get(node).ok_or("level panel absent")?.id;
    // 0x006772A0: these are sequence ids, NOT message ids.
    let scalar = |name| {
        gui.node_property(node, name)
            .and_then(|p| p.word)
            .map(f32::from_bits)
            .unwrap_or(0.0)
    };
    Ok(Panel {
        node,
        position: Vec2::new(scalar("PosX"), scalar("PosY")),
        enter: sequence(gui, if pawn { 12 } else { 3 }, id)?,
        exit: sequence(gui, if pawn { 14 } else { 5 }, id)?,
    })
}

#[derive(Clone)]
struct Quad {
    position: Vec2,
    size: Vec2,
    image: Handle<Image>,
    rect: Rect,
    color: Color,
    layer: i32,
}
struct Active {
    notice: Notice,
    clock: Clock,
    quads: Vec<Quad>,
    entities: Vec<Entity>,
}
#[derive(Default)]
struct Consumers(HashMap<u8, Active>);
impl Consumers {
    // Co-op presentation policy: a follow switch suspends that owner's panel
    // and clock, while the new followed owner can consume its own queue.
    // Resuming does not spend hidden time or alter the native clock ordering.
    fn advance(&mut self, owner: u8, panels: &[Panel; 2], delta: f32, visible: bool) -> Option<Active> {
        if !visible {
            return None;
        }
        let active = self.0.get_mut(&owner)?;
        active
            .clock
            .advance(&panels[usize::from(active.notice.pawn)], delta);
        if active.clock.phase == Phase::Done {
            self.0.remove(&owner)
        } else {
            None
        }
    }
    fn next(&self, observer: &mut Observer, owner: u8, visible: bool) -> Option<Notice> {
        if !visible || self.0.contains_key(&owner) {
            return None;
        }
        let index = observer.queue.iter().position(|notice| notice.owner == owner)?;
        observer.queue.remove(index)
    }
}
#[derive(Resource)]
struct View {
    vfs: ddvfs::Vfs,
    gui: Vec<u8>,
    font: Vec<u8>,
    texts: Texts,
    textures: HashMap<String, Handle<Image>>,
    panels: [Panel; 2],
    virtual_size: Vec2,
    observer: Observer,
    consumers: Consumers,
    captured: bool,
}

fn setup(mut commands: Commands, args: Res<crate::Args>, mut images: ResMut<Assets<Image>>) {
    if !args.opening {
        return;
    }
    match View::load(&args.native_pc, &mut images) {
        Ok(view) => commands.insert_resource(view),
        Err(e) => warn!("S43 level notice: {e}"),
    }
}

impl View {
    fn load(native_pc: &std::path::Path, images: &mut Assets<Image>) -> Result<Self, String> {
        let vfs = ddvfs::Vfs::open(native_pc)?;
        let gui = vfs.read(LAYOUT, mtarc::type_hash("rGUI"))?;
        let parsed = mtgui::Gui::parse(&gui)?;
        let panels = [panel(&parsed, false)?, panel(&parsed, true)?];
        let virtual_size = Vec2::new(parsed.width as f32, parsed.height as f32);
        let path = parsed
            .resources
            .iter()
            .find(|r| r.class == "rGUIFont")
            .and_then(|r| r.path)
            .ok_or("level font absent")?;
        let font = vfs.read(path, mtarc::type_hash("rGUIFont"))?;
        let texts = Texts::load(&vfs)?;
        let mut textures = HashMap::new();
        for region in parsed.texture_regions()? {
            if !textures.contains_key(region.path) {
                textures.insert(
                    region.path.to_owned(),
                    images.add(crate::stage::tex_to_image(
                        &vfs.read(region.path, mtarc::type_hash("rTexture"))?,
                    )?),
                );
            }
        }
        Ok(View {
            vfs,
            gui,
            font,
            texts,
            textures,
            panels,
            virtual_size,
            observer: Observer::default(),
            consumers: Consumers::default(),
            captured: false,
        })
    }
    fn texture(&mut self, path: &str, images: &mut Assets<Image>) -> Result<Handle<Image>, String> {
        if let Some(image) = self.textures.get(path) {
            return Ok(image.clone());
        }
        let image = images.add(crate::stage::tex_to_image(
            &self.vfs.read(path, mtarc::type_hash("rTexture"))?,
        )?);
        self.textures.insert(path.to_owned(), image.clone());
        Ok(image)
    }
    fn quads(&mut self, notice: &Notice, images: &mut Assets<Image>) -> Result<Vec<Quad>, String> {
        // Keep resource borrows independent from the mutable atlas cache.
        let bytes = self.gui.clone();
        let font_bytes = self.font.clone();
        let gui = mtgui::Gui::parse(&bytes)?;
        let font = mtgui::font::Font::parse(&font_bytes)?;
        let node = self.panels[usize::from(notice.pawn)].node;
        let animation = gui.local_animation(node).ok_or("level local animation absent")?;
        let objects = gui.animation_objects(animation.name)?;
        let fields = self.texts.fields(notice)?;
        let regions = gui.texture_regions()?;
        let mut out = Vec::new();
        // The settled panel objects are the first ten objects of each native
        // animation. The remaining pawn objects are transient flashes, unported.
        // 0x00677399..00677509 binds object indices 8/9/5/4 in either panel.
        for (index, object) in objects.iter().take(10).enumerate() {
            let vector = |name| object.property(name).and_then(|p| gui.vector(p));
            let Some(size) = vector("Size") else { continue };
            let size = Vec2::new(size[0], size[1]);
            let position = Vec2::new(
                object.scalar("PosX").unwrap_or(0.0),
                object.scalar("PosY").unwrap_or(0.0),
            ) + Vec2::from_array(mtgui::layout::anchor_offset(
                size.to_array(),
                object.integer("ControlPoint").unwrap_or(0),
            ));
            let layer = object.integer("Priority").unwrap_or(0) as i32;
            if let Some(field) = [4, 5, 8, 9].iter().position(|&slot| slot == index) {
                let text = &fields[field];
                let font_size = vector("FontSize").ok_or("level text size absent")?;
                let color = vector("Color").unwrap_or([255.0; 4]);
                let color = Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                );
                // Native proportional metrics from the existing font reader.
                // Object 8 is parented below the level-number transform (15/8).
                // The installed hierarchy's transform must apply to its glyphs.
                let transform = object_transform(&gui, animation, index)?;
                let spacing = object.integer("LetterSpace").unwrap_or(0) as f32;
                for (ordinal, placed) in font
                    .line(text, [font_size[0], font_size[1]])?
                    .into_iter()
                    .enumerate()
                {
                    let glyph = placed.glyph;
                    let image = self.texture(&font.texture_path(glyph.page)?, images)?;
                    out.push(Quad {
                        position: transform.transform_point2(
                            position
                                + Vec2::from_array(placed.position)
                                + Vec2::new(ordinal as f32 * spacing, 0.0),
                        ),
                        size: Vec2::from_array(placed.size),
                        image,
                        rect: Rect::new(
                            glyph.x as f32,
                            glyph.y as f32,
                            (glyph.x + glyph.width) as f32,
                            (glyph.y + glyph.height) as f32,
                        ),
                        color,
                        layer,
                    });
                }
            } else if let Some(texture) = object.integer("Texture").filter(|&t| t != u32::MAX) {
                let region = regions
                    .iter()
                    .find(|r| r.id == texture)
                    .ok_or("level texture region absent")?;
                let color = vector("VertexColor").unwrap_or([255.0; 4]);
                let rect = vector("mTextureRect").unwrap_or(region.rect);
                out.push(Quad {
                    position,
                    size,
                    image: self.texture(region.path, images)?,
                    rect: Rect::from_corners(Vec2::new(rect[0], rect[1]), Vec2::new(rect[2], rect[3])),
                    color: Color::srgba(
                        color[0] / 255.0,
                        color[1] / 255.0,
                        color[2] / 255.0,
                        color[3] / 255.0,
                    ),
                    layer,
                });
            }
        }
        Ok(out)
    }
}

fn object_transform(
    gui: &mtgui::Gui<'_>,
    animation: &mtgui::Animation<'_>,
    wanted: usize,
) -> Result<bevy::math::Affine2, String> {
    // 0x01077EC0 calls 0x01068C40 for r+C (FIRST CHILD), which writes
    // child+74=this. r+8 is NEXT SIBLING via 0x01068C60, which inherits
    // this+74 as sibling+74. Confirmed against both setters' disassembly.
    let records = gui.section(0xa4).ok_or("level object section absent")?.bytes;
    let objects = gui.animation_objects(animation.name)?;
    let first = animation.words[3] as usize;
    let parent = |child: usize| -> Result<Option<usize>, String> {
        for index in 0..objects.len() {
            let mut next = word(records, (first + index) * 32 + 12)?;
            let mut count = 0;
            while next != u32::MAX {
                let absolute = next as usize;
                if absolute < first || absolute >= first + objects.len() {
                    return Err("level child outside animation".into());
                }
                if absolute - first == child {
                    return Ok(Some(index));
                }
                next = word(records, absolute * 32 + 8)?;
                count += 1;
                if count > objects.len() {
                    return Err("cyclic level object siblings".into());
                }
            }
        }
        Ok(None)
    };
    let mut transform = bevy::math::Affine2::IDENTITY;
    let mut index = parent(wanted)?;
    let mut count = 0;
    while let Some(i) = index {
        let object = &objects[i];
        let local = bevy::math::Affine2::from_scale_angle_translation(
            Vec2::new(
                object.scalar("SclX").unwrap_or(1.0),
                object.scalar("SclY").unwrap_or(1.0),
            ),
            object.scalar("Rotation").unwrap_or(0.0).to_radians(),
            Vec2::new(
                object.scalar("PosX").unwrap_or(0.0),
                object.scalar("PosY").unwrap_or(0.0),
            ),
        );
        transform = local * transform;
        index = parent(i)?;
        count += 1;
        if count > objects.len() {
            return Err("cyclic level object parents".into());
        }
    }
    Ok(transform)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Enter,
    Hold,
    Exit,
    Done,
}
struct Clock {
    phase: Phase,
    frame: f32,
    hold: f32,
}
impl Default for Clock {
    fn default() -> Self {
        Self {
            phase: Phase::Enter,
            frame: 0.0,
            hold: HOLD_FRAMES,
        }
    }
}
impl Clock {
    /// Translated from 0x00676F20 / 0x00677030 (DDDA.exe 2017-12-13
    /// Steam): intro completion starts the hold; test the OLD counter before
    /// subtracting delta; exit completion ends the notice. The adapter supplies
    /// completion from supported authored track durations, not wall-clock age.
    fn transition(&mut self, delta: f32, sequence_done: bool) {
        match self.phase {
            Phase::Enter if sequence_done => self.phase = Phase::Hold,
            Phase::Hold if self.hold <= 0.0 => {
                self.hold = 0.0;
                self.phase = Phase::Exit;
                self.frame = 0.0;
            }
            Phase::Hold => self.hold -= delta,
            Phase::Exit if sequence_done => self.phase = Phase::Done,
            _ => {}
        }
    }
    fn advance(&mut self, panel: &Panel, delta: f32) {
        let done = match self.phase {
            Phase::Enter => self.frame >= panel.enter.frames,
            Phase::Exit => self.frame >= panel.exit.frames,
            _ => false,
        };
        self.transition(delta, done);
        // uGUI::vf09 runs AFTER the subclass state machine (0x00676F20).
        match self.phase {
            Phase::Enter => self.frame = (self.frame + delta).min(panel.enter.frames),
            Phase::Exit => self.frame = (self.frame + delta).min(panel.exit.frames),
            _ => {}
        }
    }
    fn sample<'a>(&self, panel: &'a Panel) -> Option<(&'a Sequence, f32)> {
        match self.phase {
            Phase::Enter => Some((&panel.enter, self.frame)),
            Phase::Hold => Some((&panel.enter, panel.enter.frames)),
            Phase::Exit => Some((&panel.exit, self.frame)),
            Phase::Done => None,
        }
    }
}

fn update(
    mut commands: Commands,
    opening: Option<Res<crate::opening::OpeningRes>>,
    state: Option<Res<crate::play::PlayState>>,
    view: Option<ResMut<View>>,
    time: Res<Time>,
    args: Res<crate::Args>,
    pause: Res<crate::pause::PauseState>,
    windows: Query<&Window>,
    mut images: ResMut<Assets<Image>>,
    mut quads: Query<(&mut Node, &mut ImageNode)>,
) {
    let (Some(opening), Some(state), Some(mut view)) = (opening, state, view) else {
        return;
    };
    let Ok(window) = windows.single() else { return };
    let session = opening.0.lock().unwrap();
    let pawn_name = session
        .host
        .rift
        .party
        .main_pawn
        .as_ref()
        .map(|pawn| pawn.pawn.edit.name_str())
        .unwrap_or_default();
    view.observer
        .observe(&session.host.progression, &state.locals, &pawn_name);
    let event_active = session.event_active();
    drop(session);
    let visible = !pause.paused && !event_active;
    let View {
        consumers,
        panels,
        observer,
        ..
    } = &mut *view;
    if let Some(finished) = consumers.advance(
        state.follow,
        panels,
        time.delta_secs() * ddhost::clock::BASE_FPS,
        visible,
    ) {
        for entity in finished.entities {
            commands.entity(entity).despawn();
        }
    }
    if let Some(notice) = consumers.next(observer, state.follow, visible) {
        match view.quads(&notice, &mut images) {
            Ok(quads) => {
                info!(
                    "S43_LEVEL_UP owner={} pawn={} level={} name={:?}",
                    notice.owner, notice.pawn, notice.level, notice.name
                );
                let entities = quads
                    .iter()
                    .map(|q| {
                        commands
                            .spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    ..default()
                                },
                                ImageNode {
                                    image: q.image.clone(),
                                    rect: Some(q.rect),
                                    color: q.color,
                                    ..default()
                                },
                                GlobalZIndex(50 + q.layer),
                            ))
                            .id()
                    })
                    .collect();
                view.consumers.0.insert(
                    notice.owner,
                    Active {
                        notice,
                        clock: Clock::default(),
                        quads,
                        entities,
                    },
                );
            }
            Err(e) => warn!("S43 level notice: {e}"),
        }
    }
    for active in view.consumers.0.values() {
        for entity in &active.entities {
            if let Ok((mut node, _)) = quads.get_mut(*entity) {
                node.display = if visible && active.notice.owner == state.follow {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
    }
    let Some(active) = view.consumers.0.get(&state.follow) else {
        return;
    };
    let panel = &view.panels[usize::from(active.notice.pawn)];
    let Some((sequence, frame)) = active.clock.sample(panel) else {
        return;
    };
    let mut position = panel.position;
    let mut alpha = 1.0;
    for track in &sequence.tracks {
        let value = track.sample(frame);
        match track.name.as_str() {
            "PosX" => position.x = value[0],
            "PosY" => position.y = value[0],
            "ColorScale" => alpha = value[3],
            _ => {}
        }
    }
    let scale = Vec2::new(
        window.width() / view.virtual_size.x,
        window.height() / view.virtual_size.y,
    );
    for (entity, q) in active.entities.iter().zip(&active.quads) {
        let Ok((mut node, mut image)) = quads.get_mut(*entity) else {
            continue;
        };
        node.display = if event_active || pause.paused || active.notice.owner != state.follow {
            Display::None
        } else {
            Display::Flex
        };
        node.left = Val::Px((position.x + q.position.x) * scale.x);
        node.top = Val::Px((position.y + q.position.y) * scale.y);
        node.width = Val::Px(q.size.x * scale.x);
        node.height = Val::Px(q.size.y * scale.y);
        image.color = q.color.with_alpha(q.color.alpha() * alpha);
    }
    // A bounded route capture can observe the first real level-up at its
    // settled panel, instead of guessing a wall-clock screenshot delay.
    // This captures presentation only and never generates a level-up.
    if !view.captured
        && !pause.paused
        && !event_active
        && active.notice.owner == state.follow
        && active.clock.phase == Phase::Hold
    {
        if let Some(dir) = &args.shots {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(dir.join("S43-level-up.png")));
            view.captured = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn followed_owner_switch_suspends_clock_without_blocking_other_owner() {
        // Synthetic panels isolate co-op consumption from the installed GUI.
        let panels = [false, true].map(|_| Panel {
            node: 0,
            position: Vec2::ZERO,
            enter: Sequence {
                tracks: vec![],
                frames: 8.0,
            },
            exit: Sequence {
                tracks: vec![],
                frames: 6.0,
            },
        });
        let notice = |owner, level| Notice {
            owner,
            pawn: false,
            level,
            name: String::new(),
        };
        let activate = |notice| Active {
            notice,
            clock: Clock::default(),
            quads: vec![],
            entities: vec![],
        };
        let mut observer = Observer::default();
        observer.queue = VecDeque::from([notice(0, 2), notice(0, 3), notice(1, 2)]);
        let mut consumers = Consumers::default();
        let first = consumers.next(&mut observer, 0, true).unwrap();
        consumers.0.insert(0, activate(first));
        consumers.advance(0, &panels, 3.0, true);
        assert_eq!(consumers.0[&0].clock.frame, 3.0);

        // Owner 0's in-progress notice and earlier pending record cannot block
        // owner 1 after following that seat. Hidden owner 0 never spends time.
        let second = consumers.next(&mut observer, 1, true).unwrap();
        assert_eq!(second.owner, 1);
        consumers.0.insert(1, activate(second));
        let mut finished = None;
        for _ in 0..150 {
            if let Some(active) = consumers.advance(1, &panels, 1.0, true) {
                finished = Some(active);
                break;
            }
        }
        assert_eq!(finished.unwrap().notice.owner, 1);
        assert!(!consumers.0.contains_key(&1));
        assert_eq!(consumers.0[&0].clock.phase, Phase::Enter);
        assert_eq!(consumers.0[&0].clock.frame, 3.0);
        assert_eq!(consumers.0[&0].clock.hold, HOLD_FRAMES);

        // Follow back resumes precisely; it does not consume the next owner-0
        // notice early or catch up any of the time spent following owner 1.
        assert!(consumers.next(&mut observer, 0, true).is_none());
        consumers.advance(0, &panels, 5.0, true);
        assert_eq!(consumers.0[&0].clock.phase, Phase::Enter);
        consumers.advance(0, &panels, 1.0, true);
        assert_eq!(consumers.0[&0].clock.phase, Phase::Hold);
        assert_eq!(consumers.0[&0].clock.hold, HOLD_FRAMES);
        consumers.advance(0, &panels, 500.0, false);
        assert_eq!(consumers.0[&0].clock.hold, HOLD_FRAMES);
        assert_eq!(observer.queue, VecDeque::from([notice(0, 3)]));
    }
    #[test]
    fn native_hold_waits_for_intro_and_tests_counter_before_decrement() {
        let mut clock = Clock::default();
        clock.transition(2.0, false);
        assert_eq!(clock.phase, Phase::Enter);
        assert_eq!(clock.hold, 120.0);
        clock.transition(2.0, true);
        assert_eq!(clock.phase, Phase::Hold);
        assert_eq!(clock.hold, 120.0);
        for _ in 0..119 {
            clock.transition(1.0, false);
        }
        clock.transition(0.5, false);
        assert_eq!(clock.phase, Phase::Hold);
        assert_eq!(clock.hold, 0.5);
        clock.transition(1.0, false);
        assert_eq!(clock.phase, Phase::Hold);
        assert_eq!(clock.hold, -0.5);
        clock.transition(1.0, false);
        assert_eq!(clock.phase, Phase::Exit);
        assert_eq!(clock.hold, 0.0);
        clock.transition(1.0, false);
        assert_eq!(clock.phase, Phase::Exit);
        clock.transition(1.0, true);
        assert_eq!(clock.phase, Phase::Done);
    }
    #[test]
    fn native_linear_scalar_and_vector_tracks_use_weighted_values() {
        let scalar = Track {
            name: "synthetic".into(),
            vector: false,
            keys: vec![(0.0, [10.0, 0.0, 0.0, 0.0]), (8.0, [26.0, 0.0, 0.0, 0.0])],
        };
        assert_eq!(scalar.sample(4.0)[0], 18.0);
        assert_eq!(scalar.sample(8.0)[0], 26.0);
        assert_eq!(scalar.sample(99.0)[0], 26.0);
        let vector = Track {
            name: "synthetic".into(),
            vector: true,
            keys: vec![(0.0, [1.0, 1.0, 1.0, 0.0]), (8.0, [1.0, 1.0, 1.0, 1.0])],
        };
        assert_eq!(vector.sample(4.0), [1.0, 1.0, 1.0, 0.5]);
    }
    #[test]
    fn native_identity_and_queue_preserve_levels_per_local_owner() {
        use ddhost::ddprogress::{Character, Member};
        // Progression records are synthetic; no experience or host state is
        // changed by the presenter. Installed Tables needed only for constructor.
        let root = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !root.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(root).unwrap();
        let tables = ddhost::install::tables(&vfs, &ddexe::locate(root).unwrap()).unwrap();
        let mut progression = Progression::new();
        for (slot, role) in [
            Role::MainPawn { owner: 0 },
            Role::Arisen(1),
            Role::Arisen(0),
            Role::Support { owner: 0 },
        ]
        .into_iter()
        .enumerate()
        {
            let character = progression.add(Character::new(
                &tables,
                ddhost::ddskill::vocation::Vocation::Fighter,
                slot != 1 && slot != 2,
            ));
            progression.party[slot] = Some(Member { character, role });
        }
        let mut observer = Observer::default();
        observer.observe(&progression, &[0], "Synthetic Pawn");
        assert!(observer.queue.is_empty());
        for c in &mut progression.characters {
            c.progress.level += 2
        }
        observer.observe(&progression, &[0], "Synthetic Pawn");
        assert_eq!(
            observer
                .queue
                .iter()
                .map(|n| (n.owner, n.pawn, n.level))
                .collect::<Vec<_>>(),
            vec![(0, false, 2), (0, false, 3), (0, true, 2), (0, true, 3)]
        );
        observer.observe(&progression, &[0], "Synthetic Pawn");
        assert_eq!(observer.queue.len(), 4);
        progression.characters[2].progress.level = 1;
        observer.observe(&progression, &[0], "");
        assert_eq!(observer.queue.len(), 4);
        observer.observe(&progression, &[0, 1], "");
        assert_eq!(observer.queue.len(), 4, "new local seat establishes baseline");
        progression.characters[1].progress.level += 1;
        observer.observe(&progression, &[0, 1], "");
        assert!(observer
            .queue
            .iter()
            .any(|n| n.owner == 1 && !n.pawn && n.level == 4));
    }
    #[test]
    fn installed_text_panel_tracks_and_font_match_native_bindings() {
        let root = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !root.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(root).unwrap();
        let texts = Texts::load(&vfs).unwrap();
        let notice = Notice {
            owner: 1,
            pawn: false,
            level: 2,
            name: "Synthetic".into(),
        };
        assert_eq!(
            texts.fields(&notice).unwrap(),
            ["Level up!", "Lvl.", "  2", "Synthetic"]
        );
        let bytes = vfs.read(LAYOUT, mtarc::type_hash("rGUI")).unwrap();
        let gui = mtgui::Gui::parse(&bytes).unwrap();
        let arisen = panel(&gui, false).unwrap();
        let pawn = panel(&gui, true).unwrap();
        assert_eq!(
            (
                arisen.enter.frames,
                arisen.exit.frames,
                pawn.enter.frames,
                pawn.exit.frames
            ),
            (8.0, 6.0, 8.0, 6.0)
        );
        let mut clock = Clock::default();
        for _ in 0..8 {
            clock.advance(&arisen, 1.0)
        }
        assert_eq!(clock.phase, Phase::Enter);
        clock.advance(&arisen, 1.0);
        assert_eq!(clock.phase, Phase::Hold);
        for _ in 0..120 {
            clock.advance(&arisen, 1.0)
        }
        assert_eq!(clock.phase, Phase::Hold);
        clock.advance(&arisen, 1.0);
        assert_eq!(clock.phase, Phase::Exit);
        for _ in 0..6 {
            clock.advance(&arisen, 1.0)
        }
        assert_eq!(clock.phase, Phase::Done);
        let x = arisen.enter.tracks.iter().find(|t| t.name == "PosX").unwrap();
        assert_eq!(x.sample(0.0)[0], 1430.0);
        assert_eq!(x.sample(8.0)[0], 1029.0);
        let path = gui
            .resources
            .iter()
            .find(|r| r.class == "rGUIFont")
            .unwrap()
            .path
            .unwrap();
        let bytes = vfs.read(path, mtarc::type_hash("rGUIFont")).unwrap();
        let font = mtgui::font::Font::parse(&bytes).unwrap();
        for field in texts.fields(&notice).unwrap() {
            assert!(font.line(&field, [22.0, 22.0]).is_ok())
        }
        let animation = gui.local_animation(arisen.node).unwrap();
        assert_eq!(
            object_transform(&gui, animation, 8)
                .unwrap()
                .transform_point2(Vec2::ZERO),
            Vec2::new(-80.0, 19.0)
        );
        let animation = gui.local_animation(pawn.node).unwrap();
        assert_eq!(
            object_transform(&gui, animation, 8)
                .unwrap()
                .transform_point2(Vec2::ZERO),
            Vec2::new(-90.0, 19.0)
        );
        // Title/prefix/name are siblings of the number transform, not
        // descendants: they must not inherit its position.
        for panel in [&arisen, &pawn] {
            let animation = gui.local_animation(panel.node).unwrap();
            for slot in [4, 5, 9] {
                assert_eq!(
                    object_transform(&gui, animation, slot)
                        .unwrap()
                        .transform_point2(Vec2::ZERO),
                    Vec2::ZERO
                );
            }
        }
        let cap = Notice { level: 200, ..notice };
        assert!(texts.fields(&cap).unwrap_err().contains("INFINITY"));
    }
    #[test]
    fn installed_panels_prepare_cropped_atlases_and_number_hierarchy_without_gpu() {
        let root = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !root.exists() {
            return;
        }
        let mut images = Assets::<Image>::default();
        let mut view = View::load(root, &mut images).unwrap();
        for pawn in [false, true] {
            let notice = Notice {
                owner: 1,
                pawn,
                level: 2,
                name: "Synthetic Pawn".into(),
            };
            let quads = view.quads(&notice, &mut images).unwrap();
            assert!(quads.len() > 20);
            for quad in &quads {
                let image = images.get(&quad.image).unwrap();
                assert!(quad.rect.min.x >= 0.0 && quad.rect.min.y >= 0.0);
                assert!(quad.rect.max.x <= image.width() as f32 && quad.rect.max.y <= image.height() as f32);
                assert!(quad.size.x > 0.0 && quad.size.y > 0.0);
            }
            let background = &quads[0];
            assert_eq!(background.size, Vec2::new(if pawn { 380.0 } else { 450.0 }, 88.0));
            assert_eq!(
                background.rect,
                if pawn {
                    Rect::new(0.0, 0.0, 384.0, 88.0)
                } else {
                    Rect::new(0.0, 0.0, 320.0, 75.0)
                }
            );
            assert_eq!(background.position.y, -44.0);
        }
    }
}
