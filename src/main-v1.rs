use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::render::camera::Viewport;
use bevy_rapier3d::prelude::*;
use std::f32::consts::PI;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::bloom::Bloom;
use std::time::Instant;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use rand::prelude::*;

// --- TUNING CONSTANTS ---

// Player & Movement
const PLAYER_SPEED: f32 = 45.0; 
const GRAVITY_SCALE: f32 = 6.0;

// Camera Settings (Stabilized)
const CAM_MIN_DIST: f32 = 10.0;  
const CAM_MAX_DIST: f32 = 120.0; 
const CAM_POS_DEADZONE: f32 = 4.0;   // Player moves 4 units before cam follows
const CAM_POS_LERP: f32 = 4.0;       // Smooth follow speed
const CAM_ROT_LERP: f32 = 6.0;       // Smooth rotation speed
const CAM_ALIGN_DELAY: f32 = 2.0;    // Time before auto-align starts
const CAM_WALL_BUFFER: f32 = 0.5;

// -- FIXED CONSTANTS --
const CAM_SMOOTH_POS: f32 = 4.0;       // Was CAM_POS_LERP
const CAM_SMOOTH_ROT: f32 = 6.0;       // Was CAM_ROT_LERP
const CAM_AUTO_ALIGN_DELAY: f32 = 2.0; // Was CAM_ALIGN_DELAY
const CAM_AUTO_ALIGN_SPEED: f32 = 2.0; // Was Missing

// Game Loop
const BUILD_PHASE_DURATION: f32 = 60.0;
const COMBAT_PHASE_DURATION: f32 = 45.0;

// Economy & Buildings
const STARTING_SCRAP: u32 = 250;
const FARM_COST: u32 = 20;
const TURRET_COST: u32 = 60;
const WALL_COST: u32 = 10;
const FARM_YIELD: u32 = 25;
const FARM_GROWTH_TIME: f32 = 5.0;

// Tech Stats
const TURRET_RANGE: f32 = 40.0;
const TURRET_COOLDOWN: f32 = 0.5;
const DRONE_RANGE: f32 = 50.0;
const DRONE_COOLDOWN: f32 = 0.25;

// Combat
const BULLET_SPEED: f32 = 200.0;
const FIRE_RATE: f32 = 0.08; 
const MELEE_COOLDOWN: f32 = 0.4;
const MELEE_DAMAGE: f32 = 150.0;
const BULLET_DAMAGE: f32 = 35.0;

// AI
const ZOMBIE_SPEED: f32 = 20.0;
const GIANT_SPEED: f32 = 12.0;
const AGENT_SPEED: f32 = 32.0;
const AGENT_AGGRO_RANGE: f32 = 25.0;

// Map
const CITY_SIZE: i32 = 500;
const PARK_RADIUS: f32 = 80.0;

// --- RESOURCES ---

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum CameraMode {
    #[default]
    ThirdPerson, 
    Isometric,   
}

#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum GamePhase {
    #[default]
    Build,
    Combat,
}

#[derive(Resource)]
struct PhaseManager {
    timer: Timer,
    current: GamePhase,
    wave_number: u32,
}

#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum BuildTool {
    #[default]
    FarmPlot,
    Turret,
    Wall,
}

#[derive(Resource)]
struct BuildManager {
    selected_tool: BuildTool,
}

#[derive(Resource)]
struct WorldCursorState {
    target_pos: Vec3,
}

impl Default for WorldCursorState {
    fn default() -> Self { Self { target_pos: Vec3::ZERO } }
}

#[derive(Resource, Default)]
struct SelectionState {
    is_dragging: bool,
    start_pos: Vec3,
    current_pos: Vec3,
}

#[derive(Resource, Default)]
struct PlayerStats {
    scrap: u32,
}

#[derive(Resource)]
struct PerformanceMetrics { fps: f64, last_update: Instant }
impl Default for PerformanceMetrics { fn default() -> Self { Self { fps: 60.0, last_update: Instant::now() } } }

// --- COMPONENTS ---

#[derive(Component)]
struct Player {
    fire_timer: f32,
    melee_timer: f32,
}

#[derive(Component)]
struct Health {
    current: f32,
    max: f32,
}

// The "Juice" Component
#[derive(Component)]
struct Boing {
    base_scale: Vec3,
    intensity: f32,
}
impl Default for Boing {
    fn default() -> Self { Self { base_scale: Vec3::ONE, intensity: 1.0 } }
}

#[derive(Component)]
struct Drone {
    cooldown: f32,
    hover_offset: Vec3,
}

#[derive(Component)]
struct BotAgent {
    attack_cooldown: f32,
}

#[derive(Component)]
struct Structure; 

#[derive(Component)]
struct Turret { cooldown: f32 }

#[derive(Component)]
enum FarmState { Growing, Ripe }

#[derive(Component)]
struct Farm { 
    state: FarmState,
    timer: f32, 
}

#[derive(Component)]
struct FaceCamera; 

#[derive(Component)]
struct SmartCameraRig {
    yaw: f32,
    pitch: f32,
    target_yaw: f32,
    target_pitch: f32,
    current_dist: f32,
    target_dist: f32,
    time_since_input: f32,
    focus_point: Vec3, 
}

impl Default for SmartCameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: PI / 5.0,
            target_yaw: 0.0,
            target_pitch: PI / 5.0,
            current_dist: 50.0,
            target_dist: 50.0,
            time_since_input: 0.0,
            focus_point: Vec3::ZERO,
        }
    }
}

#[derive(Component)]
struct Enemy {
    is_giant: bool,
}

#[derive(Component)]
struct Projectile {
    damage: f32,
    lifetime: Timer,
    from_player: bool,
}

#[derive(Component)]
struct MuzzleFlash { timer: Timer }

#[derive(Component)]
struct SlashEffect { timer: Timer }

#[derive(Component)]
struct WorldCursorIndicator;

#[derive(Component)]
struct HudText;

// --- MAIN ---

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "CITY SWARM: JUICED DEFENSE".into(),
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            RapierPhysicsPlugin::<NoUserData>::default(),
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default(),
        ))
        .init_state::<CameraMode>()
        .init_resource::<WorldCursorState>()
        .init_resource::<SelectionState>()
        .init_resource::<PerformanceMetrics>()
        .insert_resource(PlayerStats { scrap: STARTING_SCRAP })
        .insert_resource(PhaseManager {
            timer: Timer::from_seconds(BUILD_PHASE_DURATION, TimerMode::Once),
            current: GamePhase::Build,
            wave_number: 1,
        })
        .insert_resource(BuildManager { selected_tool: BuildTool::FarmPlot })
        .insert_resource(ClearColor(Color::srgb(0.01, 0.01, 0.02)))
        .insert_resource(AmbientLight {
            color: Color::srgb(0.05, 0.05, 0.1),
            brightness: 0.2,
        })
        .add_systems(Startup, (setup_scene, setup_player, setup_drone, setup_ui, setup_cursor, setup_face_cam))
        // Logic
        .add_systems(Update, (
            phase_cycle_system,
            toggle_camera_mode,
            unified_cursor_system,
            tool_selector_system,
            selection_system, 
            smart_camera_controls, 
            player_movement,
            player_look_system,
            weapon_mechanics,
            melee_mechanics,
            manual_interact,
            resize_face_cam,
            face_cam_follow,
        ))
        // AI & Visuals
        .add_systems(Update, (
            drone_ai,
            turret_ai,
            farm_logic,
            agent_ai,
            enemy_ai_swarm,
            enemy_spawner,
            projectile_logic,
            muzzle_flash_logic,
            slash_effect_logic,
            boing_animation_system, // The Juice!
            update_hud, 
            update_performance_metrics,
            draw_selection_gizmos,
        ))
        .run();
}

// --- SETUP ---

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Main Camera
    commands.spawn((
        Camera3d::default(),
        Camera { hdr: true, ..default() },
        Tonemapping::TonyMcMapface,
        Bloom::default(),
        SmartCameraRig {
            target_dist: 60.0,
            current_dist: 60.0,
            pitch: PI / 4.0,
            target_pitch: PI / 4.0,
            ..default()
        },
        Transform::from_xyz(0.0, 20.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y), 
    ));

    // Sun
    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            shadows_enabled: true,
            color: Color::srgb(0.6, 0.8, 1.0),
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -PI / 3.0, PI / 4.0, 0.0)),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.05, 0.08),
            perceptual_roughness: 0.8,
            ..default()
        })),
        RigidBody::Fixed,
        Collider::cuboid(1000.0, 0.1, 1000.0),
    ));

    // City Grid
    let building_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.15),
        perceptual_roughness: 0.2,
        reflectance: 0.5,
        ..default()
    });
    
    let neon_blue = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::new(0.0, 2.0, 5.0, 2.0), ..default() });
    let neon_pink = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::new(5.0, 0.0, 2.0, 2.0), ..default() });

    let mut rng = rand::thread_rng();
    
    for x in (-CITY_SIZE..CITY_SIZE).step_by(60) {
        for z in (-CITY_SIZE..CITY_SIZE).step_by(60) {
            let pos = Vec3::new(x as f32, 0.0, z as f32);
            let dist = pos.length();

            if dist < PARK_RADIUS { continue; }

            let h = rng.gen_range(15.0..45.0);
            
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(20.0, h, 20.0))),
                MeshMaterial3d(building_mat.clone()),
                Transform::from_xyz(pos.x, h / 2.0, pos.z),
                RigidBody::Fixed,
                Collider::cuboid(10.0, h / 2.0, 10.0),
            ));

            if rng.gen_bool(0.3) {
                let mat = if rng.gen_bool(0.5) { neon_blue.clone() } else { neon_pink.clone() };
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(21.0, 1.0, 21.0))),
                    MeshMaterial3d(mat),
                    Transform::from_xyz(pos.x, rng.gen_range(5.0..h), pos.z),
                ));
            }
        }
    }

    // Pre-Created Farms
    let farm_mesh = meshes.add(Cuboid::new(3.8, 0.5, 3.8));
    let farm_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.4, 0.25, 0.1), ..default() });
    
    let offsets = [Vec3::new(15.,0.,15.), Vec3::new(-15.,0.,15.), Vec3::new(15.,0.,-15.), Vec3::new(-15.,0.,-15.)];
    for off in offsets {
        commands.spawn((
            Mesh3d(farm_mesh.clone()),
            MeshMaterial3d(farm_mat.clone()),
            Transform::from_translation(off + Vec3::Y * 0.25),
            Structure,
            Health { current: 100.0, max: 100.0 },
            Farm { state: FarmState::Growing, timer: 0.0 },
            RigidBody::Fixed,
            Collider::cuboid(1.9, 0.25, 1.9),
        ));
    }
}

fn setup_player(
    mut commands: Commands, 
    mut meshes: ResMut<Assets<Mesh>>, 
    mut materials: ResMut<Assets<StandardMaterial>>
) {
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.4, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 0.8, 1.0),
            emissive: LinearRgba::new(0.0, 0.4, 0.5, 1.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 5.0, 0.0),
        Player { fire_timer: 0.0, melee_timer: 0.0 },
        Health { current: 500.0, max: 500.0 },
        Boing { base_scale: Vec3::ONE, intensity: 1.0 },
        RigidBody::Dynamic,
        Collider::capsule_y(0.5, 0.4), 
        LockedAxes::ROTATION_LOCKED,
        Velocity::default(),
        Damping { linear_damping: 0.0, angular_damping: 1.0 }, 
        GravityScale(GRAVITY_SCALE),
        Ccd::enabled(),
    )).with_children(|parent| {
        parent.spawn((
            PointLight {
                color: Color::srgb(0.0, 1.0, 1.0), 
                intensity: 1500.0,
                range: 15.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_xyz(0.0, 2.5, 0.0),
        ));
    });

    // Agents
    let bot_mesh = meshes.add(Capsule3d::new(0.3, 0.8));
    let bot_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.0, 0.0, 1.0), // Blue
        emissive: LinearRgba::new(0.0, 0.0, 1.0, 2.0),
        ..default()
    });

    for i in 0..6 {
        commands.spawn((
            Mesh3d(bot_mesh.clone()),
            MeshMaterial3d(bot_mat.clone()),
            Transform::from_xyz(5.0 + i as f32 * 2.0, 2.0, 5.0),
            BotAgent { attack_cooldown: 0.0 },
            Boing { base_scale: Vec3::ONE, intensity: 0.8 },
            Health { current: 100.0, max: 100.0 },
            RigidBody::Dynamic,
            Collider::capsule_y(0.4, 0.3),
            LockedAxes::ROTATION_LOCKED,
            Velocity::default(),
            Damping { linear_damping: 1.0, angular_damping: 1.0 },
        ));
    }
}

fn setup_drone(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.25))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.5, 0.0),
            emissive: LinearRgba::new(2.0, 1.0, 0.0, 3.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 8.0, 0.0),
        Boing { base_scale: Vec3::splat(1.0), intensity: 0.5 },
        Drone { cooldown: 0.0, hover_offset: Vec3::new(1.5, 2.5, -1.0) },
    ));
}

fn setup_ui(mut commands: Commands) {
    let font_style = TextFont { font_size: 18.0, ..default() };
    
    commands.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::SpaceBetween,
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(20.0)),
        ..default()
    }).with_children(|root| {
        // TOP LEFT: Info & Instructions
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(15.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        )).with_children(|panel| {
            panel.spawn((
                Text::new("Loading Stats..."),
                font_style.clone(),
                TextColor(Color::WHITE),
                HudText,
            ));
        });
    });
}

fn setup_face_cam(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Camera { 
            order: 1, 
            clear_color: ClearColorConfig::None, 
            ..default() 
        },
        FaceCamera,
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

fn setup_cursor(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((
        Mesh3d(meshes.add(Torus::new(0.1, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.0, 1.0, 0.0), unlit: true, ..default() })),
        Transform::default(),
        WorldCursorIndicator,
    ));
}

// --- LOGIC ---

fn phase_cycle_system(
    time: Res<Time>,
    mut phase: ResMut<PhaseManager>,
    mut light_query: Query<&mut DirectionalLight>,
) {
    phase.timer.tick(time.delta());

    if phase.timer.finished() {
        match phase.current {
            GamePhase::Build => {
                println!("WARNING: WAVE INCOMING!");
                phase.current = GamePhase::Combat;
                phase.timer = Timer::from_seconds(COMBAT_PHASE_DURATION, TimerMode::Once);
                
                if let Ok(mut light) = light_query.get_single_mut() {
                    light.color = Color::srgb(1.0, 0.2, 0.2); 
                    light.illuminance = 800.0;
                }
            },
            GamePhase::Combat => {
                println!("WAVE CLEARED. REBUILDING...");
                phase.current = GamePhase::Build;
                phase.timer = Timer::from_seconds(BUILD_PHASE_DURATION, TimerMode::Once);
                phase.wave_number += 1;

                if let Ok(mut light) = light_query.get_single_mut() {
                    light.color = Color::srgb(0.6, 0.7, 1.0); 
                    light.illuminance = 3000.0;
                }
            }
        }
    }
}

fn tool_selector_system(
    input: Res<ButtonInput<KeyCode>>,
    mut manager: ResMut<BuildManager>,
) {
    if input.just_pressed(KeyCode::Digit1) { manager.selected_tool = BuildTool::FarmPlot; }
    if input.just_pressed(KeyCode::Digit2) { manager.selected_tool = BuildTool::Turret; }
    if input.just_pressed(KeyCode::Digit3) { manager.selected_tool = BuildTool::Wall; }
}

fn selection_system(
    mut selection: ResMut<SelectionState>,
    cursor: Res<WorldCursorState>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    build_manager: Res<BuildManager>,
    mut state: ResMut<PlayerStats>,
) {
    if !keys.pressed(KeyCode::AltLeft) { 
        selection.is_dragging = false; 
        return; 
    }

    if mouse.just_pressed(MouseButton::Left) {
        selection.is_dragging = true;
        selection.start_pos = cursor.target_pos;
    }

    if selection.is_dragging {
        selection.current_pos = cursor.target_pos;
        
        if mouse.just_released(MouseButton::Left) {
            selection.is_dragging = false;
            let start = selection.start_pos;
            let end = selection.current_pos;
            
            let min_x = start.x.min(end.x);
            let max_x = start.x.max(end.x);
            let min_z = start.z.min(end.z);
            let max_z = start.z.max(end.z);

            let (size, cost, color) = match build_manager.selected_tool {
                BuildTool::FarmPlot => (4.0, FARM_COST, Color::srgb(0.4, 0.25, 0.1)),
                BuildTool::Turret => (6.0, TURRET_COST, Color::srgb(0.2, 0.2, 0.2)),
                BuildTool::Wall => (2.0, WALL_COST, Color::srgb(0.5, 0.5, 0.5)),
            };

            let mesh = match build_manager.selected_tool {
                BuildTool::FarmPlot => meshes.add(Cuboid::new(3.8, 0.5, 3.8)),
                BuildTool::Turret => meshes.add(Cuboid::new(1.0, 3.0, 1.0)),
                BuildTool::Wall => meshes.add(Cuboid::new(2.0, 4.0, 0.5)),
            };
            let mat = materials.add(StandardMaterial { base_color: color, ..default() });

            let mut x = min_x;
            while x <= max_x + 0.1 {
                let mut z = min_z;
                while z <= max_z + 0.1 {
                    if state.scrap >= cost {
                        state.scrap -= cost;
                        let pos = Vec3::new(x, 0.0, z);
                        
                        let id = commands.spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_translation(pos + Vec3::Y * 0.2),
                            Structure, Health { current: 100.0, max: 100.0 },
                            RigidBody::Fixed,
                            Collider::cuboid(1.0, 1.0, 1.0),
                        )).id();

                        if build_manager.selected_tool == BuildTool::Turret {
                            commands.entity(id).insert(Turret { cooldown: 0.0 });
                        } else if build_manager.selected_tool == BuildTool::FarmPlot {
                            commands.entity(id).insert(Farm { state: FarmState::Growing, timer: 0.0 });
                        }
                    }
                    z += size;
                }
                x += size;
            }
        }
    }
}

// --- CONTROLS ---

fn unified_cursor_system(
    mut cursor_state: ResMut<WorldCursorState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), (With<SmartCameraRig>, Without<FaceCamera>)>, 
    mut cursor_visual: Query<&mut Transform, With<WorldCursorIndicator>>,
) {
    let window = window_query.single();
    if let Ok((camera, cam_transform)) = camera_query.get_single() {
        if let Some(cursor_position) = window.cursor_position() {
            if let Ok(ray) = camera.viewport_to_world(cam_transform, cursor_position) {
                let t = -ray.origin.y / ray.direction.y;
                if t > 0.0 {
                    let ground_pos = ray.origin + ray.direction * t;
                    cursor_state.target_pos = ground_pos;
                    if let Ok(mut t) = cursor_visual.get_single_mut() {
                        t.translation = ground_pos + Vec3::new(0.0, 0.2, 0.0);
                    }
                }
            }
        }
    }
}

fn toggle_camera_mode(keys: Res<ButtonInput<KeyCode>>, state: Res<State<CameraMode>>, mut next: ResMut<NextState<CameraMode>>) {
    if keys.just_pressed(KeyCode::Tab) {
        match state.get() {
            CameraMode::ThirdPerson => next.set(CameraMode::Isometric),
            CameraMode::Isometric => next.set(CameraMode::ThirdPerson),
        }
    }
}

fn smart_camera_controls(
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut camera_query: Query<(&mut Transform, &mut SmartCameraRig), (Without<Player>, Without<FaceCamera>)>,
    player_query: Query<(&Transform, &Velocity), With<Player>>,
    cursor: Res<WorldCursorState>,
    rapier_context: Single<&RapierContext>,
) {
    if let Ok((mut cam_transform, mut rig)) = camera_query.get_single_mut() {
        if let Ok((player_t, player_v)) = player_query.get_single() {
            let dt = time.delta_secs();

            // 1. Rotation Input
            let mut has_input = false;
            let mouse_delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
            
            if mouse_btn.pressed(MouseButton::Right) && mouse_delta.length_squared() > 0.0 {
                rig.target_yaw -= mouse_delta.x * 0.003;
                rig.target_pitch -= mouse_delta.y * 0.003;
                rig.target_pitch = rig.target_pitch.clamp(0.1, PI / 2.1);
                rig.time_since_input = 0.0;
                has_input = true;
            } else {
                rig.time_since_input += dt;
            }

            // 2. Auto-Align (Lazy Follow)
            let speed = player_v.linvel.length();
            if !has_input && speed > 1.0 && rig.time_since_input > CAM_AUTO_ALIGN_DELAY {
                let move_dir = player_v.linvel.normalize();
                let target_angle = -move_dir.z.atan2(move_dir.x) - PI / 2.0;
                
                let mut diff = target_angle - rig.target_yaw;
                while diff < -PI { diff += 2.0 * PI; }
                while diff > PI { diff -= 2.0 * PI; }
                
                rig.target_yaw += diff * dt * CAM_AUTO_ALIGN_SPEED;
            }

            // 3. Zoom
            for event in mouse_wheel.read() { 
                rig.target_dist -= event.y * 5.0; 
                rig.time_since_input = 0.0;
            }
            rig.target_dist = rig.target_dist.clamp(CAM_MIN_DIST, CAM_MAX_DIST);

            // 4. Smoothing
            rig.yaw = rig.yaw.lerp(rig.target_yaw, dt * CAM_SMOOTH_ROT);
            rig.pitch = rig.pitch.lerp(rig.target_pitch, dt * CAM_SMOOTH_ROT);

            // 5. Focus Point
            let peek_target = cursor.target_pos;
            let mut ideal_focus = player_t.translation;
            ideal_focus = ideal_focus.lerp(peek_target, 0.2); 
            rig.focus_point = rig.focus_point.lerp(ideal_focus, dt * CAM_SMOOTH_POS);

            // 6. Collision & Positioning
            let rotation = Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch);
            let direction = rotation * Vec3::Z;
            let ideal_pos = rig.focus_point + direction * rig.target_dist;
            let cast_dir = (ideal_pos - rig.focus_point).normalize();
            
            let mut final_dist = rig.target_dist;
            
            if let Some((_, hit_dist)) = rapier_context.cast_ray(
                rig.focus_point, 
                cast_dir, 
                rig.target_dist, 
                true, 
                QueryFilter::exclude_dynamic().exclude_sensors() 
            ) {
                final_dist = (hit_dist - CAM_WALL_BUFFER).max(CAM_MIN_DIST);
            }
            
            rig.current_dist = rig.current_dist.lerp(final_dist, dt * 10.0);

            cam_transform.translation = rig.focus_point + cast_dir * rig.current_dist;
            cam_transform.look_at(rig.focus_point, Vec3::Y);
        }
    }
}

fn player_movement(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut player_query: Query<(&mut Transform, &mut Velocity), With<Player>>,
    camera_query: Query<&SmartCameraRig>,
) {
    if let Ok((_, mut velocity)) = player_query.get_single_mut() {
        if let Ok(rig) = camera_query.get_single() {
            let mut input = Vec3::ZERO;
            if keyboard.pressed(KeyCode::KeyW) { input.z -= 1.0; }
            if keyboard.pressed(KeyCode::KeyS) { input.z += 1.0; }
            if keyboard.pressed(KeyCode::KeyA) { input.x -= 1.0; }
            if keyboard.pressed(KeyCode::KeyD) { input.x += 1.0; }

            if input.length_squared() > 0.0 {
                input = input.normalize();
                let rot = Quat::from_rotation_y(rig.yaw);
                let move_dir = rot * input;
                velocity.linvel.x = move_dir.x * PLAYER_SPEED;
                velocity.linvel.z = move_dir.z * PLAYER_SPEED;
            } else {
                velocity.linvel.x = velocity.linvel.x.lerp(0.0, 10.0 * time.delta_secs());
                velocity.linvel.z = velocity.linvel.z.lerp(0.0, 10.0 * time.delta_secs());
            }
        }
    }
}

fn player_look_system(
    cursor: Res<WorldCursorState>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    if let Ok(mut t) = player.get_single_mut() {
        let target = Vec3::new(cursor.target_pos.x, t.translation.y, cursor.target_pos.z);
        t.look_at(target, Vec3::Y);
    }
}

fn weapon_mechanics(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut player_query: Query<(&Transform, &mut Player)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Ok((t, mut p)) = player_query.get_single_mut() {
        p.fire_timer -= time.delta_secs();

        if mouse.pressed(MouseButton::Left) && p.fire_timer <= 0.0 {
            p.fire_timer = FIRE_RATE;
            let spawn_pos = t.translation + t.forward() * 0.5 + Vec3::new(0.0, 1.0, 0.0);
            
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
                Projectile { damage: BULLET_DAMAGE, lifetime: Timer::from_seconds(1.0, TimerMode::Once), from_player: true },
                RigidBody::Dynamic, Collider::ball(0.15), Sensor,
                Velocity { linvel: t.forward() * BULLET_SPEED, angvel: Vec3::ZERO },
            ));
        }
    }
}

fn melee_mechanics(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut player_query: Query<(&Transform, &mut Player)>,
    mut enemy_query: Query<(Entity, &GlobalTransform, &mut Health), With<Enemy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Ok((t, mut p)) = player_query.get_single_mut() {
        p.melee_timer -= time.delta_secs();

        if mouse.just_pressed(MouseButton::Right) && p.melee_timer <= 0.0 {
            p.melee_timer = MELEE_COOLDOWN;
            
            commands.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(12.0, 12.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.0, 1.0, 1.0),
                    emissive: LinearRgba::new(0.0, 5.0, 5.0, 5.0),
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::from_translation(t.translation + t.forward() * 3.0 + Vec3::Y)
                    .with_rotation(t.rotation * Quat::from_rotation_x(PI / 2.0)),
                SlashEffect { timer: Timer::from_seconds(0.15, TimerMode::Once) },
            ));

            for (e, e_t, mut hp) in enemy_query.iter_mut() {
                if t.translation.distance(e_t.translation()) < 8.0 {
                    hp.current -= MELEE_DAMAGE;
                    if hp.current <= 0.0 { commands.entity(e).despawn_recursive(); }
                }
            }
        }
    }
}

fn manual_interact(
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<WorldCursorState>,
    mut structures: Query<(Entity, &GlobalTransform, &mut Health, Option<&mut Farm>)>,
    mut stats: ResMut<PlayerStats>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mesh_query: Query<&mut MeshMaterial3d<StandardMaterial>, With<Farm>>,
) {
    if keys.pressed(KeyCode::KeyE) {
        let heal_amount = 1.0; 
        let empty_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.4, 0.25, 0.1), ..default() });

        for (e, t, mut hp, farm_opt) in structures.iter_mut() {
            if t.translation().distance(cursor.target_pos) < 6.0 {
                // Repair
                if hp.current < hp.max {
                    hp.current += heal_amount;
                    if hp.current > hp.max { hp.current = hp.max; }
                }

                // Harvest Farm
                if let Some(mut farm) = farm_opt {
                    if let FarmState::Ripe = farm.state {
                        farm.state = FarmState::Growing;
                        farm.timer = 0.0;
                        stats.scrap += FARM_YIELD;
                        if let Ok(mut mat) = mesh_query.get_mut(e) {
                            *mat = MeshMaterial3d(empty_mat.clone());
                        }
                    }
                }
            }
        }
    }
}

// --- AI ---

fn drone_ai(
    time: Res<Time>,
    mut drone_query: Query<(&mut Transform, &mut Drone), Without<Player>>,
    player_query: Query<&Transform, With<Player>>,
    enemy_query: Query<(Entity, &GlobalTransform), With<Enemy>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Ok(player_t) = player_query.get_single() {
        for (mut drone_t, mut drone) in drone_query.iter_mut() {
            let target_pos = player_t.translation + (player_t.rotation * drone.hover_offset);
            drone_t.translation = drone_t.translation.lerp(target_pos, 5.0 * time.delta_secs());
            drone_t.look_at(player_t.translation + player_t.forward() * 10.0, Vec3::Y);

            drone.cooldown -= time.delta_secs();
            if drone.cooldown <= 0.0 {
                let mut nearest = None;
                let mut min_dist = DRONE_RANGE;
                for (_e_e, e_t) in enemy_query.iter() {
                    let d = drone_t.translation.distance(e_t.translation());
                    if d < min_dist {
                        min_dist = d;
                        nearest = Some(e_t.translation());
                    }
                }

                if let Some(target) = nearest {
                    drone.cooldown = DRONE_COOLDOWN;
                    let dir = (target - drone_t.translation).normalize();
                    
                    commands.spawn((
                        Mesh3d(meshes.add(Sphere::new(0.1))),
                        MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::new(2.0, 1.0, 0.0, 5.0), ..default() })),
                        Transform::from_translation(drone_t.translation),
                        Projectile { damage: 15.0, lifetime: Timer::from_seconds(1.0, TimerMode::Once), from_player: true },
                        RigidBody::Dynamic, Collider::ball(0.1), Sensor,
                        Velocity { linvel: dir * BULLET_SPEED, angvel: Vec3::ZERO },
                    ));
                }
            }
        }
    }
}

fn turret_ai(
    time: Res<Time>,
    mut turret_query: Query<(&GlobalTransform, &mut Turret)>,
    enemy_query: Query<&GlobalTransform, With<Enemy>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (t, mut turret) in turret_query.iter_mut() {
        turret.cooldown -= time.delta_secs();
        if turret.cooldown <= 0.0 {
            let mut nearest = None;
            let mut min_dist = TURRET_RANGE;
            let pos = t.translation();

            for e_t in enemy_query.iter() {
                let d = pos.distance(e_t.translation());
                if d < min_dist {
                    min_dist = d;
                    nearest = Some(e_t.translation());
                }
            }

            if let Some(target) = nearest {
                turret.cooldown = 0.3;
                let dir = (target - pos).normalize();
                
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.1, 0.1, 0.5))),
                    MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::new(0.0, 2.0, 2.0, 5.0), ..default() })),
                    Transform::from_translation(pos + Vec3::Y * 0.5).looking_at(target, Vec3::Y),
                    Projectile { damage: 25.0, lifetime: Timer::from_seconds(1.5, TimerMode::Once), from_player: true },
                    RigidBody::Dynamic, Collider::cuboid(0.05, 0.05, 0.25), Sensor,
                    Velocity { linvel: dir * BULLET_SPEED, angvel: Vec3::ZERO },
                ));
            }
        }
    }
}

fn farm_logic(
    time: Res<Time>,
    mut farms: Query<(Entity, &mut Farm, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let ripe_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.8, 0.2), emissive: LinearRgba::new(0.0, 0.5, 0.0, 1.0), ..default() });

    for (_e, mut farm, mut mat) in farms.iter_mut() {
        if let FarmState::Growing = farm.state {
            farm.timer += time.delta_secs();
            if farm.timer > FARM_GROWTH_TIME {
                farm.state = FarmState::Ripe;
                *mat = MeshMaterial3d(ripe_mat.clone());
            }
        }
    }
}

fn agent_ai(
    time: Res<Time>,
    mut agents: Query<(&mut Transform, &mut Velocity, &mut BotAgent)>,
    player_query: Query<&Transform, (With<Player>, Without<BotAgent>)>,
    enemy_query: Query<(Entity, &GlobalTransform), With<Enemy>>,
    mut structures: Query<(Entity, &GlobalTransform, &mut Health, Option<&mut Farm>, &mut MeshMaterial3d<StandardMaterial>), (With<Structure>, Without<BotAgent>)>,
    phase: Res<PhaseManager>,
    mut stats: ResMut<PlayerStats>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(player_t) = player_query.get_single() else { return };
    let mut rng = rand::thread_rng();
    let empty_farm_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.4, 0.25, 0.1), ..default() });

    for (mut t, mut v, mut agent) in agents.iter_mut() {
        agent.attack_cooldown -= time.delta_secs();

        match phase.current {
            GamePhase::Build => {
                let mut target_pos = None;
                
                // Prioritize Harvest
                for (_e, s_t, _, farm_opt, mut mat) in structures.iter_mut() {
                    if let Some(mut farm) = farm_opt {
                        if let FarmState::Ripe = farm.state {
                            if t.translation.distance(s_t.translation()) < 3.0 {
                                // Harvest!
                                farm.state = FarmState::Growing;
                                farm.timer = 0.0;
                                stats.scrap += FARM_YIELD;
                                *mat = MeshMaterial3d(empty_farm_mat.clone());
                            } else {
                                target_pos = Some(s_t.translation());
                            }
                            break;
                        }
                    }
                }

                // If no harvest, check repair
                if target_pos.is_none() {
                    for (_, s_t, mut hp, _, _) in structures.iter_mut() {
                        if hp.current < hp.max {
                            if t.translation.distance(s_t.translation()) < 3.0 {
                                hp.current += 0.5; // Repair rate
                            } else {
                                target_pos = Some(s_t.translation());
                            }
                            break;
                        }
                    }
                }

                if let Some(pos) = target_pos {
                    let dir = (pos - t.translation).normalize();
                    v.linvel.x = dir.x * AGENT_SPEED;
                    v.linvel.z = dir.z * AGENT_SPEED;
                    let look = Vec3::new(pos.x, t.translation.y, pos.z);
                    t.look_at(look, Vec3::Y);
                } else {
                    let d = t.translation.distance(player_t.translation);
                    if d > 15.0 {
                        let dir = (player_t.translation - t.translation).normalize();
                        v.linvel.x = dir.x * 12.0;
                        v.linvel.z = dir.z * 12.0;
                    } else {
                        v.linvel.x += rng.gen_range(-2.0..2.0);
                        v.linvel.z += rng.gen_range(-2.0..2.0);
                    }
                }
            },
            GamePhase::Combat => {
                let mut target_enemy = None;
                let mut min_dist = AGENT_AGGRO_RANGE; 

                for (_e_e, e_t) in enemy_query.iter() {
                    let d = t.translation.distance(e_t.translation());
                    if d < min_dist {
                        min_dist = d;
                        target_enemy = Some(e_t.translation());
                    }
                }

                if let Some(target) = target_enemy {
                    let dir = (target - t.translation).normalize();
                    v.linvel.x = dir.x * AGENT_SPEED;
                    v.linvel.z = dir.z * AGENT_SPEED;
                    let look = Vec3::new(target.x, t.translation.y, target.z);
                    t.look_at(look, Vec3::Y);
                } else {
                    let offset = Vec3::new(rng.gen_range(-6.0..6.0), 0.0, rng.gen_range(-6.0..6.0));
                    let target = player_t.translation + offset;
                    let dir = (target - t.translation).normalize();
                    v.linvel.x = dir.x * 18.0;
                    v.linvel.z = dir.z * 18.0;
                }
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
    player_query: Query<&Transform, With<Player>>,
) {
    if phase.current == GamePhase::Build { return; }

    *timer += time.delta_secs();
    let rate = 1.0 / (phase.wave_number as f32 * 0.5).max(0.5); 

    if *timer > rate {
        *timer = 0.0;
        if let Ok(player_t) = player_query.get_single() {
            let mut rng = rand::thread_rng();
            let angle = rng.gen_range(0.0..PI*2.0);
            let dist = 100.0;
            let pos = player_t.translation + Vec3::new(angle.cos() * dist, 2.0, angle.sin() * dist);
            let is_giant = rng.gen_bool(0.1);

            let (mesh, mat, hp, scale) = if is_giant {
                (
                    meshes.add(Capsule3d::new(1.0, 2.5)),
                    materials.add(StandardMaterial { base_color: Color::srgb(0.8, 0.0, 0.0), ..default() }),
                    300.0,
                    Vec3::new(1.0, 1.0, 1.0)
                )
            } else {
                (
                    meshes.add(Capsule3d::new(0.4, 1.0)),
                    materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.1, 0.1), ..default() }),
                    50.0 + (phase.wave_number as f32 * 5.0),
                    Vec3::new(1.0, 1.0, 1.0)
                )
            };

            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_translation(pos).with_scale(scale),
                Enemy { is_giant },
                Health { current: hp, max: hp },
                Boing { base_scale: scale, intensity: 1.0 },
                RigidBody::Dynamic, Collider::capsule_y(0.5, 0.4), LockedAxes::ROTATION_LOCKED,
                Velocity::default(),
            ));
        }
    }
}

fn enemy_ai_swarm(
    mut enemy_query: Query<(&mut Transform, &mut Velocity, &Enemy), (Without<Structure>, Without<Player>)>,
    player_query: Query<&Transform, (With<Player>, Without<Enemy>)>,
    structure_query: Query<(&Transform, &mut Health, Entity), (With<Structure>, Without<Enemy>)>,
) {
    let Ok(player_t) = player_query.get_single() else { return };

    for (mut t, mut v, enemy) in enemy_query.iter_mut() {
        let mut target = player_t.translation;
        let mut min_dist = t.translation.distance(player_t.translation);

        for (s_t, _, _) in structure_query.iter() {
            let d = t.translation.distance(s_t.translation);
            if d < 15.0 && d < min_dist {
                min_dist = d;
                target = s_t.translation;
            }
        }

        let dir = (target - t.translation).normalize_or_zero();
        let speed = if enemy.is_giant { GIANT_SPEED } else { ZOMBIE_SPEED };
        
        v.linvel.x = dir.x * speed;
        v.linvel.z = dir.z * speed;
        
        if dir.length_squared() > 0.01 {
            let y = t.translation.y;
            let look_target = Vec3::new(target.x, y, target.z);
            t.look_at(look_target, Vec3::Y);
        }
    }
}

fn projectile_logic(
    mut commands: Commands, 
    time: Res<Time>, 
    mut projs: Query<(Entity, &mut Projectile, &Transform)>,
    mut enemies: Query<(Entity, &GlobalTransform, &mut Health), (With<Enemy>, Without<Player>)>,
) {
    for (p_e, mut proj, p_t) in projs.iter_mut() {
        proj.lifetime.tick(time.delta());
        if proj.lifetime.finished() { commands.entity(p_e).despawn(); continue; }
        
        if proj.from_player {
            for (e_e, e_t, mut hp) in enemies.iter_mut() {
                if p_t.translation.distance(e_t.translation()) < 1.5 {
                    hp.current -= proj.damage;
                    if hp.current <= 0.0 { commands.entity(e_e).despawn_recursive(); }
                    commands.entity(p_e).despawn();
                    break;
                }
            }
        }
    }
}

// --- UTILS & FX ---

fn resize_face_cam(
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut camera_query: Query<&mut Camera, With<FaceCamera>>,
) {
    if let Ok(window) = window_query.get_single() {
        if let Ok(mut camera) = camera_query.get_single_mut() {
            let size_x = 320;
            let size_y = 240;
            let phys_w = window.physical_width();
            let phys_h = window.physical_height();

            if phys_w > size_x && phys_h > size_y {
                camera.viewport = Some(Viewport {
                    physical_position: UVec2::new(phys_w - size_x, phys_h - size_y), // Top Right
                    physical_size: UVec2::new(size_x, size_y),
                    ..default()
                });
            }
        }
    }
}

fn face_cam_follow(
    player_query: Query<&Transform, With<Player>>,
    mut cam_query: Query<&mut Transform, (With<FaceCamera>, Without<Player>)>,
) {
    if let Ok(player_t) = player_query.get_single() {
        if let Ok(mut cam_t) = cam_query.get_single_mut() {
            let head_pos = player_t.translation + Vec3::new(0.0, 1.7, 0.0);
            let offset = player_t.forward() * 1.5 + player_t.right() * 0.5 + Vec3::new(0.0, 0.2, 0.0);
            cam_t.translation = head_pos + offset;
            cam_t.look_at(head_pos, Vec3::Y);
        }
    }
}

fn update_hud(
    phase: Res<PhaseManager>,
    tool: Res<BuildManager>,
    mut text: Query<&mut Text, With<HudText>>,
    player_hp: Query<&Health, With<Player>>,
    stats: Res<PlayerStats>,
) {
    if let Ok(mut t) = text.get_single_mut() {
        let hp = player_hp.get_single().map(|h| h.current).unwrap_or(0.0);
        let phase_name = match phase.current { GamePhase::Build => "BUILD", GamePhase::Combat => "COMBAT" };
        let tool_name = match tool.selected_tool { BuildTool::FarmPlot => "FARM", BuildTool::Turret => "TURRET", BuildTool::Wall => "WALL" };
        
        t.0 = format!(
            "PHASE: {} ({:.0}s)\nWAVE: {}\nHP: {:.0}\nSCRAP: {}\nTOOL: {} [1,2,3]\n\n[ALT+DRAG] Build\n[E] Repair/Harvest\n[R-CLICK] Rotate Cam",
            phase_name, phase.timer.remaining_secs(), phase.wave_number, hp, stats.scrap, tool_name
        );
    }
}

fn update_performance_metrics(
    time: Res<Time>,
    mut metrics: ResMut<PerformanceMetrics>,
    mut timer: Local<f32>, 
) {
    *timer += time.delta_secs();
    if *timer > 0.5 {
        let current_fps = 1.0 / time.delta_secs_f64();
        metrics.fps = (metrics.fps * 0.9) + (current_fps * 0.1); 
        metrics.last_update = Instant::now();
        *timer = 0.0;
    }
}

fn muzzle_flash_logic(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut MuzzleFlash, &mut PointLight, &mut Transform)>,
) {
    for (entity, mut flash, mut light, mut transform) in query.iter_mut() {
        flash.timer.tick(time.delta());
        let percent = flash.timer.fraction();
        light.intensity = 4000.0 * (1.0 - percent).powf(2.0); 
        let scale = 1.0 + (percent * 0.5);
        transform.scale = Vec3::splat(scale);
        if flash.timer.finished() { commands.entity(entity).despawn(); }
    }
}

fn slash_effect_logic(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut SlashEffect, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut effect, mut transform, mat_handle) in query.iter_mut() {
        effect.timer.tick(time.delta());
        let percent = effect.timer.fraction();
        let expansion_scale = 1.0 + (percent * 1.5);
        transform.scale = Vec3::splat(expansion_scale);
        if let Some(material) = materials.get_mut(mat_handle) {
            let alpha = (1.0f32 - percent).clamp(0.0, 1.0);
            material.base_color.set_alpha(alpha);
        }
        if effect.timer.finished() { commands.entity(entity).despawn(); }
    }
}

fn boing_animation_system(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &Velocity, &Boing)>,
) {
    for (mut t, v, boing) in query.iter_mut() {
        let speed = v.linvel.length();
        let time_secs = time.elapsed_secs();
        
        if speed > 1.0 {
            // Running: Fast Squash & Stretch
            let bounce = (time_secs * 15.0).sin() * 0.1 * boing.intensity;
            t.scale.y = boing.base_scale.y + bounce;
            t.scale.x = boing.base_scale.x - bounce * 0.5;
            t.scale.z = boing.base_scale.z - bounce * 0.5;
            
            // Lean forward into velocity
            let dir = v.linvel.normalize();
            let tilt_axis = dir.cross(Vec3::Y);
            let tilt_amount = (speed * 0.005).min(0.2) * boing.intensity;
            // Apply local tilt
            let current = t.rotation;
            let target = Quat::from_axis_angle(tilt_axis, -tilt_amount) * current;
            t.rotation = current.slerp(target, 0.1);
            
        } else {
            // Idle: Slow Breathing
            let breath = (time_secs * 3.0).sin() * 0.02 * boing.intensity;
            t.scale.y = boing.base_scale.y + breath;
            t.scale.x = boing.base_scale.x - breath * 0.5;
            t.scale.z = boing.base_scale.z - breath * 0.5;
        }
    }
}

fn draw_selection_gizmos(mut gizmos: Gizmos, selection: Res<SelectionState>) {
    if selection.is_dragging {
        let center = (selection.start_pos + selection.current_pos) / 2.0;
        let size = (selection.start_pos - selection.current_pos).abs().xz();
        gizmos.rect(Isometry3d::new(center + Vec3::Y * 0.1, Quat::from_rotation_x(PI/2.0)), size, Color::WHITE);
    }
}