//! Stage-801 first milestone: gender, keyboard name, installed full preset,
//! confirmation, and one preview rebuilt by ddlook after appearance changes.
//! Detailed face/body/color/voice pages and native GUI sequence animation remain open.
use crate::character_creation::{Editor, Page};
use crate::stage::Loader;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    automation: Option<Automation>,
}

// Project capture policy, not native editor timing. Successful GUI draws must
// span several frames and real time; a requested screenshot must finish saving.
struct Automation {
    page: Page,
    since: f64,
    draws: u32,
    capture_dir: Option<PathBuf>,
    saved: Option<Arc<Mutex<Option<Result<(), String>>>>>,
}

impl Automation {
    fn dwell_complete(&self, now: f64) -> bool {
        self.draws >= 3 && now - self.since >= 0.75
    }
}

/// The capture runner creates this directory explicitly before launching.
/// Resolve links before accepting it: screenshots/checkpoints stay private.
pub(crate) fn capture_dir() -> Result<Option<PathBuf>, String> {
    let Some(path) = std::env::var_os("DDVIEW_CHARACTER_CAPTURE_DIR") else {
        return Ok(None);
    };
    let path = std::fs::canonicalize(path).map_err(|e| format!("capture directory: {e}"))?;
    let private = std::fs::canonicalize("E:/ddda-re").map_err(|e| format!("private research root: {e}"))?;
    if !path.is_dir() || !path.starts_with(private) {
        return Err("character capture directory must already exist inside E:/ddda-re".into());
    }
    Ok(Some(path))
}

fn fixture<T: std::str::FromStr>(field: &str, seat: ddhost::PlayerId) -> Result<Option<T>, String> {
    match std::env::var(format!("DDVIEW_CHARACTER_{field}_{seat}"))
        .or_else(|_| std::env::var(format!("DDVIEW_CHARACTER_{field}")))
    {
        Ok(value) => value
            .parse()
            .map(Some)
            .map_err(|_| format!("invalid character {field} fixture for seat {seat}")),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

impl CharaEdit {
    pub fn record(&self) -> ddpawn::cmc::Edit {
        self.editor.edit.clone()
    }

    /// Explicit unattended input; callers provide a name fixture, never invent one.
    pub fn automate(
        &mut self,
        commands: &mut Commands,
        name: &str,
        now: f64,
        seat: ddhost::PlayerId,
        profile: ddhost::ProfileId,
    ) -> Result<Option<ddpawn::cmc::Edit>, String> {
        let page = self.editor.page;
        if self.automation.as_ref().map(|a| a.page) != Some(page) {
            match page {
                Page::Gender => {
                    if let Some(sex) = fixture::<u8>("SEX", seat)? {
                        self.editor.select_preset(sex, self.editor.preset)?;
                        self.dirty = true;
                    }
                }
                Page::Name => {
                    self.editor.typed.clear();
                    self.editor.type_text(name);
                }
                Page::Preset => {
                    if let Some(preset) = fixture::<usize>("PRESET", seat)? {
                        self.editor
                            .select_preset(self.editor.edit.get("mGender").unwrap(), preset)?;
                        self.dirty = true;
                    }
                }
                Page::Confirm => {}
            }
            self.automation = Some(Automation {
                page,
                since: now,
                draws: 0,
                capture_dir: capture_dir()?,
                saved: None,
            });
            info!(
                "intro: stage801 seat {seat} profile {profile} page {} fixture prepared",
                page.label()
            );
            return Ok(None);
        }
        let automation = self.automation.as_mut().unwrap();
        if !automation.dwell_complete(now) {
            return Ok(None);
        }
        if let Some(dir) = &automation.capture_dir {
            if let Some(saved) = &automation.saved {
                match &*saved.lock().unwrap() {
                    None => return Ok(None),
                    Some(Err(error)) => return Err(error.clone()),
                    Some(Ok(())) => {}
                }
            } else {
                let path = dir.join(format!("S56-seat{seat}-profile{profile}-{}.png", page.label()));
                let saved = Arc::new(Mutex::new(None));
                let acknowledged = saved.clone();
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(move |event: On<ScreenshotCaptured>| {
                        let result = event
                            .image
                            .clone()
                            .try_into_dynamic()
                            .map_err(|e| e.to_string())
                            .and_then(|image| image.to_rgb8().save(&path).map_err(|e| e.to_string()));
                        if result.is_ok() {
                            info!("intro: stage801 rendered capture saved {}", path.display());
                        }
                        *acknowledged.lock().unwrap() = Some(result);
                    });
                automation.saved = Some(saved);
                return Ok(None);
            }
        }
        let confirmed = self.editor.accept();
        if page == Page::Name && self.editor.page == Page::Name {
            return Err(format!(
                "character name fixture for seat {seat} normalizes to empty"
            ));
        }
        Ok(confirmed.then(|| self.record()))
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

#[cfg(test)]
mod automation_tests {
    use super::*;

    #[test]
    fn unattended_character_page_waits_for_multiple_draws_and_real_time() {
        let mut automation = Automation {
            page: Page::Name,
            since: 10.0,
            draws: 0,
            capture_dir: None,
            saved: None,
        };
        // A slow first frame must not admit a page that never got drawn.
        assert!(!automation.dwell_complete(30.0));
        automation.draws = 2;
        assert!(!automation.dwell_complete(30.0));
        automation.draws = 3;
        assert!(!automation.dwell_complete(10.1));
        assert!(automation.dwell_complete(10.75));
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
        automation: None,
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
        match e.editor.draw(
            &mut commands,
            &mut images,
            e.loader.vfs,
            window.width(),
            window.height(),
        ) {
            Err(error) => error!("character editor GUI: {error}"),
            Ok(()) => {
                if let Some(automation) = &mut e.automation {
                    if automation.page == e.editor.page {
                        automation.draws = automation.draws.saturating_add(1);
                    }
                }
            }
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
