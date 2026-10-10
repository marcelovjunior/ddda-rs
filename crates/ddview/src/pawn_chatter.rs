//! Installed CMC text/voice consumer. Simulation chooses messages once.
//! This presenter supports plain single-line ordinary HUD speech. Native
//! multiline measurement, markup, filters, portraits and GUI sequence VM
//! remain explicit unsupported paths; no replacement wording is generated.

use bevy::prelude::*;
use ddhost::pawn_chatter::{Bank, Message};
use std::collections::HashMap;

const LAYOUT: &str = "id/cmc_mes/cmc_mes0"; // 0x00633020 resource string.
// Translated from 0x00632FF0 -> 0x014F7270 (DDDA.exe 2017-12-13 Steam).
const HOLD_SECONDS: f32 = 5.0;

pub(crate) struct PawnChatterPlugin;
impl Plugin for PawnChatterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inbox>()
            .add_systems(Startup, setup.after(crate::setup))
            .add_systems(Update, update.after(crate::opening::step).after(crate::net::step));
    }
}

#[derive(Resource, Default)]
struct Inbox(Vec<Message>);
pub(crate) fn queue(commands: &mut Commands, messages: &[Message]) {
    if messages.is_empty() { return; }
    let messages = messages.to_vec();
    commands.queue(move |world: &mut World| {
        if let Some(mut inbox) = world.get_resource_mut::<Inbox>() { inbox.0.extend(messages); }
    });
}

#[derive(Clone)]
struct Quad { position: Vec2, size: Vec2, image: Handle<Image>, rect: Rect }
struct Active { hold: f32, quads: Vec<Quad>, entities: Vec<Entity> }

#[derive(Resource)]
struct View {
    vfs: ddvfs::Vfs,
    exe: ddexe::ExeFile,
    gui: Vec<u8>,
    font: Vec<u8>,
    textures: HashMap<String, Handle<Image>>,
    texts: HashMap<String, Vec<u8>>,
    active: Vec<Active>,
    last_id: u64,
    base: Vec2,
    virtual_size: Vec2,
}

fn setup(mut commands: Commands, args: Res<crate::Args>) {
    if !args.opening && args.host.is_none() && args.join.is_none() { return; }
    match View::load(&args.native_pc) {
        Ok(view) => commands.insert_resource(view),
        Err(error) => warn!("S52 chatter presentation unavailable: {error}"),
    }
}

impl View {
    fn load(native_pc: &std::path::Path) -> Result<Self, String> {
        let vfs = ddvfs::Vfs::open(native_pc)?;
        let exe = ddexe::ExeFile::next_to(native_pc)?;
        let gui = vfs.read(LAYOUT, mtarc::type_hash("rGUI"))?;
        let parsed = mtgui::Gui::parse(&gui)?;
        // 0x00632F5E selects scene node index7's position for every clone.
        let scalar = |name| parsed.node_property(7, name).and_then(|property|
            property.word).map(f32::from_bits).ok_or("CMC base position missing");
        let base = Vec2::new(scalar("PosX")?, scalar("PosY")?);
        let virtual_size = Vec2::new(parsed.width as f32, parsed.height as f32);
        let objects = parsed.animation_objects("cmc_mes")?;
        let font_id = objects.get(5).and_then(|object| object.integer("Font"))
            .ok_or("CMC ordinary text font missing")?;
        let font_path = parsed.resources.iter().find(|resource|
            resource.class == "rGUIFont" && resource.id == font_id).and_then(|resource|
                resource.path).ok_or("CMC installed font resource missing")?;
        let font = vfs.read(font_path, mtarc::type_hash("rGUIFont"))?;
        Ok(Self { vfs, exe, gui, font, base, virtual_size, textures: HashMap::new(),
            texts: HashMap::new(), active: Vec::new(), last_id: 0 })
    }

    fn texture(&mut self, path: &str, images: &mut Assets<Image>) -> Result<Handle<Image>, String> {
        if let Some(image) = self.textures.get(path) { return Ok(image.clone()); }
        let image = images.add(crate::stage::tex_to_image(&self.vfs.read(path, mtarc::type_hash("rTexture"))?)?);
        self.textures.insert(path.into(), image.clone());
        Ok(image)
    }

    fn resolve(&mut self, message: &Message) -> Result<(String, ddstory::event::Cue), String> {
        let bank = Bank::resolve(&self.exe.image()?, message.message,
            u32::from(message.gender), u32::from(message.voice), false)?
            .ok_or("unsupported CMC text/voice bank")?;
        // Viewer language policy matches the opening's current English import.
        // Language options and pitch variants are not inferred from gender.
        let path = format!("{}_eng", bank.text);
        if !self.texts.contains_key(&path) {
            self.texts.insert(path.clone(), self.vfs.read(&path, mtarc::type_hash("rGUIMessage"))?);
        }
        let messages = mtgmd::MessageFile::parse(&self.texts[&path]).map_err(|error| error.to_string())?;
        // 0x00633260 formats the full message ID as a decimal GMD label.
        let text = messages.by_label(&message.message.to_string()).ok_or("CMC decimal label missing")?.to_owned();
        let cue = ddstory::event::Cue::Stream { unit: format!("cmc:{}", message.pawn_slot),
            path: bank.stream, stream_id: i32::from(bank.cue), mode: 0, prepare_id: 0 };
        Ok((text, cue))
    }

    fn quads(&mut self, message: &Message, text: &str, images: &mut Assets<Image>) -> Result<Vec<Quad>, String> {
        if text.chars().any(|character| matches!(character, '\r' | '\n' | '<' | '>')) {
            return Err("CMC markup/multiline layout unported".into());
        }
        let gui_bytes = self.gui.clone();
        let font_bytes = self.font.clone();
        let gui = mtgui::Gui::parse(&gui_bytes)?;
        let font = mtgui::font::Font::parse(&font_bytes)?;
        let animation = gui.animations.iter().find(|animation| animation.name == "cmc_mes")
            .ok_or("CMC ordinary animation missing")?;
        let objects = gui.animation_objects(animation.name)?;
        let mut quads = Vec::new();
        // Native 0x00633A70 / 0x00633B70 bind local objects5 and9.
        for (index, text) in [(5, text), (9, message.name.as_str())] {
            let object = objects.get(index).ok_or("CMC text object missing")?;
            let vector = |name| object.property(name).and_then(|property| gui.vector(property));
            let size = vector("FontSize").ok_or("CMC font size missing")?;
            let bound = vector("Size").ok_or("CMC text bounds missing")?;
            let width = text.chars().map(|character| font.glyph(character)
                .map(|glyph| glyph.advance as f32 * size[0] / font.nominal_size as f32)
                .ok_or_else(|| format!("CMC glyph {character:?} missing"))).collect::<Result<Vec<_>, _>>()?
                .into_iter().sum::<f32>();
            if width > bound[0] { return Err("CMC wrapping/layout measurement unported".into()); }
            let position = object_position(&gui, animation.words[3] as usize, index, objects.len())?;
            let anchor = Vec2::from_array(mtgui::layout::anchor_offset([bound[0], bound[1]],
                object.integer("ControlPoint").unwrap_or(0)));
            for placed in font.line(text, [size[0], size[1]])? {
                let glyph = placed.glyph;
                quads.push(Quad { position: position + anchor + Vec2::from_array(placed.position),
                    size: Vec2::from_array(placed.size), image: self.texture(&font.texture_path(glyph.page)?, images)?,
                    rect: Rect::new(glyph.x as f32, glyph.y as f32,
                        (glyph.x + glyph.width) as f32, (glyph.y + glyph.height) as f32) });
            }
        }
        Ok(quads)
    }
}

// Parent relation follows FIRST CHILD / NEXT SIBLING, as installed GUI
// setters 0x01068C40 / 0x01068C60 do. This subset refuses scale/rotation.
fn object_position(gui: &mtgui::Gui<'_>, first: usize, wanted: usize, count: usize) -> Result<Vec2, String> {
    let objects = gui.animation_objects("cmc_mes")?;
    let bytes = gui.section(0xa4).ok_or("CMC object table missing")?.bytes;
    let word = |index: usize, offset: usize| -> Result<u32, String> {
        Ok(u32::from_le_bytes(bytes.get((first + index) * 32 + offset..(first + index) * 32 + offset + 4)
            .ok_or("CMC object link outside table")?.try_into().unwrap()))
    };
    let mut parents = vec![None; count];
    for parent in 0..count {
        let mut child = word(parent, 12)?;
        let mut visited = 0;
        while child != u32::MAX {
            if child as usize >= count || visited >= count { return Err("invalid CMC object hierarchy".into()); }
            parents[child as usize] = Some(parent);
            child = word(child as usize, 8)?;
            visited += 1;
        }
    }
    let mut at = Some(wanted);
    let mut position = Vec2::ZERO;
    let mut visited = 0;
    while let Some(index) = at {
        let object = &objects[index];
        if object.scalar("SclX").unwrap_or(1.0) != 1.0 || object.scalar("SclY").unwrap_or(1.0) != 1.0
            || object.scalar("Rotation").unwrap_or(0.0) != 0.0 { return Err("CMC object transform unported".into()); }
        position += Vec2::new(object.scalar("PosX").unwrap_or(0.0), object.scalar("PosY").unwrap_or(0.0));
        at = parents[index];
        visited += 1;
        if visited > count { return Err("cyclic CMC parents".into()); }
    }
    Ok(position)
}

fn update(mut commands: Commands, mut inbox: ResMut<Inbox>, view: Option<ResMut<View>>,
    opening: Option<Res<crate::opening::OpeningRes>>, state: Option<Res<crate::play::PlayState>>,
    pause: Res<crate::pause::PauseState>, time: Res<Time>, windows: Query<&Window>,
    mut images: ResMut<Assets<Image>>, mut nodes: Query<&mut Node>) {
    let Some(mut view) = view else { inbox.0.clear(); return; };
    let Ok(window) = windows.single() else { return; };
    let Some(state) = state else { return; };
    let event_active = opening.is_some_and(|opening| opening.0.lock().unwrap().event_active());
    for message in std::mem::take(&mut inbox.0) {
        if message.id <= view.last_id { continue; }
        view.last_id = message.id;
        match view.resolve(&message) {
            Ok((text, cue)) => {
                // Decoding and playback use the ordinary installed STRQ path.
                crate::audio::cues_with_voices(&mut commands,
                    &ddstory::event::EventFrame { cues: vec![cue], ..default() }, Default::default());
                match view.quads(&message, &text, &mut images) {
                    Ok(quads) => {
                        let entities = quads.iter().map(|quad| commands.spawn((Node {
                            position_type: PositionType::Absolute, ..default() }, ImageNode {
                            image: quad.image.clone(), rect: Some(quad.rect), ..default() }, GlobalZIndex(54))).id()).collect();
                        info!("S52_PAWN_MESSAGE id={} frame={} slot={} message={} name={:?}",
                            message.id, message.frame, message.pawn_slot, message.message, message.name);
                        view.active.push(Active { hold: HOLD_SECONDS, quads, entities });
                        if view.active.len() > 3 {
                            for entity in view.active.remove(0).entities { commands.entity(entity).despawn(); }
                        }
                    }
                    Err(error) => warn!("S52 CMC text {}: {error}", message.message),
                }
            }
            Err(error) => warn!("S52 CMC resource {}: {error}", message.message),
        }
    }
    if !pause.paused && !event_active { for active in &mut view.active { active.hold -= time.delta_secs(); } }
    let mut index = 0;
    while index < view.active.len() {
        if view.active[index].hold <= 0.0 {
            for entity in view.active.remove(index).entities { commands.entity(entity).despawn(); }
        } else { index += 1; }
    }
    let scale = Vec2::new(window.width() / view.virtual_size.x, window.height() / view.virtual_size.y);
    for (index, active) in view.active.iter().enumerate() {
        // Native ordinary row shift66, 0x00633E88 -> 0x0151866C.
        // Settled layout only; animated sliding/replacement still unported.
        let row = (view.active.len() - 1 - index) as f32;
        let base = view.base - Vec2::Y * (66.0 * row);
        for (entity, quad) in active.entities.iter().zip(&active.quads) {
            if let Ok(mut node) = nodes.get_mut(*entity) {
                node.display = if !pause.paused && !event_active { Display::Flex } else { Display::None };
                node.left = Val::Px((base.x + quad.position.x) * scale.x);
                node.top = Val::Px((base.y + quad.position.y) * scale.y);
                node.width = Val::Px(quad.size.x * scale.x);
                node.height = Val::Px(quad.size.y * scale.y);
            }
        }
    }
    let _ = state.follow; // Shared party speech is independent of camera-follow seat.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_cmc_gui_binds_original_font_hierarchy_decimal_text_and_voice() {
        let root = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !root.is_dir() { return; }
        let mut view = View::load(&root).unwrap();
        let gui = mtgui::Gui::parse(&view.gui).unwrap();
        let animation = gui.animations.iter().find(|animation| animation.name == "cmc_mes").unwrap();
        let count = gui.animation_objects("cmc_mes").unwrap().len();
        assert_eq!(object_position(&gui, animation.words[3] as usize, 5, count).unwrap(), Vec2::ZERO);
        assert_eq!(object_position(&gui, animation.words[3] as usize, 9, count).unwrap(), Vec2::new(41.0, -32.0));
        let data = ddhost::pawn_chatter::Data::load(&view.vfs).unwrap();
        let message = data.orders.select(ddenemyrt::pawn::Command::Go, None, 0, || 0).unwrap();
        let speech = Message { id: 1, frame: 7, pawn_slot: 1, pawn_seat: 6,
            pawn_identity: ddhost::pawn_chatter::Identity::Hired { hirer: ddhost::ProfileId(3),
                rom_id: 1, network_id: vec![0;64], hired_at: 0 }, message: message as u32,
            name: String::new(), gender: 0, voice: 0, audience: vec![ddhost::ProfileId(3)] };
        let (text, cue) = view.resolve(&speech).unwrap();
        assert!(!text.is_empty());
        let ddstory::event::Cue::Stream { path, stream_id, .. } = cue else { panic!("ordinary STRQ speech"); };
        let bytes = view.vfs.read(&path, mtarc::type_hash("rSoundStreamRequest")).unwrap();
        let stream = mtsound::StreamRequests::parse(&bytes).unwrap();
        assert!(stream.request(stream_id as u16).is_some());
        // The installed proportional font can build the selected line. No
        // renderer/GPU or invented fallback glyph is needed for this check.
        let mut images = Assets::<Image>::default();
        assert!(!view.quads(&speech, &text, &mut images).unwrap().is_empty());
    }
}
