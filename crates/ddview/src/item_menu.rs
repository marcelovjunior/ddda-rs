//! Installed uGUIItemLE Items projection. Its GUI VM, equipment/combining,
//! giving and guest inventory commands remain outside this opening step.
//! The host continues; device input is blocked only for the menu owner.
use bevy::{math::Affine2, prelude::*};
use ddeconomy::inventory::{Bag, Owner, Slot};
use ddsim::PlayerId;
use std::collections::{BTreeMap, HashMap};

const LAYOUT: &str = "id/DDN/co_item/co_item"; // vf58 0x00668270
const NAMES: &str = "id/message/item/itemName_eng"; // 0x006682B0 +9AC
const INFO: &str = "id/message/item/itemInfo_eng"; // 0x006682B0 +9B0
const MESSAGES: &str = "id/message/pause/pause_item/pause_item_eng";

#[derive(Clone)]
struct Item {
    name: String,
    description: String,
    icon: Rect,
    category: u32,
}

pub(crate) struct Catalog {
    items: Vec<Item>,
    use_label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub index: usize,
    pub slot: Slot,
}

impl Catalog {
    fn load(vfs: &ddvfs::Vfs, native: &std::path::Path) -> Result<Self, String> {
        let exe = ddexe::ExeFile::next_to(native).map_err(String::from)?;
        let exe = exe.image().map_err(String::from)?;
        let bytes = vfs.read("etc/item/itemList", mtarc::type_hash("rItemList"))?;
        let table = dditem::itl::ItemList::parse(&bytes).map_err(|e| e.to_string())?;
        let names = vfs.read(NAMES, mtarc::type_hash("rGUIMessage"))?;
        let names = mtgmd::MessageFile::parse(&names).map_err(|e| e.to_string())?;
        let info = vfs.read(INFO, mtarc::type_hash("rGUIMessage"))?;
        let info = mtgmd::MessageFile::parse(&info).map_err(|e| e.to_string())?;
        let messages = vfs.read(MESSAGES, mtarc::type_hash("rGUIMessage"))?;
        let messages = mtgmd::MessageFile::parse(&messages).map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for (id, record) in table.records.iter().enumerate() {
            // Translated from 0x00605DA0 (DDDA.exe 2017-12-13 Steam).
            // Four u16 atlas corners per item. The bulk table is installed.
            let corners = exe.u16s(0x0151AF48 + id as u32 * 8, 4).map_err(String::from)?;
            let rect = Rect::new(
                corners[0] as f32,
                corners[1] as f32,
                corners[2] as f32,
                corners[3] as f32,
            );
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                return Err(format!("item {id} icon rectangle"));
            }
            // Ordinary item-name/info indices: 0x00606130 / 0x00607960.
            // Special late-game aliases 0x735..0x740 are not part of this
            // opening projection; their GUI tag substitutions remain open.
            items.push(Item {
                name: names.message(id as u32).ok_or("item name missing")?.to_owned(),
                description: info.message(id as u32).ok_or("item info missing")?.to_owned(),
                icon: rect,
                category: record.category_type(),
            });
        }
        Ok(Self {
            items,
            use_label: messages.message(2).ok_or("Use message missing")?.to_owned(),
        })
    }

    /// Native category lists 0x0066EEC0 -> 0x0045FD80. Keep real bag indices
    /// for use; display order currently preserves bag order (native sort and
    /// placeholder/equipment exclusion are explicitly not reproduced).
    fn entries(&self, bag: &Bag, category: u32) -> Vec<Entry> {
        bag.slots
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, slot)| {
                let item = self.items.get(usize::try_from(slot.item).ok()?)?;
                (slot.num > 0 && item.category == category).then_some(Entry { index, slot })
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BagView {
    pub owner: Owner,
    pub bag: Bag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub bags: Vec<BagView>,
}

#[derive(Default, Clone, Copy)]
struct Input {
    dx: i8,
    dy: i8,
    category: i8,
    bag: i8,
    use_item: bool,
    cancel: bool,
}

#[derive(Default)]
struct Selection {
    bag: usize,
    category: u32,
    cursor: usize,
    stick: [i8; 2],
    error: String,
}

impl Selection {
    fn apply(&mut self, input: Input, bags: &[BagView], catalog: &Catalog) -> Option<(Owner, Entry)> {
        if bags.is_empty() {
            return None;
        }
        // Category carousel wraps, 0x0060A510; bag tabs likewise use the
        // active-member list from 0x00669990. Quest-tab execution is unported.
        if input.bag != 0 {
            self.bag = (self.bag as isize + input.bag as isize).rem_euclid(bags.len() as isize) as usize;
            self.cursor = 0;
        }
        self.bag = self.bag.min(bags.len() - 1);
        if input.category != 0 {
            self.category = (self.category as i32 + input.category as i32).rem_euclid(12) as u32;
            self.cursor = 0;
        }
        let rows = catalog.entries(&bags[self.bag].bag, self.category);
        if rows.is_empty() {
            self.cursor = 0;
            return None;
        }
        self.cursor = self.cursor.min(rows.len() - 1);
        // Four-column grid, 0x0066A550. Exact hold repeat/wrap timing is
        // unported; viewer discrete navigation clamps its ends.
        let next = self.cursor as isize + input.dx as isize + input.dy as isize * 4;
        self.cursor = next.clamp(0, rows.len() as isize - 1) as usize;
        input
            .use_item
            .then(|| (bags[self.bag].owner, rows[self.cursor].clone()))
    }
}

#[derive(Resource, Default)]
pub(crate) struct Menus {
    owners: BTreeMap<PlayerId, Selection>,
    active: Option<PlayerId>,
    capture_checked: bool,
}

impl Menus {
    pub(crate) fn open(&mut self, seat: PlayerId) {
        self.owners.entry(seat).or_default();
        self.active = Some(seat);
    }
    pub(crate) fn blocks(&self, seat: PlayerId) -> bool {
        self.owners.contains_key(&seat)
    }
    pub(crate) fn close(&mut self, seat: PlayerId) {
        self.owners.remove(&seat);
        if self.active == Some(seat) {
            self.active = self.owners.keys().next_back().copied();
        }
    }
}

#[derive(Resource)]
pub(crate) struct View {
    catalog: Catalog,
    gui: Vec<u8>,
    font: Vec<u8>,
    textures: HashMap<String, Handle<Image>>,
    entities: Vec<Entity>,
    shown: Option<(PlayerId, usize, u32, usize, String, Snapshot, [u32; 2])>,
}

impl View {
    pub(crate) fn load(
        vfs: &ddvfs::Vfs,
        native: &std::path::Path,
        images: &mut Assets<Image>,
    ) -> Result<Self, String> {
        let catalog = Catalog::load(vfs, native)?;
        let gui = vfs.read(LAYOUT, mtarc::type_hash("rGUI"))?;
        let parsed = mtgui::Gui::parse(&gui)?;
        let font_path = parsed
            .resources
            .iter()
            .find(|r| r.class == "rGUIFont")
            .and_then(|r| r.path)
            .ok_or("item font missing")?;
        let font = vfs.read(font_path, mtarc::type_hash("rGUIFont"))?;
        let bitmap = mtgui::font::Font::parse(&font)?;
        let mut paths: Vec<String> = parsed
            .resources
            .iter()
            .filter(|r| r.class == "rTexture")
            .filter_map(|r| r.path.map(str::to_owned))
            .collect();
        // Only pages used by the English install's messages and numeric counts;
        // never load every Japanese font page just to draw the opening bag.
        for text in catalog
            .items
            .iter()
            .flat_map(|i| [&i.name, &i.description])
            .chain([&catalog.use_label])
        {
            for c in text.chars().filter(|c| !c.is_control()) {
                if let Some(g) = bitmap.glyph(c) {
                    paths.push(bitmap.texture_path(g.page)?);
                }
            }
        }
        for c in "0123456789".chars() {
            paths.push(bitmap.texture_path(bitmap.glyph(c).ok_or("item count glyph missing")?.page)?);
        }
        paths.sort();
        paths.dedup();
        let mut textures = HashMap::new();
        for path in paths {
            textures.insert(
                path.clone(),
                images.add(crate::stage::tex_to_image(
                    &vfs.read(&path, mtarc::type_hash("rTexture"))?,
                )?),
            );
        }
        Ok(Self {
            catalog,
            gui,
            font,
            textures,
            entities: Vec::new(),
            shown: None,
        })
    }
}

pub(crate) fn controls(
    opening: Option<Res<crate::opening::OpeningRes>>,
    view: Option<Res<View>>,
    mut menus: ResMut<Menus>,
    mut pause: ResMut<crate::pause::PauseState>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
) {
    let (Some(opening), Some(view)) = (opening, view) else {
        return;
    };
    // Capture driver opens only when an authored reward actually supplied a
    // curative; it does not grant items or bypass normal use validation.
    if !menus.capture_checked && std::env::var_os("DDVIEW_ITEM_MENU_CAPTURE").is_some() {
        if let Some(data) = opening.item_menu_snapshot(0) {
            if data
                .bags
                .iter()
                .any(|b| !view.catalog.entries(&b.bag, 0).is_empty())
            {
                menus.open(0);
                pause.paused = false;
                menus.capture_checked = true;
                info!("item menu capture: real rewarded bag opened for seat 0");
            }
        }
    }
    let mut ordered: Vec<_> = pads.iter().collect();
    ordered.sort_by_key(|(e, _)| e.to_bits());
    // Viewer keyboard bindings. The original PC binding remap/repeat table
    // remains unported; gamepads use the existing deterministic seat order.
    for pad_index in 0..ordered.len().max(2) {
        let Ok(seat) = PlayerId::try_from(pad_index) else {
            continue;
        };
        let open = if seat == 0 {
            keys.just_pressed(KeyCode::KeyI)
        } else {
            keys.just_pressed(KeyCode::NumpadDivide) && seat == 1
        } || ordered
            .get(pad_index)
            .is_some_and(|(_, p)| p.just_pressed(GamepadButton::Start));
        if open && opening.item_menu_snapshot(seat).is_some() {
            if menus.blocks(seat) {
                menus.close(seat);
            } else {
                menus.open(seat);
                pause.paused = false;
            }
        }
    }
    let owners: Vec<_> = menus.owners.keys().copied().collect();
    for owner in owners {
        let Some(data) = opening.item_menu_snapshot(owner) else {
            menus.close(owner);
            continue;
        };
        let selection = menus.owners.get_mut(&owner).unwrap();
        let mut input = if owner == 0 {
            Input {
                dx: i8::from(keys.just_pressed(KeyCode::ArrowRight))
                    - i8::from(keys.just_pressed(KeyCode::ArrowLeft)),
                dy: i8::from(keys.just_pressed(KeyCode::ArrowDown))
                    - i8::from(keys.just_pressed(KeyCode::ArrowUp)),
                category: i8::from(keys.just_pressed(KeyCode::PageDown))
                    - i8::from(keys.just_pressed(KeyCode::PageUp)),
                bag: i8::from(keys.just_pressed(KeyCode::Tab)),
                use_item: keys.just_pressed(KeyCode::Enter),
                cancel: keys.just_pressed(KeyCode::Backspace),
            }
        } else if owner == 1 {
            Input {
                dx: i8::from(keys.just_pressed(KeyCode::Numpad6))
                    - i8::from(keys.just_pressed(KeyCode::Numpad4)),
                dy: i8::from(keys.just_pressed(KeyCode::Numpad2))
                    - i8::from(keys.just_pressed(KeyCode::Numpad8)),
                category: i8::from(keys.just_pressed(KeyCode::Numpad3))
                    - i8::from(keys.just_pressed(KeyCode::Numpad9)),
                bag: i8::from(keys.just_pressed(KeyCode::NumpadMultiply)),
                use_item: keys.just_pressed(KeyCode::NumpadEnter),
                cancel: keys.just_pressed(KeyCode::NumpadSubtract),
                ..default()
            }
        } else {
            Input::default()
        };
        if let Some((_, pad)) = ordered.get(usize::from(owner)) {
            let axis = |a| {
                let v = pad.get(a).unwrap_or(0.0);
                if v > 0.5 {
                    1
                } else if v < -0.5 {
                    -1
                } else {
                    0
                }
            };
            let stick = [axis(GamepadAxis::LeftStickX), -axis(GamepadAxis::LeftStickY)];
            input.dx += i8::from(pad.just_pressed(GamepadButton::DPadRight))
                - i8::from(pad.just_pressed(GamepadButton::DPadLeft));
            input.dy += i8::from(pad.just_pressed(GamepadButton::DPadDown))
                - i8::from(pad.just_pressed(GamepadButton::DPadUp));
            for (i, delta) in [&mut input.dx, &mut input.dy].into_iter().enumerate() {
                if stick[i] != selection.stick[i] {
                    *delta += stick[i];
                }
            }
            selection.stick = stick;
            input.category += i8::from(pad.just_pressed(GamepadButton::RightTrigger))
                - i8::from(pad.just_pressed(GamepadButton::LeftTrigger));
            input.bag += i8::from(pad.just_pressed(GamepadButton::RightTrigger2))
                - i8::from(pad.just_pressed(GamepadButton::LeftTrigger2));
            input.use_item |= pad.just_pressed(GamepadButton::South);
            input.cancel |= pad.just_pressed(GamepadButton::East);
        }
        if input.cancel {
            menus.close(owner);
            continue;
        }
        if let Some((bag_owner, row)) = selection.apply(input, &data.bags, &view.catalog) {
            let result = opening.item_menu_use(owner, bag_owner, &row);
            selection.error = result.err().unwrap_or_default();
        }
    }
}

struct Painter<'a, 'w, 's, 'f> {
    commands: &'a mut Commands<'w, 's>,
    textures: &'a HashMap<String, Handle<Image>>,
    entities: &'a mut Vec<Entity>,
    scale: f32,
    offset: Vec2,
    font: &'a mtgui::font::Font<'f>,
}

impl Painter<'_, '_, '_, '_> {
    fn image(&self, path: &str) -> Result<Handle<Image>, String> {
        self.textures
            .get(path)
            .cloned()
            .ok_or_else(|| format!("item menu texture {path} not prepared"))
    }

    fn quad(&mut self, pos: Vec2, size: Vec2, image: Handle<Image>, rect: Rect, color: Color, layer: i32) {
        let pos = self.offset + pos * self.scale;
        self.entities.push(
            self.commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(pos.x),
                        top: Val::Px(pos.y),
                        width: Val::Px(size.x * self.scale),
                        height: Val::Px(size.y * self.scale),
                        ..default()
                    },
                    ImageNode {
                        image,
                        rect: Some(rect),
                        color,
                        ..default()
                    },
                    GlobalZIndex(layer),
                ))
                .id(),
        );
    }

    fn node(
        &mut self,
        gui: &mtgui::Gui<'_>,
        index: usize,
        translation: Vec2,
        text: &[(u32, &str)],
        icon: Option<Rect>,
        layer: i32,
        selector: Option<u32>,
    ) -> Result<(), String> {
        let animation = gui
            .local_animation(index)
            .ok_or("item menu node has no local animation")?;
        let transform = node_transform(gui, index)?;
        let frame = match selector {
            Some(selector) => gui
                .control_range(index, selector)?
                .ok_or("item menu selector absent")?[0] as u32,
            None => 0,
        };
        let regions = gui.texture_regions()?;
        let objects = gui.animation_objects(animation.name)?;
        let matrices = object_transforms(gui, animation, &objects)?;
        for (ordinal, object) in objects.iter().enumerate() {
            // 0x0066AB10 toggles optional overlays; those adapters remain open.
            if icon.is_some() && !matches!(object.id, 7 | 3) {
                continue;
            }
            if !object.visible_at(frame) {
                continue;
            }
            let vector = |name: &str| {
                object
                    .vector_keys
                    .iter()
                    .filter(|(key, time, _)| *key == name && *time <= frame)
                    .last()
                    .map(|(_, _, value)| *value)
                    .or_else(|| object.property(name).and_then(|p| gui.vector(p)))
            };
            let Some(size) = vector("Size") else { continue };
            let size = Vec2::new(size[0], size[1]);
            let anchor = Vec2::from_array(mtgui::layout::anchor_offset(
                size.to_array(),
                object.integer("ControlPoint").unwrap_or(0),
            ));
            let object_transform = transform * matrices[ordinal];
            let position = object_transform.transform_point2(anchor) + translation;
            let layer = layer + object.integer("Priority").unwrap_or(0) as i32;
            if let Some((_, text)) = text.iter().find(|(id, _)| *id == object.id) {
                let font_size = object
                    .property("FontSize")
                    .and_then(|p| gui.vector(p))
                    .ok_or("item menu text size")?;
                let color = vector("Color").unwrap_or([255.0; 4]);
                let color = Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                );
                let text = text_lines(text);
                for (row, line) in text.split('\n').enumerate() {
                    let glyphs = self.font.line(line, [font_size[0], font_size[1]])?;
                    let width: f32 = line
                        .chars()
                        .filter_map(|c| self.font.glyph(c))
                        .map(|g| g.advance as f32 * font_size[0] / self.font.nominal_size as f32)
                        .sum();
                    let align = match object.integer("LetterHAlign").unwrap_or(0) {
                        1 => (size.x - width) * 0.5,
                        2 => size.x - width,
                        _ => 0.0,
                    };
                    let height = font_size[1] + object.integer("LineSpace").unwrap_or(0) as f32;
                    for placed in glyphs {
                        let g = placed.glyph;
                        let image = self.image(&self.font.texture_path(g.page)?)?;
                        self.quad(
                            position
                                + Vec2::new(align, row as f32 * height)
                                + Vec2::from_array(placed.position),
                            Vec2::from_array(placed.size),
                            image,
                            Rect::new(
                                g.x as f32,
                                g.y as f32,
                                (g.x + g.width) as f32,
                                (g.y + g.height) as f32,
                            ),
                            color,
                            layer,
                        );
                    }
                }
            } else if let Some(texture) = object
                .texture_keys
                .iter()
                .filter(|(time, _)| *time <= frame)
                .last()
                .map(|(_, id)| *id)
                .or_else(|| object.integer("Texture"))
                .filter(|&id| id != u32::MAX)
            {
                let region = regions
                    .iter()
                    .find(|r| r.id == texture)
                    .ok_or("item menu atlas absent")?;
                let image = self.image(region.path)?;
                let mut rect = vector("mTextureRect").unwrap_or(region.rect);
                if object.id == 7 {
                    if let Some(icon) = icon {
                        rect = [icon.min.x, icon.min.y, icon.max.x, icon.max.y];
                    }
                }
                if rect[2] <= rect[0] || rect[3] <= rect[1] {
                    continue;
                } // inactive authored placeholder
                let color = vector("VertexColor").unwrap_or([255.0; 4]);
                let scale = object_transform.matrix2 * size;
                self.quad(
                    position,
                    scale.abs(),
                    image,
                    Rect::new(rect[0], rect[1], rect[2], rect[3]),
                    Color::srgba(
                        color[0] / 255.0,
                        color[1] / 255.0,
                        color[2] / 255.0,
                        color[3] / 255.0,
                    ),
                    layer,
                );
            }
        }
        Ok(())
    }
}

impl Painter<'_, '_, '_, '_> {
    fn message_node(
        &mut self,
        gui: &mtgui::Gui<'_>,
        index: usize,
        text: &str,
        layer: i32,
    ) -> Result<(), String> {
        let vector = |key| {
            gui.node_property(index, key)
                .and_then(|p| gui.vector(p))
                .ok_or_else(|| format!("item text node {index}: {key}"))
        };
        let font_size = vector("FontSize")?;
        let size = vector("Size")?;
        let color = vector("Color")?;
        let transform = node_transform(gui, index)?;
        let position = transform.transform_point2(Vec2::ZERO);
        let color = Color::srgba(
            color[0] / 255.0,
            color[1] / 255.0,
            color[2] / 255.0,
            color[3] / 255.0,
        );
        let align = gui
            .node_property(index, "LetterHAlign")
            .and_then(|p| p.word)
            .unwrap_or(0);
        let line_space = gui
            .node_property(index, "LineSpace")
            .and_then(|p| p.word)
            .unwrap_or(0) as f32;
        for (row, line) in text_lines(text).split('\n').enumerate() {
            let glyphs = self.font.line(line, [font_size[0], font_size[1]])?;
            let width: f32 = line
                .chars()
                .filter_map(|c| self.font.glyph(c))
                .map(|g| g.advance as f32 * font_size[0] / self.font.nominal_size as f32)
                .sum();
            let dx = match align {
                1 => (size[0] - width) * 0.5,
                2 => size[0] - width,
                _ => 0.0,
            };
            for placed in glyphs {
                let g = placed.glyph;
                self.quad(
                    position
                        + Vec2::new(dx, row as f32 * (font_size[1] + line_space))
                        + Vec2::from_array(placed.position),
                    Vec2::from_array(placed.size),
                    self.image(&self.font.texture_path(g.page)?)?,
                    Rect::new(
                        g.x as f32,
                        g.y as f32,
                        (g.x + g.width) as f32,
                        (g.y + g.height) as f32,
                    ),
                    color,
                    layer,
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn draw(
    mut commands: Commands,
    opening: Option<Res<crate::opening::OpeningRes>>,
    menus: Res<Menus>,
    view: Option<ResMut<View>>,
    windows: Query<&Window>,
) {
    let (Some(opening), Some(mut view)) = (opening, view) else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let snapshot = menus.active.and_then(|owner| {
        let selection = menus.owners.get(&owner)?;
        let data = opening.item_menu_snapshot(owner)?;
        Some((
            owner,
            selection.bag,
            selection.category,
            selection.cursor,
            selection.error.clone(),
            data,
            [window.width().to_bits(), window.height().to_bits()],
        ))
    });
    if view.shown == snapshot {
        return;
    }
    view.shown = snapshot.clone();
    for entity in view.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    let Some((owner, bag, category, cursor, _, data, _)) = snapshot else {
        return;
    };
    let View {
        catalog,
        gui,
        font,
        textures,
        entities,
        ..
    } = &mut *view;
    let mut render = || -> Result<(), String> {
        let gui = mtgui::Gui::parse(gui)?;
        let font = mtgui::font::Font::parse(font)?;
        let scale = (window.width() / gui.width as f32).min(window.height() / gui.height as f32);
        let offset = Vec2::new(
            (window.width() - gui.width as f32 * scale) * 0.5,
            (window.height() - gui.height as f32 * scale) * 0.5,
        );
        let mut painter = Painter {
            commands: &mut commands,
            textures,
            entities,
            scale,
            offset,
            font: &font,
        };
        // Static native right-hand book, bag panel, and description backdrop.
        // The category carousel's external GUI/filter VM remains unported.
        painter.node(&gui, 4, Vec2::ZERO, &[], None, 500, None)?;
        painter.node(&gui, 55, Vec2::ZERO, &[], None, 510, None)?;
        painter.node(&gui, 10, Vec2::ZERO, &[], None, 515, None)?;
        let Some(bag) = data.bags.get(bag.min(data.bags.len().saturating_sub(1))) else {
            return Ok(());
        };
        let rows = catalog.entries(&bag.bag, category);
        let cursor = cursor.min(rows.len().saturating_sub(1));
        let first = (cursor / 4).saturating_sub(2) * 4;
        // Source grid construction 0x0066A190: nodes43/44/45 define origin
        // and axis offsets, attached to node42. No tuned row spacing.
        let origin = node_transform(&gui, 43)?.transform_point2(Vec2::ZERO);
        let dx = node_transform(&gui, 44)?.transform_point2(Vec2::ZERO) - origin;
        let dy = node_transform(&gui, 45)?.transform_point2(Vec2::ZERO) - origin;
        let icon_origin = node_transform(&gui, 11)?.transform_point2(Vec2::ZERO);
        for (visible, row) in rows.iter().skip(first).take(12).enumerate() {
            let item = &catalog.items[row.slot.item as usize];
            let pos = origin + dx * (visible % 4) as f32 + dy * (visible / 4) as f32;
            let count = row.slot.num.to_string();
            painter.node(
                &gui,
                11,
                pos - icon_origin,
                &[(3, &count)],
                Some(item.icon),
                530,
                None,
            )?;
            if first + visible == cursor {
                let cursor_origin = node_transform(&gui, 41)?.transform_point2(Vec2::ZERO);
                painter.node(&gui, 41, pos - cursor_origin, &[], None, 550, None)?;
            }
        }
        if let Some(row) = rows.get(cursor) {
            let item = &catalog.items[row.slot.item as usize];
            painter.message_node(&gui, 2, &item.name, 560)?;
            painter.message_node(&gui, 8, &item.description, 560)?;
            // Native Use entry local text object5 (0x0066BE90 / 0x0066C750).
            painter.node(&gui, 37, Vec2::ZERO, &[(5, &catalog.use_label)], None, 570, None)?;
        }
        info!(
            "item menu: owner {owner}, bag {:?}, category {category}, {} real slots",
            bag.owner,
            rows.len()
        );
        Ok(())
    };
    if let Err(error) = render() {
        error!("item menu render: {error}");
    }
}

fn node_transform(gui: &mtgui::Gui<'_>, wanted: usize) -> Result<Affine2, String> {
    let mut stack = Vec::new();
    for (index, depth) in gui.preorder()? {
        stack.truncate(depth);
        let scalar = |name, default| {
            gui.node_property(index, name)
                .and_then(|p| p.word)
                .map(f32::from_bits)
                .unwrap_or(default)
        };
        let local = Affine2::from_scale_angle_translation(
            Vec2::new(scalar("SclX", 1.0), scalar("SclY", 1.0)),
            scalar("Rotation", 0.0).to_radians(),
            Vec2::new(scalar("PosX", 0.0), scalar("PosY", 0.0)),
        );
        let transform = stack.last().copied().unwrap_or(Affine2::IDENTITY) * local;
        if index == wanted {
            return Ok(transform);
        }
        stack.push(transform);
    }
    Err("item menu node absent".into())
}

/// Translated from 0x01077EC0 (DDDA.exe 2017-12-13 Steam): object +8
/// links the next sibling (0x01068C60), +C links the first child
/// (0x01068C40). Parent transforms must apply to the book and tab objects.
fn object_transforms(
    gui: &mtgui::Gui<'_>,
    animation: &mtgui::Animation<'_>,
    objects: &[mtgui::layout::Object<'_>],
) -> Result<Vec<Affine2>, String> {
    let first = animation.words[3] as usize;
    let records = gui.section(0xa4).ok_or("item object records absent")?.bytes;
    let mut out = vec![Affine2::IDENTITY; objects.len()];
    let mut visited = vec![false; objects.len()];
    let mut stack = vec![(0usize, Affine2::IDENTITY)];
    while let Some((index, parent)) = stack.pop() {
        if index >= objects.len() || visited[index] {
            return Err("item object hierarchy cycle/index".into());
        }
        visited[index] = true;
        let object = &objects[index];
        let local = Affine2::from_scale_angle_translation(
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
        out[index] = parent * local;
        for (offset, matrix) in [(8, parent), (12, out[index])] {
            let p = (first + index)
                .checked_mul(32)
                .and_then(|p| p.checked_add(offset))
                .ok_or("item object overflow")?;
            let value = u32::from_le_bytes(
                records
                    .get(p..p + 4)
                    .ok_or("item object link truncated")?
                    .try_into()
                    .unwrap(),
            );
            if value != u32::MAX {
                let relative = (value as usize)
                    .checked_sub(first)
                    .ok_or("item object link before animation")?;
                stack.push((relative, matrix));
            }
        }
    }
    if visited.iter().any(|v| !v) {
        return Err("item object not reachable".into());
    }
    Ok(out)
}

/// Installed GMD strings preserve CRLF. Treat each authored line ending as
/// one break; replacing CR alone first would invent blank lines and overflow
/// the description's authored 83-unit box.
fn text_lines(text: &str) -> String {
    text.replace("\r\r\n", "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn native() -> Option<std::path::PathBuf> {
        let path = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        path.is_dir().then_some(path)
    }

    #[test]
    fn installed_reward_rows_keep_bag_indices_and_owner_selection() {
        let Some(native) = native() else {
            return;
        };
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let catalog = Catalog::load(&vfs, &native).unwrap();
        // Pure projection fixture: runtime reward data enters headless ECON,
        // and the viewer receives only a cloned bag with its owner identity.
        let mut economy = ddeconomy::Economy::default();
        economy.inventory.bags.insert(
            Owner::Arisen(7),
            ddeconomy::new_game::bag(ddeconomy::new_game::Phase::NewlyArisen),
        );
        let items = ddhost::items::ItemData::load(&vfs).unwrap();
        let story = ddstory::data::InstallData::open(&native, None).unwrap();
        for request in ddstory::orders::reward(&story, 1).unwrap() {
            if let ddstory::Request::Reward {
                quest, items: grants, ..
            } = request
            {
                economy
                    .opening_grants
                    .reward(&mut economy.inventory, &items.items, 7, quest, &grants);
            }
        }
        let bags = vec![BagView {
            owner: Owner::Arisen(7),
            bag: economy.inventory.bag(Owner::Arisen(7)).unwrap().clone(),
        }];
        assert_eq!(bags.len(), 1);
        assert_eq!(bags[0].owner, Owner::Arisen(7));
        let rows = catalog.entries(&bags[0].bag, 0);
        assert!(rows.iter().any(|row| row.slot.item == 43 && row.slot.num == 5));
        for row in &rows {
            assert_eq!(bags[0].bag.slots[row.index], row.slot);
        }
        assert_eq!(catalog.items[43].name, "Greenwarish");
        assert_eq!(catalog.items[43].icon, Rect::new(704.0, 64.0, 768.0, 128.0));
        assert!(!catalog.items[43].description.is_empty());
        let data = Snapshot { bags };
        let mut menus = Menus::default();
        menus.open(7);
        menus.open(1);
        assert!(menus.blocks(7) && menus.blocks(1) && !menus.blocks(0));
        let selected = menus
            .owners
            .get_mut(&7)
            .unwrap()
            .apply(
                Input {
                    use_item: true,
                    ..default()
                },
                &data.bags,
                &catalog,
            )
            .unwrap();
        assert_eq!(selected.0, Owner::Arisen(7));
        assert_eq!(selected.1.slot, data.bags[0].bag.slots[selected.1.index]);
        menus.close(1);
        assert!(menus.blocks(7) && !menus.blocks(1));
    }

    #[test]
    fn installed_book_applies_parent_transform_and_rejects_bad_links() {
        let Some(native) = native() else {
            return;
        };
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let bytes = vfs.read(LAYOUT, mtarc::type_hash("rGUI")).unwrap();
        let gui = mtgui::Gui::parse(&bytes).unwrap();
        let animation = gui.local_animation(4).unwrap();
        let objects = gui.animation_objects(animation.name).unwrap();
        let transforms = object_transforms(&gui, animation, &objects).unwrap();
        let image = objects.iter().position(|o| o.id == 3).unwrap();
        assert_eq!(
            transforms[image].transform_point2(Vec2::ZERO),
            Vec2::new(26.0, -98.0)
        );
        let mut corrupt = bytes.clone();
        let offset = gui.section(0xa4).unwrap().offset + animation.words[3] as usize * 32 + 8;
        corrupt[offset..offset + 4].copy_from_slice(&animation.words[3].to_le_bytes());
        let corrupt = mtgui::Gui::parse(&corrupt).unwrap();
        let animation = corrupt.local_animation(4).unwrap();
        let objects = corrupt.animation_objects(animation.name).unwrap();
        assert!(object_transforms(&corrupt, animation, &objects).is_err());
    }

    #[test]
    fn installed_menu_assets_draw_real_icons_and_bitmap_text_headlessly() {
        let Some(native) = native() else {
            return;
        };
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let mut images = Assets::<Image>::default();
        let mut view = View::load(&vfs, &native, &mut images).unwrap();
        let gui = mtgui::Gui::parse(&view.gui).unwrap();
        let font = mtgui::font::Font::parse(&view.font).unwrap();
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut painter = Painter {
            commands: &mut commands,
            textures: &view.textures,
            entities: &mut view.entities,
            scale: 1.0,
            offset: Vec2::ZERO,
            font: &font,
        };
        painter.node(&gui, 4, Vec2::ZERO, &[], None, 500, None).unwrap();
        painter
            .node(
                &gui,
                11,
                Vec2::ZERO,
                &[(3, "5")],
                Some(view.catalog.items[43].icon),
                530,
                None,
            )
            .unwrap();
        painter
            .message_node(&gui, 2, &view.catalog.items[43].name, 550)
            .unwrap();
        painter
            .message_node(&gui, 8, &view.catalog.items[43].description, 550)
            .unwrap();
        drop(painter);
        drop(commands);
        queue.apply(&mut world);
        assert!(view.entities.len() > 25);
        let mut found_icon = false;
        for entity in view.entities {
            let image = world.get::<ImageNode>(entity).unwrap();
            assert!(images.get(&image.image).is_some());
            let rect = image.rect.unwrap();
            assert!(rect.width() > 0.0 && rect.height() > 0.0);
            found_icon |= rect == view.catalog.items[43].icon;
        }
        assert!(found_icon);
    }
}
