//! First-choice uGUIJobChange. Layouts, atlas pixels, fonts and messages are
//! read from the install; this projects its constant objects, not its GUI VM.
use bevy::math::Affine2;
use bevy::prelude::*;
use std::collections::HashMap;

const LAYOUT: &str = "id/DDN/guild/gild_jobchn"; // vf58 0x00672A70
const COMMON: &str = "id/DDN/message/common/common_message_eng";
const GUILD: &str = "id/message/shop/shop_guild/shop_guild_eng";

#[derive(Default)]
pub(crate) struct Selection {
    owner: Option<usize>,
    cursor: usize,
    confirmation: bool,
    yes: bool,
    stick: i8,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Input {
    pub up: bool,
    pub down: bool,
    pub confirm: bool,
    pub cancel: bool,
}

impl Selection {
    pub(crate) fn snapshot(&self) -> Option<(usize, usize, bool, bool)> {
        self.owner
            .map(|owner| (owner, self.cursor, self.confirmation, self.yes))
    }
    pub(crate) fn close(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn open(&mut self, owner: usize) {
        if self.owner != Some(owner) {
            *self = Self {
                owner: Some(owner),
                yes: true,
                ..Self::default()
            };
        }
    }

    // Translated from 0x00603E40/0x00603F80 (DDDA.exe 2017-12-13 Steam).
    // List ctor 0x00600560 clears +0x86/+0x87: its ends do not wrap.
    // First-choice vf05 0x00671B70 omits the list's cancel callback.
    // vf55 0x00672C10 cases 10/11: positive cost opens Yes/No; rejection
    // resumes the list, acceptance commits that same selected vocation.
    pub(crate) fn apply(&mut self, input: Input, jobs: &[u32], costs: &[u16]) -> Option<u32> {
        if self.owner.is_none() || jobs.is_empty() {
            return None;
        }
        self.cursor = self.cursor.min(jobs.len() - 1);
        if self.confirmation {
            if input.cancel {
                self.confirmation = false;
                return None;
            }
            if input.up != input.down {
                self.yes = !self.yes;
            }
            if input.confirm {
                self.confirmation = false;
                return self.yes.then_some(jobs[self.cursor]);
            }
        } else {
            if input.up != input.down {
                self.cursor = if input.up {
                    self.cursor.saturating_sub(1)
                } else {
                    (self.cursor + 1).min(jobs.len() - 1)
                };
            }
            if input.confirm {
                if costs.get(self.cursor).copied().unwrap_or(0) > 0 {
                    self.confirmation = true;
                    self.yes = true;
                } else {
                    return Some(jobs[self.cursor]);
                }
            }
        }
        None
    }

    pub(crate) fn controls(
        &mut self,
        keys: &ButtonInput<KeyCode>,
        pads: &Query<(Entity, &Gamepad)>,
    ) -> Input {
        let owner = self.owner.unwrap_or(0);
        // Viewer bindings, matching local-seat ordering in play.rs. Native
        // keyboard scan/repeat timing is not yet reproduced.
        let mut input = if owner == 0 {
            Input {
                up: keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW),
                down: keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS),
                confirm: keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::KeyE),
                cancel: keys.just_pressed(KeyCode::Backspace),
            }
        } else {
            Input {
                up: keys.just_pressed(KeyCode::Numpad8),
                down: keys.just_pressed(KeyCode::Numpad2),
                confirm: keys.just_pressed(KeyCode::NumpadEnter),
                cancel: keys.just_pressed(KeyCode::NumpadSubtract),
            }
        };
        let mut ordered: Vec<_> = pads.iter().collect();
        ordered.sort_by_key(|(entity, _)| entity.to_bits());
        if let Some((_, pad)) = ordered.get(owner) {
            // Viewer choice: discrete stick edges at 0.5; no fabricated native
            // hold-repeat timer. D-pad and the usual South/East buttons work.
            let y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
            let stick = if y > 0.5 {
                1
            } else if y < -0.5 {
                -1
            } else {
                0
            };
            input.up |= pad.just_pressed(GamepadButton::DPadUp) || (stick == 1 && self.stick != 1);
            input.down |= pad.just_pressed(GamepadButton::DPadDown) || (stick == -1 && self.stick != -1);
            input.confirm |= pad.just_pressed(GamepadButton::South);
            input.cancel |= pad.just_pressed(GamepadButton::East);
            self.stick = stick;
        }
        input
    }
}

pub(crate) struct Choice {
    job: u32,
    cost: u16,
    name: String,
    description: String,
    node: usize,
}

pub(crate) struct Catalog {
    choices: Vec<Choice>,
    prompt: String,
    answers: [String; 2],
    dialog_nodes: [usize; 3],
}

impl Catalog {
    fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let common = vfs.read(COMMON, mtarc::type_hash("rGUIMessage"))?;
        let common = mtgmd::MessageFile::parse(&common).map_err(|e| e.to_string())?;
        let guild = vfs.read(GUILD, mtarc::type_hash("rGUIMessage"))?;
        let guild = mtgmd::MessageFile::parse(&guild).map_err(|e| e.to_string())?;
        let message = |file: &mtgmd::MessageFile<'_>, index| {
            file.message(index)
                .map(str::to_owned)
                .ok_or_else(|| format!("vocation message {index} absent"))
        };
        let mut choices = Vec::new();
        // Source vf05 0x00671B70 / list creation 0x00675780, display update
        // 0x00673BF0. Bulk list, text indices and node indices remain installed
        // tables; only the supported first-choice count is code here.
        for (slot, job) in exe
            .u32s(0x01517A0C, 3)
            .map_err(String::from)?
            .into_iter()
            .enumerate()
        {
            if ddhost::ddskill::vocation::Vocation::from_id(job).is_none() {
                return Err("invalid first-choice job".into());
            }
            choices.push(Choice {
                job,
                cost: exe.u16(0x01517A7C + job * 2).map_err(String::from)?,
                name: message(&common, exe.u32(0x01517A90 + job * 4).map_err(String::from)?)?,
                description: message(&guild, exe.u32(0x01582724 + job * 4).map_err(String::from)?)?,
                node: exe.u32(0x0158269C + slot as u32 * 4).map_err(String::from)? as usize,
            });
        }
        // uGUISystemWindow: 0x00728570 chooses style 6; 0x00728760
        // selects its installed 48-byte descriptor. vf05 0x00728890 binds
        // the prompt and two answer instances from that descriptor.
        let row = 0x01516068 + 6 * 0x30;
        Ok(Self {
            choices,
            prompt: message(&guild, 0x1b)?,
            answers: [message(&common, 0x5c)?, message(&common, 0x5d)?],
            dialog_nodes: [
                exe.u32(row + 12).map_err(String::from)? as usize,
                exe.u32(row + 24).map_err(String::from)? as usize,
                exe.u32(row + 32).map_err(String::from)? as usize,
            ],
        })
    }

    pub(crate) fn input(&self, selection: &mut Selection, input: Input) -> Option<u32> {
        selection.apply(
            input,
            &self.choices.iter().map(|c| c.job).collect::<Vec<_>>(),
            &self.choices.iter().map(|c| c.cost).collect::<Vec<_>>(),
        )
    }

    /// Unattended viewer driver: feed the same list/confirm operations as a
    /// local device, rather than directly reporting a job to the host.
    pub(crate) fn demo_select(&self, selection: &mut Selection, job: u32) -> Result<(), String> {
        if selection.owner.is_none() {
            return Err("vocation menu has no owner".into());
        }
        let row = self
            .choices
            .iter()
            .position(|choice| choice.job == job)
            .ok_or_else(|| format!("job {job} is not offered by the installed first-choice list"))?;
        if selection.confirmation {
            return Ok(());
        }
        while selection.cursor != row {
            self.input(
                selection,
                Input {
                    up: selection.cursor > row,
                    down: selection.cursor < row,
                    ..default()
                },
            );
        }
        Ok(())
    }
}

#[derive(Resource)]
pub(crate) struct View {
    pub catalog: Catalog,
    gui: Vec<u8>,
    dialog: Vec<u8>,
    font: Vec<u8>,
    textures: HashMap<String, Handle<Image>>,
    entities: Vec<Entity>,
    shown: Option<(usize, usize, bool, bool, [u32; 2])>,
}

impl View {
    pub(crate) fn load(
        vfs: &ddvfs::Vfs,
        native_pc: &std::path::Path,
        images: &mut Assets<Image>,
    ) -> Result<Self, String> {
        let file = ddexe::ExeFile::next_to(native_pc).map_err(String::from)?;
        let catalog = Catalog::load(vfs, &file.image().map_err(String::from)?)?;
        let gui = vfs.read(LAYOUT, mtarc::type_hash("rGUI"))?;
        let parsed = mtgui::Gui::parse(&gui)?;
        let font = parsed
            .resources
            .iter()
            .find(|r| r.class == "rGUIFont")
            .and_then(|r| r.path)
            .ok_or("vocation font absent")?;
        let font = vfs.read(font, mtarc::type_hash("rGUIFont"))?;
        let dialog = vfs.read("id/common/sys_wind", mtarc::type_hash("rGUI"))?; // 0x00729110
        let dialog_gui = mtgui::Gui::parse(&dialog)?;
        let bitmap_font = mtgui::font::Font::parse(&font)?;
        let mut paths: Vec<String> = parsed
            .resources
            .iter()
            .chain(dialog_gui.resources.iter())
            .filter(|r| r.class == "rTexture")
            .filter_map(|r| r.path.map(str::to_owned))
            .collect();
        for text in catalog
            .choices
            .iter()
            .flat_map(|c| [&c.name, &c.description])
            .chain([&catalog.prompt, &catalog.answers[0], &catalog.answers[1]])
        {
            for character in text.chars().filter(|c| !c.is_control()) {
                let glyph = bitmap_font
                    .glyph(character)
                    .ok_or_else(|| format!("vocation font missing {character:?}"))?;
                paths.push(bitmap_font.texture_path(glyph.page)?);
            }
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
            dialog,
            font,
            textures,
            entities: Vec::new(),
            shown: None,
        })
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
            .ok_or_else(|| format!("vocation texture {path} not prepared"))
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
        layer: i32,
        selector: Option<u32>,
    ) -> Result<(), String> {
        let animation = gui
            .local_animation(index)
            .ok_or("vocation node has no local animation")?;
        let transform = node_transform(gui, index)?;
        let frame = match selector {
            Some(selector) => gui
                .control_range(index, selector)?
                .ok_or("vocation selector absent")?[0] as u32,
            None => 0,
        };
        let regions = gui.texture_regions()?;
        for object in gui.animation_objects(animation.name)? {
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
            let position = Vec2::new(
                object.scalar("PosX").unwrap_or(0.0),
                object.scalar("PosY").unwrap_or(0.0),
            );
            let Some(size) = vector("Size") else { continue };
            let size = Vec2::new(size[0], size[1]);
            let anchor = Vec2::from_array(mtgui::layout::anchor_offset(
                size.to_array(),
                object.integer("ControlPoint").unwrap_or(0),
            ));
            let position = transform.transform_point2(position + anchor) + translation;
            let layer = layer + object.integer("Priority").unwrap_or(0) as i32;
            if let Some((_, text)) = text.iter().find(|(id, _)| *id == object.id) {
                let font_size = object
                    .property("FontSize")
                    .and_then(|p| gui.vector(p))
                    .ok_or("vocation text size")?;
                let color = vector("Color").unwrap_or([255.0; 4]);
                let color = Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                );
                let text = text.replace("\r\r\n", "\n").replace('\r', "\n");
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
                    .ok_or("vocation atlas absent")?;
                let image = self.image(region.path)?;
                let rect = vector("mTextureRect").unwrap_or(region.rect);
                if rect[2] <= rect[0] || rect[3] <= rect[1] {
                    continue;
                } // inactive authored placeholder
                let color = vector("VertexColor").unwrap_or([255.0; 4]);
                let scale = transform.matrix2 * size;
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

pub(crate) fn update(
    mut commands: Commands,
    opening: Option<Res<crate::opening::OpeningRes>>,
    view: Option<ResMut<View>>,
    windows: Query<&Window>,
) {
    let (Some(opening), Some(mut view)) = (opening, view) else {
        return;
    };
    let Ok(window) = windows.single() else { return };
    let snapshot = opening.vocation_snapshot().map(|(owner, cursor, confirm, yes)| {
        (
            owner,
            cursor,
            confirm,
            yes,
            [window.width().to_bits(), window.height().to_bits()],
        )
    });
    if view.shown == snapshot {
        return;
    }
    view.shown = snapshot;
    for entity in view.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    let Some((_, cursor, confirmation, yes, _)) = snapshot else {
        return;
    };
    let View {
        catalog,
        gui,
        dialog,
        font,
        textures,
        entities,
        ..
    } = &mut *view;
    let mut render = || -> Result<(), String> {
        let gui = mtgui::Gui::parse(gui)?;
        let dialog = mtgui::Gui::parse(dialog)?;
        let font = mtgui::font::Font::parse(font)?;
        // Viewer viewport policy, as for the existing HUD: preserve authored
        // aspect and centre its virtual screen. Native animation/filter VM,
        // character preview and button legend are still outside this port.
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
        let selected = catalog
            .choices
            .get(cursor)
            .ok_or("vocation cursor outside list")?;
        painter.node(&gui, 1, Vec2::ZERO, &[(6, &selected.description)], 200, None)?;
        for choice in &catalog.choices {
            painter.node(&gui, choice.node, Vec2::ZERO, &[(2, &choice.name)], 220, None)?;
        }
        // Source 0x00673BF0: cursor anchor node38 + row * difference of
        // authored list rows; do not use the saved cursor's current row.
        let origin = node_transform(&gui, 38)?.translation;
        let spacing = node_transform(&gui, catalog.choices[1].node)?.translation.y
            - node_transform(&gui, catalog.choices[0].node)?.translation.y;
        let cursor_offset =
            origin - node_transform(&gui, 2)?.translation + Vec2::new(0.0, cursor as f32 * spacing);
        painter.node(&gui, 2, cursor_offset, &[], 210, None)?;
        if confirmation {
            painter.node(
                &dialog,
                catalog.dialog_nodes[0],
                Vec2::ZERO,
                &[(2, &catalog.prompt)],
                300,
                None,
            )?;
            for (index, label) in catalog.answers.iter().enumerate() {
                let node = catalog.dialog_nodes[index + 1];
                // vf05 0x00728890: selected/enabled = selector1, other/enabled
                // = selector0. Read its authored colour/texture key frame.
                painter.node(
                    &dialog,
                    node,
                    Vec2::ZERO,
                    &[(3, label)],
                    330,
                    Some(u32::from((index == 0) == yes)),
                )?;
            }
        }
        Ok(())
    };
    if let Err(error) = render() {
        error!("vocation menu: {error}");
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
    Err("vocation node absent".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_bounds_cancel_and_same_choice_confirmation() {
        let mut s = Selection::default();
        s.open(1);
        let jobs = [1, 2, 3];
        let costs = [1000; 3];
        s.apply(
            Input {
                up: true,
                cancel: true,
                ..default()
            },
            &jobs,
            &costs,
        );
        assert_eq!(s.cursor, 0);
        assert_eq!(s.owner, Some(1));
        for _ in 0..4 {
            s.apply(
                Input {
                    down: true,
                    ..default()
                },
                &jobs,
                &costs,
            );
        }
        assert_eq!(s.cursor, 2);
        assert_eq!(
            s.apply(
                Input {
                    confirm: true,
                    ..default()
                },
                &jobs,
                &costs
            ),
            None
        );
        assert!(s.confirmation);
        s.apply(
            Input {
                cancel: true,
                ..default()
            },
            &jobs,
            &costs,
        );
        assert!(!s.confirmation);
        assert_eq!(s.cursor, 2);
        s.apply(
            Input {
                confirm: true,
                ..default()
            },
            &jobs,
            &costs,
        );
        assert_eq!(
            s.apply(
                Input {
                    confirm: true,
                    ..default()
                },
                &jobs,
                &costs
            ),
            Some(3)
        );
        s.open(2);
        assert_eq!(s.cursor, 0);
        assert!(!s.confirmation);
    }
    #[test]
    fn all_three_jobs_and_no_rejection_side_effect() {
        for selected in 0..3 {
            let mut s = Selection::default();
            s.open(0);
            for _ in 0..selected {
                s.apply(
                    Input {
                        down: true,
                        ..default()
                    },
                    &[1, 2, 3],
                    &[1000; 3],
                );
            }
            s.apply(
                Input {
                    confirm: true,
                    ..default()
                },
                &[1, 2, 3],
                &[1000; 3],
            );
            s.apply(
                Input {
                    down: true,
                    ..default()
                },
                &[1, 2, 3],
                &[1000; 3],
            );
            assert_eq!(
                s.apply(
                    Input {
                        confirm: true,
                        ..default()
                    },
                    &[1, 2, 3],
                    &[1000; 3]
                ),
                None
            );
            assert_eq!(s.cursor, selected);
            assert_eq!(
                s.apply(
                    Input {
                        confirm: true,
                        ..default()
                    },
                    &[1, 2, 3],
                    &[0; 3]
                ),
                Some(selected as u32 + 1)
            );
        }
    }

    #[test]
    fn installed_menu_and_confirmation_project_without_gpu() {
        let root = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !root.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(&root).unwrap();
        let mut images = Assets::<Image>::default();
        let view = View::load(&vfs, &root, &mut images).unwrap();
        assert_eq!(
            view.catalog.choices.iter().map(|c| c.job).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        for choice in &view.catalog.choices {
            let mut selection = Selection::default();
            selection.open(1);
            view.catalog.demo_select(&mut selection, choice.job).unwrap();
            assert_eq!(
                view.catalog.input(
                    &mut selection,
                    Input {
                        confirm: true,
                        ..default()
                    }
                ),
                None
            );
            assert!(selection.confirmation);
            assert_eq!(
                view.catalog.input(
                    &mut selection,
                    Input {
                        confirm: true,
                        ..default()
                    }
                ),
                Some(choice.job)
            );
        }
        let gui = mtgui::Gui::parse(&view.gui).unwrap();
        let dialog = mtgui::Gui::parse(&view.dialog).unwrap();
        let font = mtgui::font::Font::parse(&view.font).unwrap();
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut entities = Vec::new();
        let mut painter = Painter {
            commands: &mut commands,
            textures: &view.textures,
            entities: &mut entities,
            scale: 1.0,
            offset: Vec2::ZERO,
            font: &font,
        };
        for choice in &view.catalog.choices {
            assert!(!choice.name.is_empty() && !choice.description.is_empty());
            painter
                .node(&gui, choice.node, Vec2::ZERO, &[(2, &choice.name)], 220, None)
                .unwrap();
            painter
                .node(&gui, 1, Vec2::ZERO, &[(6, &choice.description)], 200, None)
                .unwrap();
        }
        painter
            .node(
                &dialog,
                view.catalog.dialog_nodes[0],
                Vec2::ZERO,
                &[(2, &view.catalog.prompt)],
                300,
                None,
            )
            .unwrap();
        for (index, answer) in view.catalog.answers.iter().enumerate() {
            for selector in [0, 1] {
                painter
                    .node(
                        &dialog,
                        view.catalog.dialog_nodes[index + 1],
                        Vec2::ZERO,
                        &[(3, answer)],
                        330,
                        Some(selector),
                    )
                    .unwrap();
            }
        }
        drop(painter);
        drop(commands);
        queue.apply(&mut world);
        assert!(
            entities.len() > 200,
            "installed descriptions generated bitmap glyphs"
        );
        for entity in entities {
            let image = world.get::<ImageNode>(entity).unwrap();
            assert!(images.get(&image.image).is_some());
            let rect = image.rect.unwrap();
            assert!(rect.width() > 0.0 && rect.height() > 0.0);
        }
    }
}
