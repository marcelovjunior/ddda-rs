//! Stage-801 first milestone: gender, keyboard name, installed full preset,
//! confirmation, and one preview rebuilt by ddlook after appearance changes.
//! Detailed face/body/color/voice pages and native GUI sequence animation remain open.
use crate::character_creation::{Editor, Page};
use crate::stage::Loader;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

const MOTION: &str = r"motion\pl\m\m00\m0000_co\m0000_co";

#[derive(Resource, Default)]
pub struct EditResult(pub Option<ddpawn::cmc::Edit>);

#[derive(Resource)]
pub struct CharaEdit {
    loader: Loader<'static>,
    tables: ddlook::Tables,
    pattern: ddlook::pattern::EditPattern,
    editor: Editor,
    body: Option<Entity>,
    controls: Entity,
    dirty: bool,
    closed: bool,
}

impl CharaEdit {
    pub fn record(&self) -> ddpawn::cmc::Edit {
        self.editor.edit.clone()
    }

    /// Explicit unattended input; callers provide a name fixture, never invent one.
    pub fn automate(&mut self, name: &str) -> Option<ddpawn::cmc::Edit> {
        if self.editor.page == Page::Name {
            self.editor.typed = name.to_owned();
        }
        self.editor.accept().then(|| self.record())
    }

    pub fn close(&mut self, commands: &mut Commands) {
        if let Some(body) = self.body.take() {
            commands.entity(body).despawn();
        }
        commands.entity(self.controls).despawn();
        self.editor.close(commands);
        self.closed = true;
    }
}

pub fn setup(
    commands: &mut Commands,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
    sex: u8,
) -> Result<(), String> {
    setup_inner(commands, vfs, mfx, sex, None)
}

pub fn setup_record(
    commands: &mut Commands,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
    record: Option<ddpawn::cmc::Edit>,
) -> Result<(), String> {
    setup_inner(
        commands,
        vfs,
        mfx,
        record.as_ref().and_then(|edit| edit.get("mGender")).unwrap_or(0),
        record,
    )
}

fn setup_inner(
    commands: &mut Commands,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
    sex: u8,
    record: Option<ddpawn::cmc::Edit>,
) -> Result<(), String> {
    let mut loader = Loader {
        vfs,
        mfx,
        textures: Default::default(),
        models: Default::default(),
        model_names: Default::default(),
        water: None,
        stats: Default::default(),
        lod_eye: None,
        view_range: 1,
        tag: None,
    };
    loader.index_models();
    let tables = ddlook::Tables::load(vfs).map_err(|e| e.to_string())?;
    let pattern = ddlook::pattern::EditPattern::load(vfs).map_err(|e| e.to_string())?;
    let mut editor = Editor::load(vfs, sex)?;
    if let Some(record) = record {
        editor.restore(record)?;
    }
    // Presentation bindings, not native keyboard scan/repeat or legend geometry.
    let controls = commands
        .spawn((
            Text::new("Left/Right: choose   Enter: continue   Backspace: back / erase"),
            TextFont {
                font_size: bevy::text::FontSize::Px(15.0),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                ..default()
            },
        ))
        .id();
    commands.insert_resource(EditResult(None));
    commands.insert_resource(CharaEdit {
        loader,
        tables,
        pattern,
        editor,
        body: None,
        controls,
        dirty: true,
        closed: false,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    edit: Option<ResMut<CharaEdit>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut result: Option<ResMut<EditResult>>,
    windows: Query<&Window>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let events: Vec<_> = keyboard.read().cloned().collect();
    let Some(mut edit) = edit else { return };
    let e = &mut *edit;
    if e.closed {
        return;
    }
    if e.editor.page == Page::Name {
        if keys.just_pressed(KeyCode::Backspace) {
            e.editor.typed.pop();
        }
        for event in events {
            if event.state == ButtonState::Pressed {
                if let Some(text) = event.text {
                    e.editor.type_text(&text);
                }
            }
        }
        if keys.just_pressed(KeyCode::Escape) {
            e.editor.back();
        }
    } else if keys.just_pressed(KeyCode::Backspace) || keys.just_pressed(KeyCode::Escape) {
        e.editor.back();
    }
    for (key, forward) in [(KeyCode::ArrowRight, true), (KeyCode::ArrowLeft, false)] {
        if keys.just_pressed(key) {
            match e.editor.change(forward) {
                Ok(changed) => e.dirty |= changed,
                Err(error) => error!("character edit: {error}"),
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter) && e.editor.accept() {
        if let Some(result) = result.as_mut() {
            result.0 = Some(e.record());
        }
    }
    if let Ok(window) = windows.single() {
        if let Err(error) = e.editor.draw(
            &mut commands,
            &mut images,
            e.loader.vfs,
            window.width(),
            window.height(),
        ) {
            error!("character editor GUI: {error}");
        }
    }
    if !e.dirty {
        return;
    }
    e.dirty = false;
    let fields = e.editor.fields();
    let Some(look) = ddlook::save::edit_look(
        e.loader.vfs,
        &e.tables,
        &e.pattern,
        &[],
        ddlook::Unit::Player,
        &fields,
        &[],
    ) else {
        error!("character edit: installed preset cannot produce a look");
        return;
    };
    if let Some(old) = e.body.take() {
        commands.entity(old).despawn();
    }
    match crate::character::spawn_look(
        &mut commands,
        &mut e.loader,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
        &look,
        MOTION,
        0,
        Vec3::ZERO,
        Some(0.0),
    ) {
        Ok(body) => e.body = Some(body),
        Err(error) => error!("character edit body: {error}"),
    }
}
