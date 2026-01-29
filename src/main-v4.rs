use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::pbr::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use bevy_rapier3d::prelude::*;
use std::f32::consts::PI;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::bloom::Bloom;
use bevy::render::render_resource::{AddressMode, SamplerDescriptor};
use bevy::image::ImageSampler;
// use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use rand::prelude::*;

// --- TUNING CONSTANTS ---

const PLAYER_SPEED: f32 = 45.0; 
const GRAVITY_SCALE: f32 = 6.0;

// Camera
const CAM_SMOOTH_SPEED: f32 = 10.0;
const GRID_SIZE: f32 = 4.0; 

// Costs
const DRILL_COST: u32 = 50;
const TURRET_COST: u32 = 80;
const WALL_COST: u32 = 10;
const BARRACKS_COST: u32 = 200;
const BUILDER_HUT_COST: u32 = 150;
const STORAGE_COST: u32 = 100;

// Logic
const DRILL_CAPACITY: u32 = 5;
const WORKER_SPEED: f32 = 18.0;

// --- RESOURCES ---

#[derive(States, Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
enum GameState {
    #[default]
    Playing,
    GameOver,
}

#[derive(Resource)]
struct PlayerStats {
    scrap: u32,
    max_scrap: u32,
    unit_count: u32,
    unit_cap: u32,
}

#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
enum BuildTool {
    #[default]
    Drill,
    Turret,
    Wall,
    Barracks,
    BuilderHut,
    Storage,
}

#[derive(Resource)]
struct BuildManager {
    tool: BuildTool,
    rotation_idx: u32,
    is_drag_building: bool,
    drag_start: Option<Vec3>,
}

#[derive(Resource, Default)]
struct WorldCursor {
    pos: Vec3,
    snapped_pos: Vec3,
    on_ground: bool,
}

#[derive(Resource, Default)]
struct SelectionState {
    start_pos: Option<Vec2>,
    current_pos: Option<Vec2>,
    is_selecting: bool,
    drag_threshold_met: bool,
    selected_entities: Vec<Entity>,
}

#[derive(Resource)]
struct PhaseManager {
    timer: Timer,
    wave: u32,
    is_combat: bool,
}

#[derive(Resource)]
struct GameAssets {
    debug_tex: Handle<Image>,
}

// --- COMPONENTS ---

#[derive(Component)]
struct Player { 
    fire_timer: f32,
    jump_count: u32,
    dash_timer: f32,
    dash_cooldown: f32,
    is_somersaulting: bool,
}

#[derive(Component)]
struct Health { current: f32, max: f32 }

#[derive(Component)]
struct Structure; 

#[derive(Component)]
struct Selected; // Tag for selected items

#[derive(Component)]
struct Drill { 
    timer: Timer,
    storage: u32,
}

#[derive(Component)]
struct StorageBin;

#[derive(Component)]
struct BuilderHut { 
    spawn_timer: Timer, 
    worker_count: u32, 
    max_workers: u32 
}

#[derive(Component)]
struct Worker {
    carrying: bool,
    target_drill: Option<Entity>,
    target_storage: Option<Entity>,
}

#[derive(Component)]
struct CarryingVisual; // Child entity of worker

#[derive(Component)]
struct Barracks { 
    timer: Timer,
    spawn_drone_next: bool, 
}

#[derive(Component)]
struct HomeBase { pos: Vec3 } 

#[derive(Component)]
struct Unit {
    is_flying: bool,
    patrol_offset: Vec3,
    patrol_timer: Timer,
}

#[derive(Component)]
struct Turret { cooldown: f32 }

#[derive(Component)]
struct Enemy {
    #[allow(dead_code)]
    is_giant: bool 
}

#[derive(Component)]
struct Projectile { 
    damage: f32, 
    lifetime: Timer, 
    #[allow(dead_code)]
    from_player: bool 
}

#[derive(Component)]
struct Particle {
    pub velocity: Vec3,
    pub lifetime: Timer,
    pub fade: bool,
    pub initial_scale: f32,
}

#[derive(Component)]
struct Steer {
    pub target: Option<Vec3>,
    pub speed: f32,
}

impl Default for Steer {
    fn default() -> Self {
        Self { target: None, speed: 20.0 }
    }
}

#[derive(Component)]
struct WowCameraRig {
    pub yaw: f32,
    pub pitch: f32,
    pub radius: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub target_radius: f32,
}

impl Default for WowCameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0, pitch: PI / 6.0, radius: 50.0,
            target_yaw: 0.0, target_pitch: PI / 6.0, target_radius: 50.0,
        }
    }
}

#[derive(Component)]
struct MuzzleFlash { timer: Timer }

#[derive(Component)]
struct Bob {
    pub speed: f32,
    pub amount: f32,
    pub base_y: f32,
    pub offset: f32,
}

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct FancyCursorVisual;

// --- MAIN ---

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "SWARM DEFENSE: LOGISTICS COMMANDER".into(),
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            RapierPhysicsPlugin::<NoUserData>::default(),
            // FrameTimeDiagnosticsPlugin, // Uncomment for FPS
            // LogDiagnosticsPlugin::default(),
        ))
        .init_state::<GameState>()
        .insert_resource(PlayerStats { scrap: 600, max_scrap: 1000, unit_count: 0, unit_cap: 5 })
        .insert_resource(BuildManager { tool: BuildTool::Drill, rotation_idx: 0, is_drag_building: false, drag_start: None })
        .init_resource::<WorldCursor>()
        .init_resource::<SelectionState>()
        .insert_resource(PhaseManager { 
            timer: Timer::from_seconds(60.0, TimerMode::Once), 
            wave: 1, 
            is_combat: false 
        })
        .insert_resource(ClearColor(Color::srgb(0.01, 0.01, 0.03)))
        .insert_resource(AmbientLight { color: Color::srgb(0.1, 0.1, 0.2), brightness: 100.0 })
        .add_systems(PreStartup, setup_assets)
        .add_systems(Startup, (setup_world, setup_environment, setup_player, setup_ui, setup_cursor_visuals))
        .add_systems(Update, (
            wow_camera_system,     
            cursor_raycast_system,
            update_cursor_visual,
            update_hud,
            check_game_over,
            muzzle_flash_logic,
            projectile_logic,
            selection_input_system,
            draw_selection_gizmos,
            delete_selected_system,
            particle_system,
            steering_system,
            bob_system,
        ))
        .add_systems(Update, (
            wow_movement_system,
            player_bounce_system,
            phase_logic,
            build_tool_input,
            ghost_preview_system,
            place_building_system, // Includes drag building
            weapon_mechanics,
            manual_repair,
            // Logic
            unit_spawner_system,
            unit_idle_behavior,
            unit_combat_ai,
            worker_spawner,
            worker_logistics_ai,
            drill_production,
            turret_ai,
            enemy_spawner,
            enemy_ai,
        ).run_if(in_state(GameState::Playing)))
        .add_systems(Update, restart_game_system.run_if(in_state(GameState::GameOver)))
        .run();
}

fn bob_system(time: Res<Time>, mut q: Query<(&mut Transform, &Bob)>) {
    let t = time.elapsed_secs();
    for (mut transform, bob) in q.iter_mut() {
        transform.translation.y = bob.base_y + (t * bob.speed + bob.offset).sin() * bob.amount;
    }
}

fn setup_assets(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    const SIZE: usize = 64;
    let mut data = vec![255u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = (y * SIZE + x) * 4;
            let on_edge = x < 2 || y < 2 || x > SIZE - 3 || y > SIZE - 3;
            if on_edge {
                data[i] = 200; data[i+1] = 200; data[i+2] = 220; data[i+3] = 255;
            } else {
                data[i] = 30; data[i+1] = 30; data[i+2] = 45; data[i+3] = 255;
            }
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::render::render_asset::RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(SamplerDescriptor {
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        ..default()
    }.into());
    let debug_tex = images.add(image);
    commands.insert_resource(GameAssets { debug_tex });
}

// --- SETUP & ENV ---

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<GameAssets>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Camera { hdr: true, ..default() },
        Tonemapping::TonyMcMapface,
        Bloom { 
            intensity: 0.5, 
            low_frequency_boost: 0.7,
            ..default() 
        },
        WowCameraRig::default(),
        Transform::from_xyz(0.0, 50.0, 50.0),
        DistanceFog {
            color: Color::srgb(0.02, 0.01, 0.05), // Matches ClearColor
            falloff: FogFalloff::Linear { start: 100.0, end: 500.0 }, // Clear center, foggy distance
            ..default()
        },
    ));

    // Sun with Shadows
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0, // Brighter
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.95, 1.0), 
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -PI / 3.0, PI / 4.0, 0.0)),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 10.0,
            maximum_distance: 150.0,
            ..default()
        }.build(),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(1000.0, 1000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(assets.debug_tex.clone()),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RigidBody::Fixed,
        Collider::halfspace(Vec3::Y).unwrap(),
    ));
}

fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // City Materials
    let building_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.08),
        perceptual_roughness: 0.8,
        metallic: 0.2,
        ..default()
    });

    let window_mats = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.5, 0.7, 1.0),
            emissive: LinearRgba::new(0.5, 1.5, 5.0, 1.0), // Bright Blue
            unlit: false,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.8, 0.4),
            emissive: LinearRgba::new(3.0, 1.0, 0.2, 1.0), // Warm Orange
            unlit: false,
            ..default()
        }),
    ];

    let mesh_cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut rng = rand::thread_rng();

    // Spawn Buildings in a ring
    for i in 0..60 {
        let angle = (i as f32 / 60.0) * PI * 2.0;
        let dist = 140.0 + rng.r#gen::<f32>() * 60.0;
        let w = 15.0 + rng.r#gen::<f32>() * 15.0;
        let h = 40.0 + rng.r#gen::<f32>() * 100.0;
        let d = 15.0 + rng.r#gen::<f32>() * 15.0;
        
        let x = angle.cos() * dist;
        let z = angle.sin() * dist;

        commands.spawn((
            Mesh3d(mesh_cube.clone()),
            MeshMaterial3d(building_mat.clone()),
            Transform::from_xyz(x, h/2.0, z).with_scale(Vec3::new(w, h, d)),
        )).with_children(|parent| {
            // Add grid of windows on 4 sides
            for side in 0..4 {
                let (normal, rot) = match side {
                    0 => (Vec3::Z, Quat::IDENTITY),
                    1 => (Vec3::X, Quat::from_rotation_y(PI/2.0)),
                    2 => (-Vec3::Z, Quat::from_rotation_y(PI)),
                    3 => (-Vec3::X, Quat::from_rotation_y(-PI/2.0)),
                    _ => unreachable!(),
                };

                // Rows and Columns of windows
                let cols = (w / 4.0) as i32;
                let rows = (h / 6.0) as i32;

                for r in 0..rows {
                    if rng.r#gen_bool(0.3) { continue; } // Random empty rows
                    for c in 0..cols {
                        if rng.r#gen_bool(0.4) { continue; } // Random light off

                        let win_mat = &window_mats[rng.r#gen_range(0..window_mats.len())];
                        
                        let lx = (c as f32 - (cols as f32 - 1.0) / 2.0) * (4.0/w);
                        let ly = (r as f32 - (rows as f32 - 1.0) / 2.0) * (6.0/h);
                        
                        parent.spawn((
                            Mesh3d(mesh_cube.clone()),
                            MeshMaterial3d(win_mat.clone()),
                            Transform {
                                translation: normal * 0.51 + rot * Vec3::new(lx * 0.45, ly * 0.45, 0.0),
                                rotation: rot,
                                scale: Vec3::new(0.05, 0.05, 0.01),
                            },
                        ));
                    }
                }
            }
        });
    }
}

fn setup_player(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, assets: Res<GameAssets>) {
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.4, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 0.8, 1.0),
            base_color_texture: Some(assets.debug_tex.clone()),
            emissive: LinearRgba::new(0.0, 0.8, 1.0, 2.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 5.0, 0.0),
        Player { 
            fire_timer: 0.0,
            jump_count: 0,
            dash_timer: 0.0,
            dash_cooldown: 0.0,
            is_somersaulting: false,
        },
        Health { current: 500.0, max: 500.0 },
        RigidBody::Dynamic, Collider::capsule_y(0.5, 0.4), LockedAxes::ROTATION_LOCKED,
        Velocity::default(), Friction::coefficient(0.0), GravityScale(GRAVITY_SCALE),
    )).with_children(|parent| {
        parent.spawn(PointLight { color: Color::srgb(0.0, 1.0, 1.0), intensity: 1000.0, range: 15.0, ..default() });
    });
}
fn setup_cursor_visuals(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // Fancy Cursor Entity: A super thin flat torus base with a pulsing vertical pointer
    let ring_mesh = meshes.add(Torus::new(0.005, 0.4)); // Much thinner and smaller radius
    let line_mesh = meshes.add(Cylinder::new(0.005, 8.0)); // Extremely thin vertical line
    
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.0), // Gold
        emissive: LinearRgba::new(5.0, 4.0, 0.0, 2.0),
        unlit: true,
        ..default()
    });

    commands.spawn((
        Transform::default(),
        Visibility::Visible,
        FancyCursorVisual,
        Bob { speed: 2.0, amount: 0.3, base_y: 0.0, offset: 0.0 },
    )).with_children(|parent| {
        // Horizontal thin ring (lying flat on ground)
        parent.spawn((
            Mesh3d(ring_mesh),
            MeshMaterial3d(mat.clone()),
            Transform::IDENTITY, // Torus is XZ plane by default in Bevy
        ));
        // Pulsing Vertical Line
        parent.spawn((
            Mesh3d(line_mesh),
            MeshMaterial3d(mat),
            Transform::from_xyz(0.0, 4.0, 0.0),
            AnimatedPointer,
        ));
    });
}

#[derive(Component)]
struct AnimatedPointer;

fn setup_ui(mut commands: Commands) {
    commands.spawn(Node { width: Val::Percent(100.0), padding: UiRect::all(Val::Px(10.0)), ..default() }).with_children(|root| {
        root.spawn((
            Node {
                position_type: PositionType::Absolute, left: Val::Px(10.0), top: Val::Px(10.0),
                padding: UiRect::all(Val::Px(15.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            BorderRadius::all(Val::Px(8.0)),
        )).with_children(|panel| {
            panel.spawn((Text::new("Init..."), TextFont { font_size: 16.0, ..default() }, TextColor(Color::WHITE), HudText));
        });
    });
}

// --- INPUT & CAMERA ---

fn wow_camera_system(
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut q_win: Query<&mut Window, With<PrimaryWindow>>,
    mut q_cam: Query<(&mut Transform, &mut WowCameraRig)>,
    mut q_player: Query<&mut Transform, (With<Player>, Without<WowCameraRig>)>,
    rapier: Single<&RapierContext>,
    time: Res<Time>,
) {
    let Ok(mut window) = q_win.get_single_mut() else { return };
    let Ok((mut cam_t, mut rig)) = q_cam.get_single_mut() else { return };
    let Ok(mut player_t) = q_player.get_single_mut() else { return };
    
    let dt = time.delta_secs();
    let delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
    let right_click = mouse_btn.pressed(MouseButton::Right);
    let _alt_held = keys.pressed(KeyCode::AltLeft);

    // Cursor Lock
    if right_click {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
        
        rig.target_yaw -= delta.x * 0.003;
        rig.target_pitch = (rig.target_pitch - delta.y * 0.003).clamp(0.1, PI / 2.1);
        
        // Face player to camera
        let target_rot = Quat::from_rotation_y(rig.target_yaw);
        player_t.rotation = player_t.rotation.slerp(target_rot, dt * 20.0);
    } else {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }

    for ev in mouse_wheel.read() {
        rig.target_radius = (rig.target_radius - ev.y * 5.0).clamp(10.0, 120.0);
    }

    // Smooth
    rig.yaw = rig.yaw.lerp(rig.target_yaw, dt * CAM_SMOOTH_SPEED);
    rig.pitch = rig.pitch.lerp(rig.target_pitch, dt * CAM_SMOOTH_SPEED);
    rig.radius = rig.radius.lerp(rig.target_radius, dt * 5.0);

    // Calc Position
    let head = player_t.translation + Vec3::new(0.0, 1.5, 0.0);
    let rot = Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch);
    let desired = head + rot * Vec3::new(0.0, 0.0, rig.radius);

    // Collision
    let dir = (desired - head).normalize();
    let mut final_pos = desired;
    if let Some((_, dist)) = rapier.cast_ray(head, dir, rig.radius, true, QueryFilter::exclude_dynamic()) {
        final_pos = head + dir * (dist - 0.5).max(0.5);
    }
    
    cam_t.translation = final_pos;
    cam_t.look_at(head, Vec3::Y);
}

fn cursor_raycast_system(
    mut cursor: ResMut<WorldCursor>,
    q_win: Query<&Window, With<PrimaryWindow>>,
    q_cam: Query<(&Camera, &GlobalTransform)>,
) {
    let (cam, cam_t) = q_cam.single();
    let win = q_win.single();

    if let Some(screen_pos) = win.cursor_position() {
        if let Ok(ray) = cam.viewport_to_world(cam_t, screen_pos) {
            let t = -ray.origin.y / ray.direction.y;
            if t > 0.0 {
                cursor.pos = ray.origin + ray.direction * t;
                cursor.snapped_pos = (cursor.pos / GRID_SIZE).round() * GRID_SIZE;
                cursor.snapped_pos.y = 0.0;
                cursor.on_ground = true;
                return;
            }
        }
    }
    cursor.on_ground = false;
}

fn update_cursor_visual(
    cursor: Res<WorldCursor>,
    mut q_vis: Query<(&mut Transform, &mut Bob), (With<FancyCursorVisual>, Without<AnimatedPointer>)>,
    mut q_pointer: Query<&mut Transform, With<AnimatedPointer>>,
    time: Res<Time>,
) {
    if let Ok((mut t, mut bob)) = q_vis.get_single_mut() {
        if cursor.on_ground {
            t.translation = cursor.pos;
            bob.base_y = cursor.pos.y + 0.1; 
            t.scale = Vec3::ONE;
            t.rotate_y(time.delta_secs() * 1.5);
            
            // Animate pointer height/pulse
            if let Ok(mut pt) = q_pointer.get_single_mut() {
                let pulse = (time.elapsed_secs() * 5.0).sin() * 0.5 + 1.0;
                pt.scale.y = pulse;
            }
        } else {
            t.scale = Vec3::ZERO;
        }
    }
}

// --- SELECTION & DELETION ---

fn selection_input_system(
    mut sel: ResMut<SelectionState>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    q_win: Query<&Window, With<PrimaryWindow>>,
    q_cam: Query<(&Camera, &GlobalTransform)>,
    q_structures: Query<(Entity, &GlobalTransform), With<Structure>>,
    mut commands: Commands,
) {
    let Ok(win) = q_win.get_single() else { return };
    let Ok((cam, cam_t)) = q_cam.get_single() else { return };

    let alt = keys.pressed(KeyCode::AltLeft);

    // Initial press (don't start selecting yet, just record start pos)
    if !alt && mouse.just_pressed(MouseButton::Left) {
        if let Some(pos) = win.cursor_position() {
            sel.start_pos = Some(pos);
            sel.drag_threshold_met = false;
            sel.is_selecting = false;
        }
    }

    if let Some(start) = sel.start_pos {
        if mouse.pressed(MouseButton::Left) {
            if let Some(pos) = win.cursor_position() {
                sel.current_pos = Some(pos);
                // Check if moved enough to be a drag
                if !sel.drag_threshold_met && start.distance(pos) > 15.0 {
                    sel.drag_threshold_met = true;
                    sel.is_selecting = true;
                    // Clear previous selection on start of drag
                    for e in &sel.selected_entities { 
                        if let Some(mut ec) = commands.get_entity(*e) {
                            ec.remove::<Selected>(); 
                        }
                    }
                    sel.selected_entities.clear();
                }
            }
        }
        
        if mouse.just_released(MouseButton::Left) || alt {
            if sel.is_selecting {
                if let Some(end) = sel.current_pos {
                    let min = start.min(end);
                    let max = start.max(end);
                    let rect = Rect { min, max };

                    for (e, t) in q_structures.iter() {
                        if let Ok(screen_pos) = cam.world_to_viewport(cam_t, t.translation()) {
                            if rect.contains(screen_pos) {
                                commands.entity(e).insert(Selected);
                                sel.selected_entities.push(e);
                            }
                        }
                    }
                }
            }
            sel.is_selecting = false;
            sel.drag_threshold_met = false;
            sel.start_pos = None;
            sel.current_pos = None;
        }
    }
}

fn draw_selection_gizmos(
    sel: Res<SelectionState>, 
    mut gizmos: Gizmos, 
    _q_win: Query<&Window, With<PrimaryWindow>>, 
    q_cam: Query<(&Camera, &GlobalTransform)>,
    q_selected: Query<(Entity, &GlobalTransform), With<Selected>>,
) {
    if sel.is_selecting {
        if let (Some(start), Some(end)) = (sel.start_pos, sel.current_pos) {
            let Ok((cam, cam_t)) = q_cam.get_single() else { return };

            // Draw a rectangle in world space at y=0 for the selection area
            if let (Ok(ray_start), Ok(ray_end)) = (cam.viewport_to_world(cam_t, start), cam.viewport_to_world(cam_t, end)) {
                let t_start = -ray_start.origin.y / ray_start.direction.y;
                let t_end = -ray_end.origin.y / ray_end.direction.y;
                
                if t_start > 0.0 && t_end > 0.0 {
                    let p_start = ray_start.origin + ray_start.direction * t_start;
                    let p_end = ray_end.origin + ray_end.direction * t_end;
                    
                    let min_x = p_start.x.min(p_end.x);
                    let max_x = p_start.x.max(p_end.x);
                    let min_z = p_start.z.min(p_end.z);
                    let max_z = p_start.z.max(p_end.z);
                    
                    let color = Color::srgb(1.0, 1.0, 0.0);
                    let p1 = Vec3::new(min_x, 0.1, min_z);
                    let p2 = Vec3::new(max_x, 0.1, min_z);
                    let p3 = Vec3::new(max_x, 0.1, max_z);
                    let p4 = Vec3::new(min_x, 0.1, max_z);
                    gizmos.line(p1, p2, color);
                    gizmos.line(p2, p3, color);
                    gizmos.line(p3, p4, color);
                    gizmos.line(p4, p1, color);
                }
            }
        }
    }
    
    // Draw highlight boxes around selected
    for (_, t) in q_selected.iter() {
        gizmos.cuboid(Transform::from_translation(t.translation() + Vec3::Y * 0.5).with_scale(Vec3::splat(4.5)), Color::srgb(1.0, 0.0, 0.0));
    }
}

fn delete_selected_system(
    mut commands: Commands,
    mut sel: ResMut<SelectionState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut stats: ResMut<PlayerStats>,
) {
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        for e in &sel.selected_entities {
            // Refund some scrap
            stats.scrap += 5; 
            commands.entity(*e).despawn_recursive();
        }
        sel.selected_entities.clear();
    }
}

// --- BUILDING SYSTEM ---

fn build_tool_input(keys: Res<ButtonInput<KeyCode>>, mut mgr: ResMut<BuildManager>) {
    if keys.just_pressed(KeyCode::Digit1) { mgr.tool = BuildTool::Drill; }
    if keys.just_pressed(KeyCode::Digit2) { mgr.tool = BuildTool::Turret; }
    if keys.just_pressed(KeyCode::Digit3) { mgr.tool = BuildTool::Wall; }
    if keys.just_pressed(KeyCode::Digit4) { mgr.tool = BuildTool::Barracks; }
    if keys.just_pressed(KeyCode::Digit5) { mgr.tool = BuildTool::BuilderHut; }
    if keys.just_pressed(KeyCode::Digit6) { mgr.tool = BuildTool::Storage; }
    if keys.just_pressed(KeyCode::KeyR) { mgr.rotation_idx = (mgr.rotation_idx + 1) % 4; }
}

#[derive(Component)]
struct Ghost;

fn ghost_preview_system(
    mut commands: Commands,
    mgr: Res<BuildManager>,
    cursor: Res<WorldCursor>,
    keys: Res<ButtonInput<KeyCode>>,
    q_ghosts: Query<Entity, With<Ghost>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Clear all existing ghosts every frame to simplify drag logic
    for e in q_ghosts.iter() {
        commands.entity(e).despawn_recursive();
    }

    let show_ghost = keys.pressed(KeyCode::AltLeft); // Show ghost ONLY when Alt is held
    if !show_ghost || !cursor.on_ground {
        return;
    }

    let ghost_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 1.0, 0.0, 0.3),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    let mut points_to_preview = Vec::new();

    if mgr.is_drag_building && mgr.tool == BuildTool::Wall {
        if let Some(start) = mgr.drag_start {
            let end = cursor.snapped_pos;
            let dist = start.distance(end);
            let count = (dist / GRID_SIZE).round() as u32;
            if count == 0 {
                points_to_preview.push(start);
            } else {
                for i in 0..=count {
                    let t = i as f32 / count as f32;
                    let p = start.lerp(end, t);
                    let snapped = (p / GRID_SIZE).round() * GRID_SIZE;
                    points_to_preview.push(snapped);
                }
            }
        }
    } else {
        points_to_preview.push(cursor.snapped_pos);
    }

    for pos in points_to_preview {
        let (mesh, y_off) = match mgr.tool {
            BuildTool::Wall => (meshes.add(Cuboid::new(4.0, 3.0, 1.0)), 1.5),
            BuildTool::Drill => (meshes.add(Cuboid::new(3.0, 4.0, 3.0)), 2.0),
            BuildTool::Turret => (meshes.add(Cuboid::new(1.0, 2.5, 1.0)), 1.25),
            BuildTool::Barracks => (meshes.add(Cuboid::new(4.0, 3.0, 4.0)), 1.5),
            BuildTool::BuilderHut => (meshes.add(Cuboid::new(2.5, 2.5, 2.5)), 1.25),
            BuildTool::Storage => (meshes.add(Cylinder::new(2.0, 2.0)), 1.0),
        };

        let rot = Quat::from_rotation_y(mgr.rotation_idx as f32 * (PI / 2.0));
        let p = pos + Vec3::new(0.0, y_off, 0.0);

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(ghost_mat.clone()),
            Transform::from_translation(p).with_rotation(rot),
            NotShadowCaster,
            Ghost,
        ));
    }
}

fn place_building_system(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<WorldCursor>,
    mut mgr: ResMut<BuildManager>,
    mut stats: ResMut<PlayerStats>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<GameAssets>,
) {
    if !keys.pressed(KeyCode::AltLeft) { return; } // Only build if Alt is held

    let _left_down = mouse.pressed(MouseButton::Left);
    let left_just = mouse.just_pressed(MouseButton::Left);
    let left_up = mouse.just_released(MouseButton::Left);

    // Setup materials
    let drill_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.5, 0.0), base_color_texture: Some(assets.debug_tex.clone()), metallic: 0.8, ..default() });
    let turret_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.5, 0.5, 0.5), base_color_texture: Some(assets.debug_tex.clone()), metallic: 0.5, ..default() });
    let wall_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.3, 0.3), base_color_texture: Some(assets.debug_tex.clone()), perceptual_roughness: 0.8, ..default() });
    let barracks_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.0, 0.2, 0.8), base_color_texture: Some(assets.debug_tex.clone()), emissive: LinearRgba::new(0.0, 0.5, 1.0, 1.0), ..default() });
    let hut_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.4, 0.2), base_color_texture: Some(assets.debug_tex.clone()), ..default() });
    let store_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 1.0, 0.0), base_color_texture: Some(assets.debug_tex.clone()), metallic: 0.9, ..default() });

    let mut points_to_build = Vec::new();

    // Logic for Single Click vs Drag
    if mgr.tool == BuildTool::Wall {
        if left_just {
            mgr.is_drag_building = true;
            mgr.drag_start = Some(cursor.snapped_pos);
        }
        
        if mgr.is_drag_building && left_up {
            mgr.is_drag_building = false;
            if let Some(start) = mgr.drag_start {
                let end = cursor.snapped_pos;
                // Interpolate
                let dist = start.distance(end);
                let count = (dist / GRID_SIZE).round() as u32;
                if count == 0 {
                    points_to_build.push(start);
                } else {
                    for i in 0..=count {
                        let t = i as f32 / count as f32;
                        let p = start.lerp(end, t);
                        let snapped = (p / GRID_SIZE).round() * GRID_SIZE;
                        points_to_build.push(snapped);
                    }
                }
            }
            mgr.drag_start = None;
        }
    } else {
        if left_just {
            points_to_build.push(cursor.snapped_pos);
        }
    }

    // Process Build Queue
    for pos in points_to_build {
        // Simple duplicate check (very basic)
        // Ideally use a spatial hash map
        
        let (cost, mesh, mat, collider, y_off) = match mgr.tool {
            BuildTool::Wall => (WALL_COST, meshes.add(Cuboid::new(4.0, 3.0, 1.0)), wall_mat.clone(), Collider::cuboid(2.0, 1.5, 0.5), 1.5),
            BuildTool::Drill => (DRILL_COST, meshes.add(Cuboid::new(3.0, 4.0, 3.0)), drill_mat.clone(), Collider::cuboid(1.5, 2.0, 1.5), 2.0),
            BuildTool::Turret => (TURRET_COST, meshes.add(Cuboid::new(1.0, 2.5, 1.0)), turret_mat.clone(), Collider::cuboid(0.5, 1.25, 0.5), 1.25),
            BuildTool::Barracks => (BARRACKS_COST, meshes.add(Cuboid::new(4.0, 3.0, 4.0)), barracks_mat.clone(), Collider::cuboid(2.0, 1.5, 2.0), 1.5),
            BuildTool::BuilderHut => (BUILDER_HUT_COST, meshes.add(Cuboid::new(2.5, 2.5, 2.5)), hut_mat.clone(), Collider::cuboid(1.25, 1.25, 1.25), 1.25),
            BuildTool::Storage => (STORAGE_COST, meshes.add(Cylinder::new(2.0, 2.0)), store_mat.clone(), Collider::cylinder(1.0, 2.0), 1.0),
        };

        if stats.scrap >= cost {
            stats.scrap -= cost;
            let rot = Quat::from_rotation_y(mgr.rotation_idx as f32 * (PI / 2.0));
            
            // Wall Rotation Logic fix
            let final_collider = if mgr.tool == BuildTool::Wall && mgr.rotation_idx % 2 != 0 {
                Collider::cuboid(0.5, 1.5, 2.0)
            } else {
                collider
            };

            let id = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_translation(pos + Vec3::new(0.0, y_off, 0.0)).with_rotation(rot),
                Structure,
                Health { current: 150.0, max: 150.0 },
                RigidBody::Fixed,
                final_collider,
            )).id();

            match mgr.tool {
                BuildTool::Drill => { commands.entity(id).insert(Drill { timer: Timer::from_seconds(2.0, TimerMode::Repeating), storage: 0 }); },
                BuildTool::Turret => { 
                    commands.entity(id).insert(Turret { cooldown: 0.0 }); 
                    // Add Archer
                    let archer_mesh = meshes.add(Capsule3d::new(0.2, 0.5));
                    let archer_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.0, 1.0, 0.5), emissive: LinearRgba::new(0.0, 1.0, 0.0, 1.0), ..default() });
                    commands.spawn((
                        Mesh3d(archer_mesh), MeshMaterial3d(archer_mat),
                        Transform::from_xyz(0.0, 1.5, 0.0),
                    )).set_parent(id);
                },
                BuildTool::Barracks => { 
                    commands.entity(id).insert((
                        Barracks { timer: Timer::from_seconds(8.0, TimerMode::Repeating), spawn_drone_next: true },
                        HomeBase { pos: pos + Vec3::new(0.0, 5.0, 0.0) }
                    )); 
                    stats.unit_cap += 3;
                },
                BuildTool::BuilderHut => {
                    commands.entity(id).insert(BuilderHut { spawn_timer: Timer::from_seconds(5.0, TimerMode::Once), worker_count: 0, max_workers: 2 });
                },
                BuildTool::Storage => {
                    commands.entity(id).insert(StorageBin);
                    stats.max_scrap += 500;
                },
                _ => {}
            }
        }
    }
}

// --- LOGISTICS AI (WORKERS) ---

fn worker_spawner(
    time: Res<Time>,
    mut huts: Query<(&mut BuilderHut, &GlobalTransform)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let worker_mesh = meshes.add(Capsule3d::new(0.25, 0.5));
    let worker_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 1.0, 0.0), ..default() });
    
    for (mut hut, t) in huts.iter_mut() {
        if hut.worker_count < hut.max_workers {
            hut.spawn_timer.tick(time.delta());
            if hut.spawn_timer.finished() {
                hut.worker_count += 1;
                commands.spawn((
                    Mesh3d(worker_mesh.clone()),
                    MeshMaterial3d(worker_mat.clone()),
                    Transform::from_translation(t.translation() + Vec3::new(1.0, 0.5, 0.0)),
                    Worker { carrying: false, target_drill: None, target_storage: None },
                    Health { current: 20.0, max: 20.0 },
                    RigidBody::Dynamic, Collider::capsule_y(0.25, 0.25), LockedAxes::ROTATION_LOCKED,
                    Velocity::default(),
                    Steer { speed: WORKER_SPEED, ..default() },
                    Bob { speed: 5.0, amount: 0.15, base_y: 0.5, offset: rand::random::<f32>() * PI },
                ));
            }
        }
    }
}

fn worker_logistics_ai(
    mut commands: Commands,
    mut workers: Query<(Entity, &mut Worker, &mut Transform, &mut Steer)>,
    mut drills: Query<(Entity, &GlobalTransform, &mut Drill), Without<Worker>>,
    storages: Query<(Entity, &GlobalTransform), With<StorageBin>>,
    mut stats: ResMut<PlayerStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = meshes.add(Cuboid::new(0.3, 0.3, 0.3));
    let scrap_mat = materials.add(StandardMaterial { emissive: LinearRgba::new(1.0, 0.5, 0.0, 2.0), ..default() });

    for (w_entity, mut worker, mut t, mut steer) in workers.iter_mut() {
        if !worker.carrying {
            // FIND SCRAP
            if worker.target_drill.is_none() {
                // Find drill with scrap
                let mut best_drill = None;
                let mut min_dist = 9999.0;
                for (d_e, d_t, drill) in drills.iter() {
                    if drill.storage > 0 {
                        let dist = t.translation.distance(d_t.translation());
                        if dist < min_dist { min_dist = dist; best_drill = Some(d_e); }
                    }
                }
                worker.target_drill = best_drill;
            }

            if let Some(target) = worker.target_drill {
                if let Ok((_, target_t, mut drill_comp)) = drills.get_mut(target) {
                    let dist = t.translation.distance(target_t.translation());
                    
                    if dist > 2.5 {
                        steer.target = Some(target_t.translation());
                        t.look_at(target_t.translation(), Vec3::Y);
                    } else {
                        // Pickup
                        if drill_comp.storage > 0 {
                            drill_comp.storage -= 1;
                            worker.carrying = true;
                            worker.target_drill = None;
                            steer.target = None;
                            
                            // Visual attachment
                            let vis = commands.spawn((
                                Mesh3d(cube.clone()), MeshMaterial3d(scrap_mat.clone()),
                                Transform::from_xyz(0.0, 0.8, 0.5), CarryingVisual
                            )).id();
                            commands.entity(w_entity).add_child(vis);
                        } else {
                            // Drill empty before arrival
                            worker.target_drill = None; 
                        }
                    }
                } else {
                    worker.target_drill = None; // Drill destroyed
                }
            } else {
                steer.target = None;
            }

        } else {
            // DELIVER SCRAP
            if worker.target_storage.is_none() {
                let mut best_store = None;
                let mut min_dist = 9999.0;
                for (s_e, s_t) in storages.iter() {
                    let dist = t.translation.distance(s_t.translation());
                    if dist < min_dist { min_dist = dist; best_store = Some(s_e); }
                }
                worker.target_storage = best_store;
            }

            if let Some(target) = worker.target_storage {
                if let Ok((_, target_t)) = storages.get(target) {
                    let dist = t.translation.distance(target_t.translation());

                    if dist > 3.0 {
                        steer.target = Some(target_t.translation());
                        t.look_at(target_t.translation(), Vec3::Y);
                    } else {
                        // Drop off
                        worker.carrying = false;
                        worker.target_storage = None;
                        steer.target = None;
                        if stats.scrap < stats.max_scrap {
                            stats.scrap += 15;
                        }
                        commands.entity(w_entity).despawn_descendants(); // Remove visual
                    }
                } else {
                    worker.target_storage = None;
                }
            } else {
                // No storage? Stop.
                steer.target = None;
            }
        }
    }
}

fn drill_production(time: Res<Time>, mut drills: Query<&mut Drill>) {
    for mut d in drills.iter_mut() {
        if d.storage < DRILL_CAPACITY {
            d.timer.tick(time.delta());
            if d.timer.just_finished() {
                d.storage += 1;
            }
        }
    }
}

// --- UNIT LOGIC ---

fn unit_spawner_system(
    time: Res<Time>,
    mut barracks: Query<(&mut Barracks, &GlobalTransform)>,
    mut stats: ResMut<PlayerStats>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let drone_mesh = meshes.add(Sphere::new(0.3));
    let drone_mat = materials.add(StandardMaterial { emissive: LinearRgba::new(0.0, 5.0, 1.0, 3.0), ..default() });
    let soldier_mesh = meshes.add(Capsule3d::new(0.3, 0.6));
    let soldier_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.0, 0.2, 0.8), ..default() });

    for (mut b, t) in barracks.iter_mut() {
        b.timer.tick(time.delta());
        if b.timer.just_finished() && stats.unit_count < stats.unit_cap {
            stats.unit_count += 1;
            let spawn_pos = t.translation() + Vec3::new(0.0, 2.0, 2.0);
            
            if b.spawn_drone_next {
                commands.spawn((
                    Mesh3d(drone_mesh.clone()), MeshMaterial3d(drone_mat.clone()),
                    Transform::from_translation(spawn_pos + Vec3::Y * 4.0),
                    Unit { is_flying: true, patrol_offset: Vec3::ZERO, patrol_timer: Timer::from_seconds(1.0, TimerMode::Once) },
                    HomeBase { pos: t.translation() + Vec3::Y * 6.0 },
                    Health { current: 30.0, max: 30.0 },
                    RigidBody::Dynamic, Collider::ball(0.3), GravityScale(0.0), Damping { linear_damping: 2.0, angular_damping: 1.0 },
                    Velocity::default(),
                    Steer { speed: 25.0, ..default() },
                    Bob { speed: 4.0, amount: 0.3, base_y: spawn_pos.y + 4.0, offset: rand::random::<f32>() * PI },
                ));
            } else {
                commands.spawn((
                    Mesh3d(soldier_mesh.clone()), MeshMaterial3d(soldier_mat.clone()),
                    Transform::from_translation(spawn_pos),
                    Unit { is_flying: false, patrol_offset: Vec3::ZERO, patrol_timer: Timer::from_seconds(1.0, TimerMode::Once) },
                    HomeBase { pos: t.translation() },
                    Health { current: 80.0, max: 80.0 },
                    RigidBody::Dynamic, Collider::capsule_y(0.3, 0.3), LockedAxes::ROTATION_LOCKED,
                    Velocity::default(),
                    Steer { speed: 22.0, ..default() },
                    Bob { speed: 6.0, amount: 0.1, base_y: 0.6, offset: rand::random::<f32>() * PI },
                ));
            }
            b.spawn_drone_next = !b.spawn_drone_next;
        }
    }
}

fn unit_idle_behavior(
    time: Res<Time>,
    mut units: Query<(&mut Unit, &HomeBase)>,
) {
    let mut rng = rand::thread_rng();
    for (mut u, home) in units.iter_mut() {
        u.patrol_timer.tick(time.delta());
        if u.patrol_timer.finished() {
            u.patrol_timer = Timer::from_seconds(3.0 + rng.r#gen::<f32>() * 3.0, TimerMode::Once);
            // Pick a random spot near home
            let offset_x = (rng.r#gen::<f32>() - 0.5) * 10.0;
            let offset_z = (rng.r#gen::<f32>() - 0.5) * 10.0;
            u.patrol_offset = home.pos + Vec3::new(offset_x, 0.0, offset_z);
        }
    }
}

fn unit_combat_ai(
    mut commands: Commands,
    time: Res<Time>,
    mut units: Query<(&mut Transform, &mut Steer, &Unit, &HomeBase)>,
    enemies: Query<(Entity, &GlobalTransform), With<Enemy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shoot_timer: Local<f32>,
) {
    *shoot_timer += time.delta_secs();
    
    // Cache projectile assets
    let proj_mesh = meshes.add(Sphere::new(0.1));
    let proj_mat = materials.add(StandardMaterial { emissive: LinearRgba::new(0.0, 10.0, 10.0, 5.0), ..default() });

    for (mut t, mut steer, unit, _) in units.iter_mut() {
        let mut target = None;
        let mut closest_dist = 60.0; 

        for (e_e, e_t) in enemies.iter() {
            let d = t.translation.distance(e_t.translation());
            if d < closest_dist {
                closest_dist = d;
                target = Some((e_e, e_t.translation()));
            }
        }

        if let Some((_, target_pos)) = target {
            let desired_dist = if unit.is_flying { 15.0 } else { 1.5 }; 
            
            if closest_dist > desired_dist {
                steer.target = Some(target_pos);
                t.look_at(target_pos, Vec3::Y);
            } else {
                steer.target = None;
                // Shoot logic for drones
                if unit.is_flying {
                    t.look_at(target_pos, Vec3::Y);
                    if *shoot_timer > 0.1 && rand::random::<f32>() < 0.02 { 
                        let dir = (target_pos - t.translation).normalize();
                         commands.spawn((
                            Mesh3d(proj_mesh.clone()), MeshMaterial3d(proj_mat.clone()),
                            Transform::from_translation(t.translation + *t.forward()),
                            Projectile { damage: 10.0, lifetime: Timer::from_seconds(1.0, TimerMode::Once), from_player: true },
                            RigidBody::Dynamic, Collider::ball(0.1), Sensor, GravityScale(0.0),
                            Velocity { linvel: dir * 60.0, angvel: Vec3::ZERO },
                        ));
                    }
                }
            }
        } else {
            // Patrol / Return Home
            let dest = unit.patrol_offset;
            if t.translation.distance(dest) > 1.0 {
                steer.target = Some(dest);
                t.look_at(dest, Vec3::Y);
             } else {
                steer.target = None;
             }
        }
    }
}

// --- GAMEPLAY SYSTEMS ---

fn wow_movement_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut q_player: Query<(&mut Velocity, &mut Transform, &mut Player)>,
    q_cam: Query<&WowCameraRig>,
    rapier: Single<&RapierContext>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((mut velocity, mut transform, mut player)) = q_player.get_single_mut() else { return };
    let Ok(rig) = q_cam.get_single() else { return };

    let dt = time.delta_secs();
    player.dash_cooldown = (player.dash_cooldown - dt).max(0.0);
    player.dash_timer = (player.dash_timer - dt).max(0.0);

    // Ground Check
    let on_ground = if let Some((_, dist)) = rapier.cast_ray(transform.translation, Vec3::NEG_Y, 1.2, true, QueryFilter::exclude_dynamic()) {
        dist < 1.1
    } else { false };

    if on_ground {
        player.jump_count = 0;
        player.is_somersaulting = false;
        transform.rotation = Quat::from_rotation_y(transform.rotation.to_euler(EulerRot::YXZ).0);
    }

    // Dash
    if keys.just_pressed(KeyCode::ShiftLeft) && player.dash_cooldown <= 0.0 {
        player.dash_timer = 0.25;
        player.dash_cooldown = 1.0;
        let dash_dir = transform.forward().normalize();
        velocity.linvel += dash_dir * 100.0;
        spawn_dust_burst(&mut commands, &mut meshes, &mut materials, transform.translation, Color::WHITE);
    }

    // Jump
    if keys.just_pressed(KeyCode::Space) && player.jump_count < 2 {
        player.jump_count += 1;
        velocity.linvel.y = 18.0;
        if player.jump_count == 2 {
            player.is_somersaulting = true;
            spawn_dust_burst(&mut commands, &mut meshes, &mut materials, transform.translation, Color::srgb(0.5, 0.8, 1.0));
        }
    }

    if player.is_somersaulting {
        transform.rotate_local_x(dt * 15.0);
    }

    let right_click_held = mouse_btn.pressed(MouseButton::Right);
    let mut move_input = Vec3::ZERO;

    if keys.pressed(KeyCode::KeyW) { move_input.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { move_input.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { move_input.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { move_input.x += 1.0; }

    if move_input.length_squared() > 0.0 {
        move_input = move_input.normalize();
        let cam_rot = Quat::from_rotation_y(rig.yaw);
        let move_dir = cam_rot * move_input;

        let speed = if player.dash_timer > 0.0 { PLAYER_SPEED * 2.0 } else { PLAYER_SPEED };
        velocity.linvel.x = move_dir.x * speed;
        velocity.linvel.z = move_dir.z * speed;

        if !right_click_held {
            let target_angle = move_dir.x.atan2(move_dir.z) + PI;
            let target_rot = Quat::from_rotation_y(target_angle);
            transform.rotation = transform.rotation.slerp(target_rot, dt * 10.0);
        } 
        
        // Footstep dust
        if on_ground && time.elapsed_secs() % 0.2 < 0.02 {
            spawn_dust(&mut commands, &mut meshes, &mut materials, transform.translation - Vec3::Y * 0.5, Color::srgba(0.5, 0.5, 0.5, 0.5));
        }
    } else {
        velocity.linvel.x = velocity.linvel.x.lerp(0.0, dt * 10.0);
        velocity.linvel.z = velocity.linvel.z.lerp(0.0, dt * 10.0);
    }
}

fn player_bounce_system(
    mut q_player: Query<(&Transform, &mut Velocity), With<Player>>,
    q_targets: Query<(&GlobalTransform, Entity), (Or<(With<Enemy>, With<Structure>)>, Without<Player>)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((t, mut v)) = q_player.get_single_mut() else { return };
    if v.linvel.y > 0.0 { return; } // Only bounce while falling

    for (gt, _e) in q_targets.iter() {
        let dist = t.translation.distance(gt.translation());
        if dist < 2.5 && t.translation.y > gt.translation().y + 0.5 {
            v.linvel.y = 25.0; // Mega bounce
            spawn_dust_burst(&mut commands, &mut meshes, &mut materials, t.translation - Vec3::Y * 1.0, Color::srgb(1.0, 1.0, 0.0));
            break;
        }
    }
}

fn weapon_mechanics(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut player_query: Query<(&Transform, &mut Player)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    keys: Res<ButtonInput<KeyCode>>,
    sel: Res<SelectionState>,
) {
    if keys.pressed(KeyCode::AltLeft) || sel.is_selecting { return; }

    if let Ok((t, mut p)) = player_query.get_single_mut() {
        p.fire_timer -= time.delta_secs();
        if mouse.pressed(MouseButton::Left) && p.fire_timer <= 0.0 {
            p.fire_timer = 0.1;
            let spawn_pos = t.translation + *t.forward() * 0.5 + Vec3::new(0.0, 1.0, 0.0);
            commands.spawn((
                PointLight { color: Color::srgb(1.0, 0.65, 0.0), intensity: 2000.0, range: 5.0, ..default() },
                Transform::from_translation(spawn_pos),
                MuzzleFlash { timer: Timer::from_seconds(0.05, TimerMode::Once) }
            ));
            commands.spawn((
                Mesh3d(meshes.add(Sphere::new(0.15))),
                MeshMaterial3d(materials.add(StandardMaterial { 
                    base_color: Color::srgb(1.0, 1.0, 0.0), 
                    emissive: LinearRgba::new(5.0, 5.0, 0.0, 5.0), 
                    ..default() 
                })),
                Transform::from_translation(spawn_pos),
                Projectile { damage: 35.0, lifetime: Timer::from_seconds(1.0, TimerMode::Once), from_player: true },
                RigidBody::Dynamic, Collider::ball(0.15), Sensor,
                Velocity { linvel: *t.forward() * 200.0, angvel: Vec3::ZERO },
            ));
        }
    }
}

fn manual_repair(keys: Res<ButtonInput<KeyCode>>, cursor: Res<WorldCursor>, mut structures: Query<(&GlobalTransform, &mut Health), With<Structure>>) {
    if keys.pressed(KeyCode::KeyE) {
        for (t, mut hp) in structures.iter_mut() {
            if t.translation().distance(cursor.pos) < 6.0 && hp.current < hp.max {
                hp.current += 1.0; 
            }
        }
    }
}

fn enemy_spawner(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut commands: Commands,
    phase: Res<PhaseManager>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<&Transform, With<Player>>,
) {
    if !phase.is_combat { return; }
    
    let spawn_delay = (1.5 - (phase.wave as f32 * 0.05)).max(0.3);
    *timer += time.delta_secs();
    
    if *timer > spawn_delay {
        *timer = 0.0;
        if let Ok(p_t) = player_q.get_single() {
            let angle = rand::random::<f32>() * PI * 2.0;
            let pos = p_t.translation + Vec3::new(angle.cos() * 80.0, 2.0, angle.sin() * 80.0);
            
            let mut rng = rand::thread_rng();
            let is_giant = rng.r#gen_bool(0.15); // 15% chance for giants
            
            let (hp, scale, color) = if is_giant {
                (250.0 * 1.2f32.powi(phase.wave as i32), 3.0, Color::srgb(0.5, 0.0, 1.0))
            } else {
                (50.0 * 1.15f32.powi(phase.wave as i32), 1.0, Color::srgb(1.0, 0.2, 0.2))
            };
            
            commands.spawn((
                Mesh3d(meshes.add(Capsule3d::new(0.4, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
                Enemy { is_giant },
                Health { current: hp, max: hp },
                RigidBody::Dynamic, Collider::capsule_y(0.5, 0.4), LockedAxes::ROTATION_LOCKED,
                Velocity::default(),
                Steer { speed: 15.0, ..default() },
                Bob { speed: 3.0 + rng.r#gen::<f32>() * 2.0, amount: 0.2 * scale, base_y: 2.0, offset: rng.r#gen::<f32>() * PI },
            ));
        }
    }
}

fn enemy_ai(mut enemies: Query<(&mut Steer, &mut Transform), With<Enemy>>, player: Query<&Transform, (With<Player>, Without<Enemy>)>, structures: Query<&GlobalTransform, With<Structure>>) {
    let Ok(p_t) = player.get_single() else { return };
    for (mut steer, mut t) in enemies.iter_mut() {
        let mut target = p_t.translation;
        let mut min_dist = t.translation.distance(target);
        
        for s in structures.iter() {
            let d = t.translation.distance(s.translation());
            if d < 20.0 && d < min_dist { min_dist = d; target = s.translation(); }
        }
        
        steer.target = Some(target);
        let target_y = t.translation.y;
        t.look_at(Vec3::new(target.x, target_y, target.z), Vec3::Y);
    }
}

fn steering_system(
    mut q_steer: Query<(Entity, &mut Velocity, &mut Steer, &Transform)>,
    q_neighbors: Query<(Entity, &Transform), With<Velocity>>,
    q_obstacles: Query<&GlobalTransform, With<Structure>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    for (e1, mut v, steer, t1) in q_steer.iter_mut() {
        let mut steer_acc = Vec3::ZERO;
        
        if let Some(target) = steer.target {
            // 1. SEEK
            let desired = (target - t1.translation).normalize_or_zero() * steer.speed;
            steer_acc += (desired - v.linvel) * 2.0;
        } else {
            // BRAKE
            steer_acc -= v.linvel * 5.0;
        }

        // 2. SEPARATION
        let mut sep_acc = Vec3::ZERO;
        for (e2, t2) in q_neighbors.iter() {
            if e1 == e2 { continue; }
            let dist = t1.translation.distance(t2.translation);
            if dist < 3.0 && dist > 0.0 {
                sep_acc += (t1.translation - t2.translation).normalize() / dist;
            }
        }
        steer_acc += sep_acc * 20.0;

        // 3. OBSTACLE AVOIDANCE (Simple)
        let mut avoid_acc = Vec3::ZERO;
        for obs in q_obstacles.iter() {
            let dist = t1.translation.distance(obs.translation());
            if dist < 6.0 && dist > 0.0 {
                avoid_acc += (t1.translation - obs.translation()).normalize() / dist;
            }
        }
        steer_acc += avoid_acc * 30.0;

        // Apply
        let y_vel = v.linvel.y;
        v.linvel += steer_acc * dt;
        v.linvel.y = y_vel; 
        
        // Clamp horizontal
        let mut horiz = Vec3::new(v.linvel.x, 0.0, v.linvel.z);
        if horiz.length() > steer.speed {
            horiz = horiz.normalize() * steer.speed;
            v.linvel.x = horiz.x;
            v.linvel.z = horiz.z;
        }
    }
}

fn turret_ai(
    time: Res<Time>, 
    mut turrets: Query<(&GlobalTransform, &mut Turret)>, 
    enemies: Query<&GlobalTransform, With<Enemy>>, 
    mut commands: Commands, 
    mut meshes: ResMut<Assets<Mesh>>, 
    mut materials: ResMut<Assets<StandardMaterial>>
) {
    let proj_mesh = meshes.add(Cuboid::new(0.1, 0.1, 0.5));
    let proj_mat = materials.add(StandardMaterial { emissive: LinearRgba::new(1.0, 0.5, 0.0, 5.0), ..default() });

    for (t, mut tur) in turrets.iter_mut() {
        tur.cooldown -= time.delta_secs();
        if tur.cooldown <= 0.0 {
            for e_t in enemies.iter() {
                if t.translation().distance(e_t.translation()) < 40.0 {
                    tur.cooldown = 0.4;
                    let dir = (e_t.translation() - t.translation()).normalize();
                    commands.spawn((
                        Mesh3d(proj_mesh.clone()), MeshMaterial3d(proj_mat.clone()),
                        Transform::from_translation(t.translation() + Vec3::Y * 2.0).looking_at(e_t.translation(), Vec3::Y),
                        Projectile { damage: 20.0, lifetime: Timer::from_seconds(2.0, TimerMode::Once), from_player: true },
                        RigidBody::Dynamic, Collider::ball(0.1), Sensor, Velocity { linvel: dir * 80.0, angvel: Vec3::ZERO },
                    ));
                    break;
                }
            }
        }
    }
}

fn projectile_logic(
    mut commands: Commands, 
    time: Res<Time>, 
    mut projs: Query<(Entity, &mut Projectile, &mut Transform)>, 
    mut enemies: Query<(Entity, &GlobalTransform, &mut Health), With<Enemy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (pe, mut p, pt) in projs.iter_mut() {
        p.lifetime.tick(time.delta());
        if p.lifetime.finished() { commands.entity(pe).despawn(); continue; }
        
        // Trail
        if time.elapsed_secs() % 0.05 < 0.02 {
            spawn_dust(&mut commands, &mut meshes, &mut materials, pt.translation, Color::srgba(0.0, 1.0, 1.0, 0.3));
        }

        for (ee, et, mut hp) in enemies.iter_mut() {
            if pt.translation.distance(et.translation()) < 2.0 {
                hp.current -= p.damage;
                spawn_dust_burst(&mut commands, &mut meshes, &mut materials, pt.translation, Color::srgb(1.0, 0.5, 0.0));
                commands.entity(pe).despawn();
                if hp.current <= 0.0 { commands.entity(ee).despawn_recursive(); }
                break;
            }
        }
    }
}

fn check_game_over(mut next_state: ResMut<NextState<GameState>>, player_q: Query<&Health, With<Player>>) {
    if let Ok(hp) = player_q.get_single() {
        if hp.current <= 0.0 { next_state.set(GameState::GameOver); }
    }
}

fn restart_game_system(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
    enemies: Query<Entity, With<Enemy>>,
    structures: Query<Entity, With<Structure>>,
    units: Query<Entity, With<Unit>>,
    workers: Query<Entity, With<Worker>>,
    mut player_q: Query<(&mut Health, &mut Transform), With<Player>>,
    mut phase: ResMut<PhaseManager>,
    mut stats: ResMut<PlayerStats>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        if let Ok((mut hp, mut t)) = player_q.get_single_mut() { 
            hp.current = hp.max; 
            t.translation = Vec3::new(0.0, 5.0, 0.0);
        }
        stats.scrap = 600;
        stats.unit_count = 0;
        stats.unit_cap = 5;
        phase.wave = 1;
        phase.is_combat = false;
        phase.timer = Timer::from_seconds(60.0, TimerMode::Once);

        for e in enemies.iter() { commands.entity(e).despawn_recursive(); }
        for e in structures.iter() { commands.entity(e).despawn_recursive(); }
        for e in units.iter() { commands.entity(e).despawn_recursive(); }
        for e in workers.iter() { commands.entity(e).despawn_recursive(); }

        next_state.set(GameState::Playing);
    }
}

fn phase_logic(
    time: Res<Time>, 
    mut pm: ResMut<PhaseManager>, 
    mut lights: Query<&mut DirectionalLight>, 
    mut fog: Query<&mut DistanceFog>,
    mut ambient: ResMut<AmbientLight>,
) {
    pm.timer.tick(time.delta());
    if pm.timer.finished() {
        pm.is_combat = !pm.is_combat;
        pm.timer = Timer::from_seconds(if pm.is_combat { 45.0 } else { 60.0 }, TimerMode::Once);
        if !pm.is_combat { pm.wave += 1; }
        
        let (l_col, f_col, a_col) = if pm.is_combat {
            // COMBAT: Vibrant Red/Purple, high contrast
            (Color::srgb(2.0, 0.5, 0.5), Color::srgb(0.1, 0.0, 0.1), Color::srgb(0.2, 0.05, 0.1)) 
        } else {
            // BUILD: Cool Cyan/Blue
            (Color::srgb(1.0, 1.0, 1.5), Color::srgb(0.01, 0.01, 0.05), Color::srgb(0.1, 0.1, 0.2))
        };

        if let Ok(mut l) = lights.get_single_mut() { l.color = l_col; }
        if let Ok(mut f) = fog.get_single_mut() { f.color = f_col; }
        ambient.color = a_col;
    }
}

fn update_hud(
    pm: Res<PhaseManager>, mgr: Res<BuildManager>, stats: Res<PlayerStats>, mut txt: Query<&mut Text, With<HudText>>, state: Res<State<GameState>>
) {
    if let Ok(mut t) = txt.get_single_mut() {
        if *state.get() == GameState::GameOver {
            t.0 = "GAME OVER\nPRESS [R] TO RESTART".to_string(); return;
        }
        let phase = if pm.is_combat { "COMBAT" } else { "BUILD" };
        let tool = match mgr.tool { 
            BuildTool::Drill => "Drill ($50)", BuildTool::Turret => "Turret ($80)", BuildTool::Wall => "Wall ($10)", 
            BuildTool::Barracks => "Barracks ($200)", BuildTool::BuilderHut => "Hut ($150)", BuildTool::Storage => "Storage ($100)" 
        };
        t.0 = format!(
            "{} - {:.0}s | Wave {}\nScrap: {}/{}\nUnits: {}/{}\nTool: {} [1-6]\n[Alt+Drag] Select & Delete",
            phase, pm.timer.remaining_secs(), pm.wave, stats.scrap, stats.max_scrap, stats.unit_count, stats.unit_cap, tool
        );
    }
}

fn muzzle_flash_logic(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut MuzzleFlash)>) {
    for (e, mut f) in q.iter_mut() {
        f.timer.tick(time.delta());
        if f.timer.finished() { commands.entity(e).despawn(); }
    }
}

// --- PARTICLE HELPERS ---

fn particle_system(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    for (e, mut p, mut t) in q.iter_mut() {
        p.lifetime.tick(time.delta());
        if p.lifetime.finished() {
            commands.entity(e).despawn_recursive();
            continue;
        }

        t.translation += p.velocity * time.delta_secs();
        
        if p.fade {
            let pct = p.lifetime.fraction_remaining();
            t.scale = Vec3::splat(p.initial_scale * pct);
        }
    }
}

fn spawn_dust(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, pos: Vec3, color: Color) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::from_translation(pos).with_scale(Vec3::splat(0.2)),
        Particle {
            velocity: Vec3::new(0.0, 1.0, 0.0),
            lifetime: Timer::from_seconds(0.5, TimerMode::Once),
            fade: true,
            initial_scale: 0.2,
        },
        NotShadowCaster,
    ));
}

fn spawn_dust_burst(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, pos: Vec3, color: Color) {
    let mut rng = rand::thread_rng();
    for _ in 0..10 {
        let vel = Vec3::new(rng.r#gen_range(-5.0..5.0), rng.r#gen_range(2.0..10.0), rng.r#gen_range(-5.0..5.0));
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::from_translation(pos).with_scale(Vec3::splat(0.4)),
            Particle {
                velocity: vel,
                lifetime: Timer::from_seconds(0.8, TimerMode::Once),
                fade: true,
                initial_scale: 0.4,
            },
            NotShadowCaster,
        ));
    }
}