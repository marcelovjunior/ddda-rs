//! S56's bounded editor: installed presets, messages and basic-page geometry.
//! It omits the full GUI animation VM and detailed editor pages.
use bevy::math::Affine2;
use bevy::prelude::*;
use ddpawn::cmc::Edit;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Gender,
    Name,
    Preset,
    Confirm,
}

pub(crate) struct Editor {
    pub edit: Edit,
    pub page: Page,
    pub preset: usize,
    pub typed: String,
    presets: ddlook::creation::Presets,
    names: ddlook::creation::NameRules,
    messages: Vec<String>,
    gui: Vec<u8>,
    font: Vec<u8>,
    textures: HashMap<String, Handle<Image>>,
    entities: Vec<Entity>,
    shown: Option<(Page, u8, usize, String, [u32; 2])>,
}

impl Editor {
    pub fn load(vfs: &ddvfs::Vfs, sex: u8) -> Result<Self, String> {
        let exe = ddexe::ExeFile::next_to(&vfs.root).map_err(String::from)?;
        let presets = ddlook::creation::Presets::load(vfs).map_err(|e| e.to_string())?;
        let names = ddlook::creation::NameRules::load(vfs, &exe.image().map_err(String::from)?)
            .map_err(|e| e.to_string())?;
        let mut edit = Edit {
            name: [0; 25],
            nickname: 0,
            fields: [0; 41],
        };
        // 0x0060C6CA / 0x00617420: initialize full preset zero for each sex.
        presets.apply(&mut edit, sex, 0).map_err(|e| e.to_string())?;
        let messages = vfs.read(
            "id/message/charactor_edit/charactor_edit_eng",
            mtarc::type_hash("rGUIMessage"),
        )?;
        let messages = mtgmd::MessageFile::parse(&messages)
            .map_err(|e| e.to_string())?
            .messages
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        for index in [9, 32, 36, 115, 139, 140] {
            if messages.get(index).is_none() {
                return Err(format!("editor message {index} missing"));
            }
        }
        // uGUICharacterEdit0::vf58 0x00617F60.
        let gui = vfs.read("id/chr_edit/chr_edit_0", mtarc::type_hash("rGUI"))?;
        let parsed = mtgui::Gui::parse(&gui)?;
        let font = parsed
            .resources
            .iter()
            .find(|r| r.class == "rGUIFont")
            .and_then(|r| r.path)
            .ok_or("character editor font missing")?;
        let font = vfs.read(font, mtarc::type_hash("rGUIFont"))?;
        Ok(Self {
            edit,
            page: Page::Gender,
            preset: 0,
            typed: String::new(),
            presets,
            names,
            messages,
            gui,
            font,
            textures: HashMap::new(),
            entities: Vec::new(),
            shown: None,
        })
    }

    pub fn fields(&self) -> Vec<(String, u32)> {
        ddpawn::cmc::EDIT_U8_FIELDS
            .iter()
            .zip(self.edit.fields)
            .map(|(name, value)| ((*name).to_owned(), u32::from(value)))
            .collect()
    }

    /// Viewer input bindings; native page order is reduced to this milestone.
    /// Name editing uses committed keyboard text, with native normalization on accept.
    pub fn change(&mut self, forward: bool) -> Result<bool, String> {
        match self.page {
            Page::Gender => {
                let sex = self.edit.get("mGender").unwrap() ^ 1;
                self.presets
                    .apply(&mut self.edit, sex, self.preset)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            Page::Preset => {
                let sex = self.edit.get("mGender").unwrap();
                let count = self.presets.all[usize::from(sex)].len();
                self.preset = if forward {
                    (self.preset + 1) % count
                } else {
                    (self.preset + count - 1) % count
                };
                self.presets
                    .apply(&mut self.edit, sex, self.preset)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn accept(&mut self) -> bool {
        self.page = match self.page {
            Page::Gender => Page::Name,
            Page::Name => {
                self.edit.name = self.names.normalize(&self.typed);
                self.typed = self.edit.name_str();
                if self.typed.is_empty() {
                    return false;
                }
                Page::Preset
            }
            Page::Preset => Page::Confirm,
            Page::Confirm => return true,
        };
        false
    }

    pub fn back(&mut self) {
        self.page = match self.page {
            Page::Gender => Page::Gender,
            Page::Name => Page::Gender,
            Page::Preset => Page::Name,
            Page::Confirm => Page::Preset,
        };
    }

    pub fn close(&mut self, commands: &mut Commands) {
        for entity in self.entities.drain(..) {
            commands.entity(entity).despawn();
        }
    }

    pub fn draw(
        &mut self,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        vfs: &ddvfs::Vfs,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let sex = self.edit.get("mGender").unwrap();
        let snapshot = (
            self.page,
            sex,
            self.preset,
            self.typed.clone(),
            [width.to_bits(), height.to_bits()],
        );
        if self.shown.as_ref() == Some(&snapshot) {
            return Ok(());
        }
        self.close(commands);
        self.shown = None;
        let gui = mtgui::Gui::parse(&self.gui)?;
        let font = mtgui::font::Font::parse(&self.font)?;
        let instruction = match self.page {
            Page::Gender => 9,
            Page::Name => 115,
            Page::Preset => 32,
            Page::Confirm => 36,
        };
        let value = match self.page {
            Page::Gender => self.messages[139 + usize::from(sex)].clone(),
            Page::Name => self.typed.clone(),
            Page::Preset => format!("{}/{}", self.preset + 1, self.presets.all[usize::from(sex)].len()),
            Page::Confirm => self.edit.name_str(),
        };
        let scale = (width / gui.width as f32).min(height / gui.height as f32);
        let offset = Vec2::new(
            (width - gui.width as f32 * scale) * 0.5,
            (height - gui.height as f32 * scale) * 0.5,
        );
        // Basic prompt: native 0x006182D6 binds object ordinal2, whose installed
        // object ID is9, on instance ordinal3. Name window is authored instance34.
        // This first milestone reuses that basic input panel for preset/confirmation;
        // those two full-page native layouts and cursor animations remain later work.
        for (node, text) in [(3, self.messages[instruction].as_str()), (34, value.as_str())] {
            let transform = node_transform(&gui, node)?;
            let animation = gui.local_animation(node).ok_or("editor panel animation absent")?;
            for object in gui.animation_objects(animation.name)? {
                if !object.visible_at(0) {
                    continue;
                }
                let vector = |name| {
                    object
                        .vector_keys
                        .iter()
                        .filter(|(key, time, _)| *key == name && *time == 0)
                        .last()
                        .map(|(_, _, v)| *v)
                        .or_else(|| object.property(name).and_then(|p| gui.vector(p)))
                };
                let Some(size) = vector("Size") else { continue };
                let size = Vec2::new(size[0], size[1]);
                let anchor = Vec2::from_array(mtgui::layout::anchor_offset(
                    size.to_array(),
                    object.integer("ControlPoint").unwrap_or(0),
                ));
                let position = transform.transform_point2(
                    Vec2::new(
                        object.scalar("PosX").unwrap_or(0.0),
                        object.scalar("PosY").unwrap_or(0.0),
                    ) + anchor,
                );
                if object.id == 9 {
                    let fs = vector("FontSize").ok_or("editor font size missing")?;
                    let color = vector("Color").unwrap_or([255.0; 4]);
                    for placed in font.line(text, [fs[0], fs[1]])? {
                        let glyph = placed.glyph;
                        let path = font.texture_path(glyph.page)?;
                        let image = texture(&mut self.textures, vfs, images, &path)?;
                        let pos = offset
                            + (position + transform.matrix2 * Vec2::from_array(placed.position)) * scale;
                        let size = (transform.matrix2 * Vec2::from_array(placed.size)).abs() * scale;
                        self.entities.push(quad(
                            commands,
                            pos,
                            size,
                            image,
                            Rect::new(
                                glyph.x as f32,
                                glyph.y as f32,
                                (glyph.x + glyph.width) as f32,
                                (glyph.y + glyph.height) as f32,
                            ),
                            color,
                            302,
                        ));
                    }
                } else if let Some(id) = object
                    .texture_keys
                    .iter()
                    .filter(|(time, _)| *time == 0)
                    .last()
                    .map(|(_, id)| *id)
                    .or_else(|| object.integer("Texture"))
                {
                    let regions = gui.texture_regions()?;
                    let Some(region) = regions.iter().find(|region| region.id == id) else {
                        continue;
                    };
                    let rect = vector("mTextureRect").unwrap_or(region.rect);
                    if rect[2] <= rect[0] || rect[3] <= rect[1] {
                        continue;
                    }
                    let image = texture(&mut self.textures, vfs, images, region.path)?;
                    let color = vector("VertexColor").unwrap_or([255.0; 4]);
                    self.entities.push(quad(
                        commands,
                        offset + position * scale,
                        (transform.matrix2 * size).abs() * scale,
                        image,
                        Rect::new(rect[0], rect[1], rect[2], rect[3]),
                        color,
                        301,
                    ));
                }
            }
        }
        self.shown = Some(snapshot);
        Ok(())
    }
}

fn texture(
    cache: &mut HashMap<String, Handle<Image>>,
    vfs: &ddvfs::Vfs,
    images: &mut Assets<Image>,
    path: &str,
) -> Result<Handle<Image>, String> {
    if let Some(image) = cache.get(path) {
        return Ok(image.clone());
    }
    let image = images.add(crate::stage::tex_to_image(
        &vfs.read(path, mtarc::type_hash("rTexture"))?,
    )?);
    cache.insert(path.to_owned(), image.clone());
    Ok(image)
}

fn quad(
    commands: &mut Commands,
    pos: Vec2,
    size: Vec2,
    image: Handle<Image>,
    rect: Rect,
    color: [f32; 4],
    layer: i32,
) -> Entity {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pos.x),
                top: Val::Px(pos.y),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            ImageNode {
                image,
                rect: Some(rect),
                color: Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                ),
                ..default()
            },
            GlobalZIndex(layer),
        ))
        .id()
}

fn node_transform(gui: &mtgui::Gui<'_>, wanted: usize) -> Result<Affine2, String> {
    let mut stack = Vec::new();
    for (index, depth) in gui.preorder()? {
        stack.truncate(depth);
        let scalar = |name, fallback| {
            gui.node_property(index, name)
                .and_then(|p| p.word)
                .map(f32::from_bits)
                .unwrap_or(fallback)
        };
        let local = Affine2::from_scale_angle_translation(
            Vec2::new(scalar("SclX", 1.0), scalar("SclY", 1.0)),
            scalar("Rotation", 0.0).to_radians(),
            Vec2::new(scalar("PosX", 0.0), scalar("PosY", 0.0)),
        );
        let world = stack.last().copied().unwrap_or(Affine2::IDENTITY) * local;
        if index == wanted {
            return Ok(world);
        }
        stack.push(world);
    }
    Err("editor panel node absent".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_creation_accepts_a_named_female_preset_without_losing_edit_fields() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            eprintln!("SKIP: install absent");
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let mut editor = Editor::load(&vfs, 0).unwrap();
        assert!(editor.change(true).unwrap());
        assert_eq!(editor.edit.get("mGender"), Some(1));
        assert!(!editor.accept());
        assert_eq!(editor.page, Page::Name);
        assert!(!editor.accept());
        assert_eq!(editor.page, Page::Name);
        editor.typed = "Alba".into();
        assert!(!editor.accept());
        assert_eq!(editor.page, Page::Preset);
        assert!(editor.change(true).unwrap());
        let appearance = editor.edit.fields;
        assert!(!editor.accept());
        assert_eq!(editor.page, Page::Confirm);
        editor.back();
        assert_eq!(editor.page, Page::Preset);
        assert!(!editor.accept());
        assert!(editor.accept());
        assert_eq!(editor.edit.fields, appearance);
        assert_eq!(editor.edit.name_str(), "Alba");
        let look = ddlook::save::edit_look_dressed(&vfs, &editor.fields()).unwrap();
        assert_eq!(look.sex, 1);
        let gui = mtgui::Gui::parse(&editor.gui).unwrap();
        let font = mtgui::font::Font::parse(&editor.font).unwrap();
        for index in [3, 34] {
            let objects = gui
                .animation_objects(gui.local_animation(index).unwrap().name)
                .unwrap();
            assert!(
                objects
                    .iter()
                    .any(|object| object.id == 9 && object.property("FontSize").is_some())
            );
            assert!(node_transform(&gui, index).unwrap().translation.is_finite());
        }
        assert!(!font.line("Alba", [18.0; 2]).unwrap().is_empty());
    }
}
