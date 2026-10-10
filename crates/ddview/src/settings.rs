//! Per-viewer graphics settings. The original config is only an import source.
//! Settings never enter the host simulation or the multiplayer protocol.
use bevy::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU16, Ordering},
    OnceLock,
};

// A display-wide sampler policy: this process's windows/local seats use the
// same graphics options. No player or host simulation state is global here.
static SAMPLER_ANISOTROPY: AtomicU16 = AtomicU16::new(0);
static INITIAL: OnceLock<Result<Store, String>> = OnceLock::new();

pub fn initial_graphics() -> Graphics {
    INITIAL
        .get_or_init(Store::load)
        .as_ref()
        .map(|s| s.graphics.clone())
        .unwrap_or_default()
}

pub fn filter_image(image: &mut Image, filtering: Filtering) {
    use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    let mut sampler = match &image.sampler {
        ImageSampler::Descriptor(s) => s.clone(),
        ImageSampler::Default => ImageSamplerDescriptor::linear(),
    };
    sampler.mag_filter = ImageFilterMode::Linear;
    sampler.min_filter = ImageFilterMode::Linear;
    sampler.mipmap_filter = ImageFilterMode::Linear;
    sampler.anisotropy_clamp = filtering.anisotropy();
    image.sampler = ImageSampler::Descriptor(sampler);
}

/// Called only by the world Loader, not HUD/font/movie texture conversion.
pub fn filter_loaded_image(image: &mut Image) {
    let level = SAMPLER_ANISOTROPY.load(Ordering::Relaxed);
    if let Some(filtering) = Filtering::ALL.into_iter().find(|f| f.anisotropy() == level) {
        filter_image(image, filtering);
    }
}

#[derive(Resource)]
pub struct RenderSource {
    pub vfs: &'static ddvfs::Vfs,
    pub textures: std::collections::HashMap<String, Option<Handle<Image>>>,
}

#[derive(Resource, Default)]
pub struct Runtime {
    pub open: bool,
    pub(crate) available: bool,
    store: Option<Store>,
    graphics: Graphics,
    revision: u64,
    error: Option<String>,
}

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let mut state = Runtime::default();
        match INITIAL.get_or_init(Store::load) {
            Ok(store) => {
                state.graphics = store.graphics.clone();
                state.store = Some(store.clone());
            }
            Err(error) => {
                error!("viewer settings: {error}");
                state.error = Some(error.clone());
            }
        }
        SAMPLER_ANISOTROPY.store(
            state.graphics.filtering.map_or(0, Filtering::anisotropy),
            Ordering::Relaxed,
        );
        app.insert_resource(state)
            .add_systems(Startup, menu_setup.after(crate::pause::setup))
            .add_systems(
                Update,
                menu_update.after(crate::pause::update).before(crate::play::step),
            )
            .add_systems(PostUpdate, apply);
    }
}

#[derive(Component)]
struct SettingsMenu;
#[derive(Component)]
struct SettingText(SettingKey);
#[derive(Component, Clone, Copy)]
enum MenuAction {
    Change(SettingKey),
    Back,
}
#[derive(Component)]
struct SettingsError;

#[derive(Clone, Copy)]
enum SettingKey {
    Unsupported(&'static str, &'static str),
    Vsync,
    Dof,
    Shadow,
    Filtering,
    Range,
    Fov,
}

impl SettingKey {
    fn value(self, state: &Runtime) -> String {
        let g = &state.graphics;
        let boolean = |value| match value {
            Some(true) => "On",
            Some(false) => "Off",
            None => "Unspecified",
        };
        match self {
            Self::Vsync => boolean(g.vsync).into(),
            Self::Dof => boolean(g.dof).into(),
            Self::Shadow => g.shadow.map(Quality::token).unwrap_or("Unspecified").into(),
            Self::Filtering => g.filtering.map(Filtering::token).unwrap_or("Unspecified").into(),
            Self::Range => format!(
                "{} (restart to apply)",
                g.view_range.map(ViewRange::token).unwrap_or("Unspecified")
            ),
            Self::Fov => g
                .camera_fov
                .map(|v| format!("{v}°"))
                .unwrap_or_else(|| "Unspecified".into()),
            Self::Unsupported(section, key) => format!(
                "{} — Unavailable",
                state
                    .store
                    .as_ref()
                    .and_then(|s| s.document.get(section, key))
                    .unwrap_or("Unspecified")
            ),
        }
    }

    fn change(self, graphics: &mut Graphics) -> bool {
        fn next<T: PartialEq + Copy>(all: &[T], current: Option<T>) -> T {
            let index = current.and_then(|value| all.iter().position(|v| *v == value));
            all[index.map_or(0, |i| (i + 1) % all.len())]
        }
        match self {
            Self::Vsync => graphics.vsync = Some(!graphics.vsync.unwrap_or(false)),
            Self::Dof => graphics.dof = Some(!graphics.dof.unwrap_or(false)),
            Self::Shadow => graphics.shadow = Some(next(&Quality::ALL, graphics.shadow)),
            Self::Filtering => graphics.filtering = Some(next(&Filtering::ALL, graphics.filtering)),
            Self::Range => graphics.view_range = Some(next(&ViewRange::ALL, graphics.view_range)),
            // Temporary viewer button granularity; the native 0..15 range
            // is sourced, its original slider/controller widget is unported.
            Self::Fov => {
                graphics.camera_fov =
                    Some(
                        graphics
                            .camera_fov
                            .map_or(0.0, |v| if v >= 15.0 { 0.0 } else { (v + 1.0).min(15.0) }),
                    )
            }
            Self::Unsupported(..) => return false,
        }
        true
    }
}

fn menu_setup(
    mut commands: Commands,
    args: Res<crate::Args>,
    source: Option<Res<RenderSource>>,
    mut state: ResMut<Runtime>,
) {
    if !args.play {
        return;
    }
    let Some(source) = source else { return };
    let options = match OriginalOptions::load(source.vfs) {
        Ok(options) => options,
        Err(error) => {
            error!("graphics menu resources: {error}");
            state.error = Some(error);
            return;
        }
    };
    state.available = true;
    if args.paused && args.unattended && std::env::var("DDVIEW_SETTINGS_CAPTURE").as_deref() == Ok("1") {
        state.open = true;
        info!("graphics settings: capture menu opened from installed option labels");
    }
    // The controls below are a plain temporary viewer menu using runtime
    // native labels. The original layout/atlas widget renderer is unported.
    commands
        .spawn((
            SettingsMenu,
            GlobalZIndex(101),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.95)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Graphics settings — viewer menu"),
                TextFont {
                    font_size: bevy::text::FontSize::Px(26.0),
                    ..default()
                },
            ));
            for (id, key) in [
                (139, SettingKey::Unsupported(DISPLAY, "Resolution")),
                (140, SettingKey::Unsupported(DISPLAY, "FullScreen")),
                (141, SettingKey::Unsupported(DISPLAY, "RefreshRate")),
                (142, SettingKey::Vsync),
                (143, SettingKey::Unsupported(GRAPHICS, "MaxFPS")),
                (144, SettingKey::Unsupported(GRAPHICS, "AltAntiAlias")),
                (145, SettingKey::Unsupported(GRAPHICS, "HDR")),
                (146, SettingKey::Shadow),
                (147, SettingKey::Unsupported(GRAPHICS, "TextureDetail")),
                (148, SettingKey::Filtering),
                (149, SettingKey::Unsupported(GRAPHICS, "GrassQuality")),
                (150, SettingKey::Unsupported(GRAPHICS, "EffectVolume")),
                (151, SettingKey::Fov),
                (233, SettingKey::Range),
                (237, SettingKey::Dof),
                (37, SettingKey::Unsupported(GRAPHICS, "Brightness")),
            ] {
                let label = options.text(id).unwrap_or("");
                panel
                    .spawn(
                        (Node {
                            width: Val::Px(780.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        }),
                    )
                    .with_children(|row| {
                        row.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(17.0),
                                ..default()
                            },
                        ));
                        let enabled = !matches!(key, SettingKey::Unsupported(..));
                        row.spawn((
                            Button,
                            MenuAction::Change(key),
                            Node {
                                width: Val::Px(440.0),
                                padding: UiRect::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(if enabled {
                                Color::srgb(0.15, 0.2, 0.28)
                            } else {
                                Color::srgb(0.08, 0.08, 0.09)
                            }),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                SettingText(key),
                                Text::new(""),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(if enabled {
                                    Color::WHITE
                                } else {
                                    Color::srgb(0.5, 0.5, 0.5)
                                }),
                            ));
                        });
                    });
            }
            panel.spawn((
                SettingsError,
                Text::new(""),
                TextFont {
                    font_size: bevy::text::FontSize::Px(15.0),
                    ..default()
                },
            ));
            panel
                .spawn((
                    Button,
                    MenuAction::Back,
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.2, 0.28)),
                ))
                .with_children(|button| {
                    button.spawn(Text::new("Back (Esc)"));
                });
        });
}

fn menu_update(
    mut state: ResMut<Runtime>,
    pause: Res<crate::pause::PauseState>,
    buttons: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut menus: Query<&mut Node, With<SettingsMenu>>,
    mut values: Query<(&SettingText, &mut Text), Without<SettingsError>>,
    mut errors: Query<&mut Text, With<SettingsError>>,
) {
    if state.open && pause.paused {
        for (interaction, action) in &buttons {
            if *interaction != Interaction::Pressed {
                continue;
            }
            match action {
                MenuAction::Back => state.open = false,
                MenuAction::Change(key) => {
                    let mut candidate = state.graphics.clone();
                    if !key.change(&mut candidate) {
                        continue;
                    }
                    let Some(store) = &mut state.store else {
                        continue;
                    };
                    match store.save(&candidate) {
                        Ok(()) => {
                            state.graphics = candidate;
                            state.revision += 1;
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
            }
        }
    }
    for mut menu in &mut menus {
        let display = if state.open && pause.paused {
            Display::Flex
        } else {
            Display::None
        };
        if menu.display != display {
            menu.display = display;
        }
    }
    if !state.open {
        return;
    }
    for (key, mut text) in &mut values {
        let value = key.0.value(&state);
        if **text != value {
            **text = value;
        }
    }
    for mut text in &mut errors {
        let error = state.error.clone().unwrap_or_default();
        if **text != error {
            **text = error;
        }
    }
}

fn apply(
    mut commands: Commands,
    state: Res<Runtime>,
    args: Res<crate::Args>,
    source: Option<Res<RenderSource>>,
    streamer: Option<Res<crate::stream::Streamer>>,
    mut windows: Query<&mut Window>,
    mut shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
    mut cameras: Query<(Entity, &Projection, Option<&mut crate::stage::post::DdPost>), With<Camera3d>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut dd_materials: ResMut<Assets<crate::stage::DdMaterial>>,
    mut previous: Local<Option<(u64, u32)>>,
    mut previous_filtering: Local<Option<Filtering>>,
    mut camera_config: ResMut<crate::play::CameraConfig>,
    mut previous_fov: Local<Option<f32>>,
) {
    let stage = streamer.as_ref().map_or(args.stage, |s| s.stage);
    let changed = previous
        .as_ref()
        .is_none_or(|(revision, _)| *revision != state.revision);
    let stage_changed = previous.as_ref().is_none_or(|(_, old)| *old != stage);
    if !changed && !stage_changed {
        return;
    }
    let initial = previous.is_none();
    *previous = Some((state.revision, stage));
    let g = &state.graphics;
    if changed {
        // Args already applies explicit --camera-fov at startup. Preserve
        // that override until this control itself is deliberately changed.
        if !initial && *previous_fov != g.camera_fov {
            if let Some(offset) = g.camera_fov {
                camera_config.fov_offset_degrees = offset;
            }
        }
        *previous_fov = g.camera_fov;
        if let Some(vsync) = g.vsync {
            // Explicit CLI performance/uncapped mode keeps its existing priority.
            let vsync = vsync && !(args.no_vsync || args.perf || args.perf_route);
            for mut window in &mut windows {
                window.present_mode = if vsync {
                    bevy::window::PresentMode::AutoVsync
                } else {
                    bevy::window::PresentMode::AutoNoVsync
                };
            }
        }
        if let Some(quality) = g.shadow {
            shadow_map.size = quality.shadow_resolution();
        }
        SAMPLER_ANISOTROPY.store(g.filtering.map_or(0, Filtering::anisotropy), Ordering::Relaxed);
        // World images use RenderAssetUsages::RENDER_WORLD: their CPU payload
        // may have been discarded after upload. Re-read only on a live edit,
        // restore the same asset IDs, and regenerate material bind groups.
        // GUI/fonts/movies never belong to these Loader texture maps.
        if !initial && *previous_filtering != g.filtering {
            if let (Some(filtering), Some(source)) = (g.filtering, source.as_ref()) {
                let mut sources = source.textures.clone();
                if let Some(streamer) = &streamer {
                    sources.extend(streamer.loader.textures.clone());
                }
                for (path, handle) in sources {
                    let Some(handle) = handle else {
                        continue;
                    };
                    match source
                        .vfs
                        .read(&path, mtarc::type_hash("rTexture"))
                        .and_then(|bytes| crate::stage::tex_to_image(&bytes))
                    {
                        Ok(mut image) => {
                            filter_image(&mut image, filtering);
                            if let Err(error) = images.insert(handle.id(), image) {
                                warn!("settings texture {path}: {error}");
                            }
                        }
                        Err(error) => warn!("settings texture {path}: {error}"),
                    }
                }
                for (_, mut material) in materials.iter_mut() {
                    let _ = &mut *material;
                }
                for (_, mut material) in dd_materials.iter_mut() {
                    let _ = &mut *material;
                }
            }
        }
        *previous_filtering = g.filtering;
    }
    if let (Some(enabled), Some(source)) = (g.dof, source) {
        let filter = crate::stage::post::DdPost::dof_filter(source.vfs, stage);
        for (entity, projection, post) in &mut cameras {
            let Projection::Perspective(lens) = projection else {
                continue;
            };
            let size = windows
                .iter()
                .next()
                .map(|w| (w.physical_width(), w.physical_height()))
                .unwrap_or(args.size);
            if let Some(mut post) = post {
                if let Some(filter) = filter {
                    *post = (*post).with_dof(
                        &filter,
                        lens.near / crate::stage::UNIT,
                        lens.far / crate::stage::UNIT,
                        size,
                    );
                }
                post.flags.z = u8::from(enabled && filter.is_some()) as f32;
            } else if let Some(mut post) = crate::stage::post::DdPost::load(source.vfs, stage) {
                if enabled {
                    if let Some(filter) = filter {
                        post = post.with_dof(
                            &filter,
                            lens.near / crate::stage::UNIT,
                            lens.far / crate::stage::UNIT,
                            size,
                        );
                    }
                }
                commands.entity(entity).insert((post, bevy::camera::Hdr));
            }
        }
    }
}

/// Project format; unrelated to the original game's save/config formats.
const CONFIG_VERSION: &str = "1";
const GRAPHICS: &str = "GRAPHICS";
const DISPLAY: &str = "DISPLAY";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ini(BTreeMap<(String, String), String>);

impl Ini {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut section = String::new();
        let mut result = Self::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = name.trim().to_string();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("INI line {} has no '='", index + 1))?;
            if section.is_empty() || key.trim().is_empty() {
                return Err(format!("INI line {} has no section/key", index + 1));
            }
            let key = (section.clone(), key.trim().to_string());
            if result.0.insert(key, value.trim().to_string()).is_some() {
                return Err(format!("duplicate INI key at line {}", index + 1));
            }
        }
        Ok(result)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.0.get(&(section.into(), key.into())).map(String::as_str)
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        self.0.insert((section.into(), key.into()), value.to_string());
    }

    pub fn encode(&self) -> String {
        let mut text = String::new();
        let mut previous = "";
        for ((section, key), value) in &self.0 {
            if section != previous {
                text.push_str(&format!("[{section}]\n"));
                previous = section;
            }
            text.push_str(&format!("{key}={value}\n"));
        }
        text
    }

    /// Graphics/display only: never import controls, sound, CPU or window state.
    pub fn graphics_only(mut self) -> Self {
        self.0
            .retain(|(section, _), _| section == GRAPHICS || section == DISPLAY);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];
    // Config writer 0x00EDDB20: shadow +0x10 and texture +0x0C.
    pub fn token(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.token() == value)
    }
    /// Stable shadow allocation 0x00EDFB50 -> 0x00DA97D0(arg 0),
    /// embedded resolution table 0x014292BC (DDDA.exe 2017-12-13 Steam).
    pub fn shadow_resolution(self) -> usize {
        match self {
            Self::Low => 512,
            Self::Medium => 1024,
            Self::High => 2048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filtering {
    Trilinear,
    Aniso2,
    Aniso4,
    Aniso8,
    Aniso16,
}

impl Filtering {
    pub const ALL: [Self; 5] = [
        Self::Trilinear,
        Self::Aniso2,
        Self::Aniso4,
        Self::Aniso8,
        Self::Aniso16,
    ];
    // Config writer 0x00EDDD83..0x00EDDE08, native capability selection
    // 0x00EDE3D6..0x00EDE4CD (DDDA.exe 2017-12-13 Steam).
    pub fn token(self) -> &'static str {
        match self {
            Self::Trilinear => "TRILINEAR",
            Self::Aniso2 => "ANISO_X2",
            Self::Aniso4 => "ANISO_X4",
            Self::Aniso8 => "ANISO_X8",
            Self::Aniso16 => "ANISO_X16",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.token() == value)
    }
    pub fn anisotropy(self) -> u16 {
        match self {
            Self::Trilinear => 1,
            Self::Aniso2 => 2,
            Self::Aniso4 => 4,
            Self::Aniso8 => 8,
            Self::Aniso16 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewRange {
    Normal,
    Far,
    Farthest,
}

impl ViewRange {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Far, Self::Farthest];
    // Config writer 0x00EDDE65..0x00EDDE99; renderer 0x00EA5320.
    pub fn token(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Far => "FAR",
            Self::Farthest => "FARTHEST",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.token() == value)
    }
    pub fn native(self) -> u32 {
        match self {
            Self::Normal => 1,
            Self::Far => 2,
            Self::Farthest => 3,
        }
    }
}

/// Absent options remain absent; the caller retains its existing renderer
/// behavior rather than making up an original-game quality default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graphics {
    pub vsync: Option<bool>,
    pub dof: Option<bool>,
    pub camera_fov: Option<f32>,
    pub shadow: Option<Quality>,
    pub filtering: Option<Filtering>,
    pub view_range: Option<ViewRange>,
}

impl Graphics {
    pub fn read(ini: &Ini) -> Result<Self, String> {
        fn boolean(ini: &Ini, section: &str, key: &str) -> Result<Option<bool>, String> {
            match ini.get(section, key) {
                None => Ok(None),
                Some("ON") => Ok(Some(true)),
                Some("OFF") => Ok(Some(false)),
                Some(v) => Err(format!("{section}.{key}: expected ON/OFF, got {v}")),
            }
        }
        fn enumeration<T>(
            ini: &Ini,
            key: &str,
            parse: impl FnOnce(&str) -> Option<T>,
        ) -> Result<Option<T>, String> {
            ini.get(GRAPHICS, key)
                .map(|v| parse(v).ok_or_else(|| format!("GRAPHICS.{key}: unsupported value {v}")))
                .transpose()
        }
        let camera_fov = ini
            .get(GRAPHICS, "CameraFov")
            .map(|v| {
                let value = v
                    .parse::<f32>()
                    .map_err(|_| "CameraFov is not a number".to_string())?;
                // Native config reader 0x00EDE2D2..0x00EDE2FF clamps to 0..15
                // (upper constant 0x0142AC80). Project edits reject invalid values.
                if value.is_finite() && (0.0..=15.0).contains(&value) {
                    Ok(value)
                } else {
                    Err("CameraFov must be finite and between 0 and 15".into())
                }
            })
            .transpose()?;
        Ok(Self {
            vsync: boolean(ini, DISPLAY, "VSYNC")?,
            dof: boolean(ini, GRAPHICS, "DofFilter")?,
            camera_fov,
            shadow: enumeration(ini, "ShadowQuality", Quality::parse)?,
            filtering: enumeration(ini, "TextureFiltering", Filtering::parse)?,
            view_range: enumeration(ini, "ViewRange", ViewRange::parse)?,
        })
    }

    pub fn write(&self, ini: &mut Ini) {
        for (section, key, value) in [(DISPLAY, "VSYNC", self.vsync), (GRAPHICS, "DofFilter", self.dof)] {
            if let Some(value) = value {
                ini.set(section, key, if value { "ON" } else { "OFF" });
            }
        }
        if let Some(value) = self.camera_fov {
            ini.set(GRAPHICS, "CameraFov", value);
        }
        if let Some(value) = self.shadow {
            ini.set(GRAPHICS, "ShadowQuality", value.token());
        }
        if let Some(value) = self.filtering {
            ini.set(GRAPHICS, "TextureFiltering", value.token());
        }
        if let Some(value) = self.view_range {
            ini.set(GRAPHICS, "ViewRange", value.token());
        }
    }
}

pub fn original_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("CAPCOM/DRAGONS DOGMA DARK ARISEN/config.ini"))
}

/// A different, fixed directory from the game's own config/save directory.
/// No public arbitrary save destination can accidentally target the install.
pub fn project_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("ddda-rs/ddview.ini"))
}

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    pub document: Ini,
    pub graphics: Graphics,
}

impl Store {
    pub fn load() -> Result<Self, String> {
        let path = project_path().ok_or("LOCALAPPDATA is unavailable")?;
        let project = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let original = if project.is_none() {
            original_path().and_then(|p| std::fs::read_to_string(p).ok())
        } else {
            None
        };
        Self::from_text(path, project.as_deref(), original.as_deref())
    }

    fn from_text(path: PathBuf, project: Option<&str>, original: Option<&str>) -> Result<Self, String> {
        let mut document = match project {
            Some(text) => {
                let ini = Ini::parse(text)?;
                if ini.get("DDVIEW", "Version") != Some(CONFIG_VERSION) {
                    return Err("unsupported ddview settings version".into());
                }
                ini
            }
            None => Ini::parse(original.unwrap_or("")).map(Ini::graphics_only)?,
        };
        let graphics = Graphics::read(&document)?;
        document.set("DDVIEW", "Version", CONFIG_VERSION);
        Ok(Self {
            path,
            document,
            graphics,
        })
    }

    pub fn save(&mut self, candidate: &Graphics) -> Result<(), String> {
        let mut document = self.document.clone();
        candidate.write(&mut document);
        let validated = Graphics::read(&document)?;
        let parent = self.path.parent().ok_or("settings path has no parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // Two viewer processes can have separate rendering preferences while
        // sharing the user's config location. Replace a complete file, never
        // leave a partly written INI visible to the next viewer startup.
        let temporary = self.path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, document.encode()).map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(&temporary, &self.path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        self.document = document;
        self.graphics = validated;
        Ok(())
    }
}

/// IDs come from uGUIOption's installed pause_option message resource.
/// Text, option layouts and the renderer's readable tables stay install reads.
pub struct OriginalOptions {
    messages: Vec<String>,
    pub top: Vec<u8>,
    pub panel: Vec<u8>,
}

impl OriginalOptions {
    // Resource loads translated from 0x006A6160 (DDDA.exe 2017-12-13 Steam).
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let top = vfs.read(r"id\DDN\menu\option\top_option", mtarc::type_hash("rGUI"))?;
        let panel = vfs.read(r"id\DDN\p_option\p_option", mtarc::type_hash("rGUI"))?;
        mtgui::Gui::parse(&top)?;
        mtgui::Gui::parse(&panel)?;
        let bytes = vfs.read(
            r"id\DDN\message\pause\pause_option\pause_option_eng",
            mtarc::type_hash("rMessage"),
        )?;
        let messages = mtgmd::MessageFile::parse(&bytes)
            .map_err(|e| e.to_string())?
            .messages
            .into_iter()
            .map(str::to_owned)
            .collect();
        Ok(Self { messages, top, panel })
    }

    pub fn text(&self, id: u32) -> Option<&str> {
        self.messages.get(id as usize).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_roundtrip_preserves_unported_graphics_without_importing_inputs() {
        let original = "[GRAPHICS]\nCameraFov=15\nDofFilter=ON\nShadowQuality=LOW\nTextureFiltering=ANISO_X4\nViewRange=NORMAL\nTextureDetail=MEDIUM\nAltAntiAlias=FXAA3HQ\nTextureMipLimite=2\n[DISPLAY]\nVSYNC=OFF\nResolution=1920x1080\n[KEYBOARDMOUSE]\nSTG_ATTACK_0=256\n";
        let mut store = Store::from_text(PathBuf::from("unused.ini"), None, Some(original)).unwrap();
        assert_eq!(store.graphics.camera_fov, Some(15.0));
        assert_eq!(store.graphics.vsync, Some(false));
        assert_eq!(store.document.get("KEYBOARDMOUSE", "STG_ATTACK_0"), None);
        let changed = Graphics {
            shadow: Some(Quality::High),
            filtering: Some(Filtering::Aniso16),
            view_range: Some(ViewRange::Farthest),
            dof: Some(false),
            vsync: Some(true),
            camera_fov: Some(8.0),
        };
        changed.write(&mut store.document);
        let text = store.document.encode();
        let restored = Store::from_text(PathBuf::from("unused.ini"), Some(&text), Some("not read")).unwrap();
        assert_eq!(restored.graphics, changed);
        assert_eq!(restored.document.get(GRAPHICS, "TextureDetail"), Some("MEDIUM"));
        assert_eq!(restored.document.get(GRAPHICS, "AltAntiAlias"), Some("FXAA3HQ"));
        assert_eq!(restored.document.get(DISPLAY, "Resolution"), Some("1920x1080"));
    }

    #[test]
    fn malformed_settings_fail_without_picking_arbitrary_defaults() {
        for text in [
            "[GRAPHICS]\nCameraFov=NaN",
            "[GRAPHICS]\nCameraFov=16",
            "[GRAPHICS]\nCameraFov=-1",
            "[GRAPHICS]\nShadowQuality=ULTRA",
            "[GRAPHICS]\nDofFilter=yes",
            "[DISPLAY]\nVSYNC=FALSE",
            "[GRAPHICS]\nTextureFiltering=ANISO_X3",
        ] {
            let ini = Ini::parse(text).unwrap();
            assert!(Graphics::read(&ini).is_err(), "{text}");
        }
        assert!(Ini::parse("[GRAPHICS]\nCameraFov=1\nCameraFov=2").is_err());
        assert!(Store::from_text(PathBuf::from("unused.ini"), Some("[DDVIEW]\nVersion=2"), None).is_err());
        assert_eq!(Graphics::read(&Ini::default()).unwrap(), Graphics::default());
        let unrelated = Ini::parse("[OTHER]\nCameraFov=NaN\nVSYNC=FALSE").unwrap();
        assert_eq!(Graphics::read(&unrelated).unwrap(), Graphics::default());
    }

    #[test]
    fn shadow_allocations_and_sampler_levels_follow_native_tables() {
        assert_eq!(Quality::ALL.map(Quality::shadow_resolution), [512, 1024, 2048]);
        assert_eq!(Filtering::ALL.map(Filtering::anisotropy), [1, 2, 4, 8, 16]);
        assert_eq!(ViewRange::ALL.map(ViewRange::native), [1, 2, 3]);
        // A reflection/cube shadow uses half resolution in 0x00DA97D0;
        // this mapping is specifically the stable sun's arg-zero allocation.
        assert_eq!(Quality::High.shadow_resolution(), 2048);
    }

    #[test]
    fn filtering_preserves_compressed_payload_mips_dimensions_and_addressing() {
        use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let mut image = Image::new_uninit(
            Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Bc1RgbaUnormSrgb,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = 3;
        image.data = Some(vec![0x47; 48]);
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::MirrorRepeat,
            lod_min_clamp: 1.0,
            lod_max_clamp: 3.0,
            ..ImageSamplerDescriptor::linear()
        });
        let before = image.clone();
        for filtering in Filtering::ALL {
            filter_image(&mut image, filtering);
            let ImageSampler::Descriptor(sampler) = &image.sampler else {
                panic!()
            };
            assert_eq!(sampler.anisotropy_clamp, filtering.anisotropy());
            assert_eq!(sampler.address_mode_u, ImageAddressMode::Repeat);
            assert_eq!(sampler.address_mode_v, ImageAddressMode::MirrorRepeat);
            assert_eq!((sampler.lod_min_clamp, sampler.lod_max_clamp), (1.0, 3.0));
            assert_eq!(image.data, before.data);
            assert_eq!(image.texture_descriptor.format, before.texture_descriptor.format);
            assert_eq!(image.texture_descriptor.size, before.texture_descriptor.size);
            assert_eq!(image.texture_descriptor.mip_level_count, 3);
        }
    }

    #[test]
    fn own_file_save_is_reloaded_and_rejected_edits_do_not_replace_it() {
        let dir = std::env::temp_dir().join(format!("ddview-s42-{}", std::process::id()));
        let path = dir.join("ddview.ini");
        let mut store = Store::from_text(path.clone(), None, None).unwrap();
        let mut candidate = Graphics {
            camera_fov: Some(15.0),
            dof: Some(false),
            ..Graphics::default()
        };
        store.save(&candidate).unwrap();
        candidate.camera_fov = Some(8.0);
        store.save(&candidate).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        let restored = Store::from_text(path.clone(), Some(&first), None).unwrap();
        assert_eq!(restored.graphics, candidate);
        let invalid = Graphics {
            camera_fov: Some(f32::INFINITY),
            ..candidate
        };
        assert!(store.save(&invalid).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        assert_eq!(store.graphics, restored.graphics);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn installed_options_and_shadow_table_match_the_traced_build() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        if !native.is_dir() {
            return;
        }
        let file = ddexe::ExeFile::next_to(&native).unwrap();
        let exe = file.image().unwrap();
        assert_eq!(exe.u32s(0x0142_92bc, 3).unwrap(), [512, 1024, 2048]);
        assert_eq!(exe.f32(0x0142_ac80).unwrap(), 15.0);
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let options = OriginalOptions::load(&vfs).unwrap();
        assert_eq!(options.text(139), Some("Screen Resolution"));
        assert_eq!(options.text(237), Some("Depth of Field"));
        assert_eq!(mtgui::Gui::parse(&options.top).unwrap().nodes.len(), 170);
        for id in (139..=151).chain([233, 234, 237, 238]) {
            assert!(
                !options.text(id).unwrap().is_empty(),
                "missing option message {id}"
            );
        }
    }
}
