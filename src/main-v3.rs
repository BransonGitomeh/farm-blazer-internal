use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_rapier3d::prelude::*;
use std::f32::consts::PI;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::bloom::Bloom;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use rand::prelude::*;
use bevy::window::CursorGrabMode;

// --- TUNING CONSTANTS ---

const PLAYER_SPEED: f32 = 45.0; 
const GRAVITY_SCALE: f32 = 6.0;

// Camera V2 Stats (Deadzone Edition)
const CAM_DIST_DEFAULT: f32 = 60.0;
const CAM_SMOOTH_SPEED: f32 = 5.0;
const CAM_ROT_SPEED: f32 = 0.003;
const CAM_DEADZONE: f32 = 8.0; // The "Loose Rope" length

// Building
const GRID_SIZE: f32 = 4.0; 
const WALL_SIZE_X: f32 = 4.0;
const WALL_SIZE_Z: f32 = 1.0;

// Costs
const DRILL_COST: u32 = 50;
const TURRET_COST: u32 = 80;
const WALL_COST: u32 = 15;
const BARRACKS_COST: u32 = 200;

// Balance & Scaling
const BASE_ENEMY_HP: f32 = 50.0;
const HP_SCALING_PER_WAVE: f32 = 1.15; // 15% hp increase per wave
const DRILL_INTERVAL: f32 = 3.0;
const BARRACKS_SPAWN_RATE: f32 = 6.0; 

// Unit Stats
const DRONE_SPEED: f32 = 25.0;
const SOLDIER_SPEED: f32 = 22.0;

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
    unit_count: u32,
    unit_cap: u32,
}

#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum BuildTool {
    #[default]
    Drill,
    Turret,
    Wall,
    Barracks,
}

#[derive(Resource)]
struct BuildManager {
    tool: BuildTool,
    rotation_idx: u32, 
}

#[derive(Resource, Default)]
struct WorldCursor {
    pos: Vec3,
    snapped_pos: Vec3,
}

#[derive(Resource)]
struct PhaseManager {
    timer: Timer,
    wave: u32,
    is_combat: bool,
}

// --- COMPONENTS ---

#[derive(Component)]
struct Player {
    fire_timer: f32,
}

#[derive(Component)]
struct Health { current: f32, max: f32 }

#[derive(Component)]
struct Boing { base_scale: Vec3, intensity: f32 }
impl Default for Boing { fn default() -> Self { Self { base_scale: Vec3::ONE, intensity: 1.0 } } }

#[derive(Component)]
struct Structure; 

#[derive(Component)]
struct GhostPreview; 

#[derive(Component)]
struct ScrapDrill { timer: Timer }

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
}

#[derive(Component)]
struct Turret { cooldown: f32 }

#[derive(Component)]
struct Enemy { is_giant: bool }

#[derive(Component)]
struct Projectile { damage: f32, lifetime: Timer, from_player: bool }

#[derive(Component)]
struct WowCameraRig {
    // Spherical Coordinates
    pub yaw: f32,
    pub pitch: f32,
    pub radius: f32,
    
    // Smooth Targets
    pub target_yaw: f32,
    pub target_pitch: f32,
    pub target_radius: f32,

    // Settings
    pub sensitivity: f32,
    pub zoom_speed: f32,
    pub min_pitch: f32, // Look down angle
    pub max_pitch: f32, // Look up angle
    pub min_dist: f32,
    pub max_dist: f32,
}

impl Default for WowCameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: PI / 6.0,   // Slight angle down
            radius: 50.0,
            
            target_yaw: 0.0,
            target_pitch: PI / 6.0,
            target_radius: 50.0,

            sensitivity: 0.003,
            zoom_speed: 5.0,
            min_pitch: 0.1,       // Almost horizontal
            max_pitch: PI / 2.1,  // Top-down
            min_dist: 5.0,
            max_dist: 150.0,
        }
    }
}

#[derive(Component)]
struct MuzzleFlash { timer: Timer }

#[derive(Component)]
struct HudText;

// --- MAIN ---

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "SWARM DEFENSE: ENDLESS".into(),
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            RapierPhysicsPlugin::<NoUserData>::default(),
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default(),
        ))
        .init_state::<GameState>()
        .insert_resource(PlayerStats { scrap: 500, unit_count: 0, unit_cap: 8 })
        .insert_resource(BuildManager { tool: BuildTool::Drill, rotation_idx: 0 })
        .init_resource::<WorldCursor>()
        .insert_resource(PhaseManager { 
            timer: Timer::from_seconds(60.0, TimerMode::Once), 
            wave: 1, 
            is_combat: false 
        })
        .insert_resource(ClearColor(Color::srgb(0.01, 0.01, 0.02)))
        .insert_resource(AmbientLight { color: Color::srgb(0.1, 0.1, 0.2), brightness: 0.3 })
        .add_systems(Startup, (setup_world, setup_player, setup_ui, setup_cursor))
        // Logic (Only run when Playing)
        .add_systems(Update, (
            wow_camera_system,     
            cursor_raycast_system,
            update_hud,
            check_game_over,
            boing_anim,
            muzzle_flash_logic,
            projectile_logic,
        ))
        .add_systems(Update, (
            wow_movement_system,
            phase_logic,
            build_tool_input,
            ghost_preview_system,
            place_building_system,
            weapon_mechanics,
            manual_repair,
            unit_spawner_system,
            unit_ai_system,
            turret_ai,
            enemy_spawner,
            enemy_ai,
            economy_logic,
        ).run_if(in_state(GameState::Playing)))
        // Restart Logic
        .add_systems(Update, restart_game_system.run_if(in_state(GameState::GameOver)))
        .run();
}

// --- SETUP ---

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera V2 with Deadzone defaults
    commands.spawn((
        Camera3d::default(),
        Camera { hdr: true, ..default() },
        Tonemapping::TonyMcMapface,
        Bloom::default(),
        WowCameraRig::default(),
        Transform::from_xyz(0.0, 50.0, 50.0),
    ));

    // Sun
    commands.spawn((
        DirectionalLight {
            illuminance: 4000.0,
            shadows_enabled: true,
            color: Color::srgb(0.8, 0.9, 1.0),
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -PI / 3.0, PI / 4.0, 0.0)),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.06, 0.08),
            perceptual_roughness: 0.8,
            ..default()
        })),
        RigidBody::Fixed,
        Collider::cuboid(1000.0, 0.1, 1000.0),
    ));
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
        Player { fire_timer: 0.0 },
        Health { current: 500.0, max: 500.0 },
        RigidBody::Dynamic,
        Collider::capsule_y(0.5, 0.4), 
        LockedAxes::ROTATION_LOCKED,
        Velocity::default(),
        GravityScale(GRAVITY_SCALE),
    )).with_children(|parent| {
        parent.spawn(PointLight { 
            color: Color::srgb(0.0, 1.0, 1.0), 
            intensity: 2000.0, 
            range: 20.0, 
            ..default() 
        });
    });
}

fn setup_ui(mut commands: Commands) {
    commands.spawn(Node {
        width: Val::Percent(100.0),
        padding: UiRect::all(Val::Px(10.0)),
        ..default()
    }).with_children(|root| {
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        )).with_children(|panel| {
            panel.spawn((Text::new("Init..."), TextFont { font_size: 16.0, ..default() }, HudText));
        });
    });
}

fn setup_cursor(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // Hidden cursor helper, mostly logic based now
}

// --- CAMERA V2 (DEADZONE FIX) ---

fn wow_camera_system(
    mut commands: Commands,
    time: Res<Time>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut q_win: Query<&mut Window, With<PrimaryWindow>>,
    mut q_cam: Query<(&mut Transform, &mut WowCameraRig)>,
    mut q_player: Query<&mut Transform, (With<Player>, Without<WowCameraRig>)>, // Note: Mutable access to player rotation
    rapier_context: Single<&RapierContext>,
) {
    let Ok(mut window) = q_win.get_single_mut() else { return };
    let Ok((mut cam_t, mut rig)) = q_cam.get_single_mut() else { return };
    let Ok(mut player_t) = q_player.get_single_mut() else { return };
    
    let dt = time.delta_secs();
    let mouse_delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
    
    // --- INPUT STATES ---
    let is_building = keys.pressed(KeyCode::AltLeft);
    let right_click = mouse_btn.pressed(MouseButton::Right);
    let left_click = mouse_btn.pressed(MouseButton::Left);
    
    // 1. Cursor Locking Logic
    if !is_building && (right_click || left_click) {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    } else {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }

    // 2. Rotation Logic
    if !is_building {
        if right_click || left_click {
            // Camera Rotate
            rig.target_yaw -= mouse_delta.x * rig.sensitivity;
            rig.target_pitch = (rig.target_pitch - mouse_delta.y * rig.sensitivity)
                .clamp(rig.min_pitch, rig.max_pitch);
        }

        // WOW MECHANIC: If Right Click, Player turns to face Camera Yaw immediately
        if right_click {
            let target_player_rot = Quat::from_rotation_y(rig.target_yaw);
            player_t.rotation = player_t.rotation.slerp(target_player_rot, dt * 15.0);
        }
    }

    // 3. Zoom Logic
    for ev in mouse_wheel.read() {
        rig.target_radius = (rig.target_radius - ev.y * rig.zoom_speed)
            .clamp(rig.min_dist, rig.max_dist);
    }

    // 4. Smoothing
    rig.yaw = rig.yaw.lerp(rig.target_yaw, dt * 15.0);
    rig.pitch = rig.pitch.lerp(rig.target_pitch, dt * 15.0);
    rig.radius = rig.radius.lerp(rig.target_radius, dt * 10.0);

    // 5. Position Calculation & Wall Collision
    let player_head = player_t.translation + Vec3::new(0.0, 1.5, 0.0); // Look at head, not feet
    let rot = Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch);
    let offset = rot * Vec3::new(0.0, 0.0, rig.radius);
    let desired_pos = player_head + offset;

    // Raycast for anti-clip (Camera Collision)
    let dir = (desired_pos - player_head).normalize();
    let max_dist = rig.radius;
    let mut final_pos = desired_pos;

    if let Some((_, hit_dist)) = rapier_context.cast_ray(
        player_head, 
        dir, 
        max_dist, 
        true, 
        QueryFilter::exclude_dynamic().exclude_sensors()
    ) {
        // Wall hit! Pull camera in
        final_pos = player_head + (dir * (hit_dist - 0.5).max(0.5));
    }

    cam_t.translation = final_pos;
    cam_t.look_at(player_head, Vec3::Y);
}

fn cursor_raycast_system(
    mut cursor: ResMut<WorldCursor>,
    q_win: Query<&Window, With<PrimaryWindow>>,
    q_cam: Query<(&Camera, &GlobalTransform), With<WowCameraRig>>,
) {
    let (cam, cam_t) = q_cam.single();
    let win = q_win.single();

    if let Some(screen_pos) = win.cursor_position() {
        if let Ok(ray) = cam.viewport_to_world(cam_t, screen_pos) {
            let t = -ray.origin.y / ray.direction.y;
            if t > 0.0 {
                cursor.pos = ray.origin + ray.direction * t;
                let gs = GRID_SIZE;
                let sx = (cursor.pos.x / gs).round() * gs;
                let sz = (cursor.pos.z / gs).round() * gs;
                cursor.snapped_pos = Vec3::new(sx, 0.0, sz);
            }
        }
    }
}

// --- BUILDING SYSTEM ---

fn build_tool_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut mgr: ResMut<BuildManager>,
) {
    if keys.just_pressed(KeyCode::Digit1) { mgr.tool = BuildTool::Drill; }
    if keys.just_pressed(KeyCode::Digit2) { mgr.tool = BuildTool::Turret; }
    if keys.just_pressed(KeyCode::Digit3) { mgr.tool = BuildTool::Wall; }
    if keys.just_pressed(KeyCode::Digit4) { mgr.tool = BuildTool::Barracks; }
    if keys.just_pressed(KeyCode::KeyR) { mgr.rotation_idx = (mgr.rotation_idx + 1) % 4; }
}

fn ghost_preview_system(
    mut commands: Commands,
    mgr: Res<BuildManager>,
    cursor: Res<WorldCursor>,
    keys: Res<ButtonInput<KeyCode>>,
    mut q_ghost: Query<(Entity, &mut Transform, &mut Mesh3d), With<GhostPreview>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let show_ghost = keys.pressed(KeyCode::AltLeft);

    if !show_ghost {
        for (e, _, _) in q_ghost.iter() { commands.entity(e).despawn(); }
        return;
    }

    let rot_quat = Quat::from_rotation_y(mgr.rotation_idx as f32 * (PI / 2.0));

    if let Ok((_, mut t, mut mesh)) = q_ghost.get_single_mut() {
        t.translation = cursor.snapped_pos + Vec3::Y * 0.1;
        t.rotation = rot_quat;
        
        let new_mesh = match mgr.tool {
            BuildTool::Wall => meshes.add(Cuboid::new(WALL_SIZE_X, 3.0, WALL_SIZE_Z)),
            BuildTool::Drill => meshes.add(Cuboid::new(3.0, 4.0, 3.0)),
            BuildTool::Turret => meshes.add(Cuboid::new(1.0, 2.0, 1.0)),
            BuildTool::Barracks => meshes.add(Cuboid::new(4.0, 3.0, 4.0)),
        };
        *mesh = Mesh3d(new_mesh);

    } else {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::default())), 
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.0, 1.0, 0.0, 0.3), 
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            GhostPreview,
        ));
    }
}

fn place_building_system(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<WorldCursor>,
    mgr: Res<BuildManager>,
    mut stats: ResMut<PlayerStats>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !keys.pressed(KeyCode::AltLeft) { return; }

    if mouse.just_pressed(MouseButton::Left) {
        let (cost, mesh, color, size_y) = match mgr.tool {
            BuildTool::Drill => (DRILL_COST, meshes.add(Cuboid::new(3.0, 4.0, 3.0)), Color::srgb(1.0, 0.5, 0.0), 2.0),
            BuildTool::Turret => (TURRET_COST, meshes.add(Cuboid::new(1.0, 2.5, 1.0)), Color::srgb(0.5, 0.5, 0.5), 1.25),
            BuildTool::Wall => (WALL_COST, meshes.add(Cuboid::new(WALL_SIZE_X, 3.0, WALL_SIZE_Z)), Color::srgb(0.3, 0.3, 0.3), 1.5),
            BuildTool::Barracks => (BARRACKS_COST, meshes.add(Cuboid::new(4.0, 3.0, 4.0)), Color::srgb(0.0, 0.2, 0.8), 1.5),
        };

        if stats.scrap >= cost {
            stats.scrap -= cost;
            let rot = Quat::from_rotation_y(mgr.rotation_idx as f32 * (PI / 2.0));
            
            let collider = if mgr.tool == BuildTool::Wall {
                if mgr.rotation_idx % 2 != 0 {
                    Collider::cuboid(WALL_SIZE_Z / 2.0, 1.5, WALL_SIZE_X / 2.0)
                } else {
                    Collider::cuboid(WALL_SIZE_X / 2.0, 1.5, WALL_SIZE_Z / 2.0)
                }
            } else {
                Collider::cuboid(1.5, size_y / 2.0, 1.5)
            };

            let id = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                Transform::from_translation(cursor.snapped_pos + Vec3::new(0.0, size_y, 0.0)).with_rotation(rot),
                Structure,
                Health { current: 150.0, max: 150.0 },
                Boing { base_scale: Vec3::ONE, intensity: 0.5 },
                RigidBody::Fixed,
                collider,
            )).id();

            match mgr.tool {
                BuildTool::Drill => { commands.entity(id).insert(ScrapDrill { timer: Timer::from_seconds(3.0, TimerMode::Repeating) }); },
                BuildTool::Turret => { commands.entity(id).insert(Turret { cooldown: 0.0 }); },
                BuildTool::Barracks => { 
                    commands.entity(id).insert((
                        Barracks { timer: Timer::from_seconds(BARRACKS_SPAWN_RATE, TimerMode::Repeating), spawn_drone_next: true },
                        HomeBase { pos: cursor.snapped_pos + Vec3::new(0.0, 5.0, 0.0) }
                    )); 
                    stats.unit_cap += 4;
                },
                _ => {}
            }
        }
    }
}

// --- UNIT LOGIC ---

fn unit_spawner_system(
    time: Res<Time>,
    mut barracks: Query<(&mut Barracks, &GlobalTransform, &mut Boing)>,
    mut stats: ResMut<PlayerStats>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut b, t, mut boing) in barracks.iter_mut() {
        b.timer.tick(time.delta());
        if b.timer.just_finished() && stats.unit_count < stats.unit_cap {
            stats.unit_count += 1;
            boing.intensity = 1.5;
            let spawn_pos = t.translation() + Vec3::new(0.0, 2.0, 2.0);
            
            if b.spawn_drone_next {
                commands.spawn((
                    Mesh3d(meshes.add(Sphere::new(0.3))),
                    MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::new(0.0, 5.0, 1.0, 3.0), ..default() })),
                    Transform::from_translation(spawn_pos + Vec3::Y * 4.0),
                    Unit { is_flying: true },
                    HomeBase { pos: t.translation() + Vec3::Y * 6.0 },
                    Health { current: 30.0, max: 30.0 },
                    Boing::default(),
                    RigidBody::Dynamic, Collider::ball(0.3), GravityScale(0.0), Damping { linear_damping: 2.0, angular_damping: 1.0 },
                    Velocity::default(),
                ));
            } else {
                commands.spawn((
                    Mesh3d(meshes.add(Capsule3d::new(0.3, 0.6))),
                    MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.0, 0.0, 1.0), ..default() })),
                    Transform::from_translation(spawn_pos),
                    Unit { is_flying: false },
                    HomeBase { pos: t.translation() },
                    Health { current: 80.0, max: 80.0 },
                    Boing::default(),
                    RigidBody::Dynamic, Collider::capsule_y(0.3, 0.3), LockedAxes::ROTATION_LOCKED,
                    Velocity::default(),
                ));
            }
            b.spawn_drone_next = !b.spawn_drone_next;
        }
    }
}

fn unit_ai_system(
    mut commands: Commands,
    time: Res<Time>,
    mut units: Query<(Entity, &mut Transform, &mut Velocity, &Unit, &HomeBase)>,
    enemies: Query<(Entity, &GlobalTransform), With<Enemy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shoot_timer: Local<f32>,
) {
    *shoot_timer += time.delta_secs();

    for (_e, mut t, mut v, unit, home) in units.iter_mut() {
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
                let dir = (target_pos - t.translation).normalize();
                let speed = if unit.is_flying { DRONE_SPEED } else { SOLDIER_SPEED };
                v.linvel = v.linvel.lerp(dir * speed, 0.1);
                t.look_at(target_pos, Vec3::Y);
            } else {
                v.linvel = v.linvel.lerp(Vec3::ZERO, 0.1);
                if unit.is_flying {
                    if *shoot_timer > 0.1 && rand::random::<f32>() < 0.03 { 
                        let dir = (target_pos - t.translation).normalize();
                        commands.spawn((
                            Mesh3d(meshes.add(Sphere::new(0.1))),
                            MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::new(0.0, 10.0, 10.0, 5.0), ..default() })),
                            Transform::from_translation(t.translation + *t.forward()),
                            Projectile { damage: 10.0, lifetime: Timer::from_seconds(1.0, TimerMode::Once), from_player: true },
                            RigidBody::Dynamic, Collider::ball(0.1), Sensor, GravityScale(0.0),
                            Velocity { linvel: dir * 60.0, angvel: Vec3::ZERO },
                        ));
                    }
                }
            }
        } else {
            let dir = (home.pos - t.translation).normalize();
            let dist = t.translation.distance(home.pos);
            if dist > 2.0 {
                let speed = if unit.is_flying { DRONE_SPEED } else { SOLDIER_SPEED };
                v.linvel = v.linvel.lerp(dir * speed, 0.05);
                t.look_at(home.pos, Vec3::Y);
            } else {
                v.linvel = v.linvel.lerp(Vec3::ZERO, 0.1);
            }
        }
    }
}

// --- GAMEPLAY SYSTEMS ---

fn economy_logic(time: Res<Time>, mut drills: Query<(&mut ScrapDrill, &mut Boing)>, mut stats: ResMut<PlayerStats>) {
    for (mut d, mut b) in drills.iter_mut() {
        d.timer.tick(time.delta());
        if d.timer.just_finished() { 
            stats.scrap += 15; 
            b.intensity = 2.0;
        }
    }
}

fn wow_movement_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut q_player: Query<(&mut Velocity, &mut Transform), With<Player>>,
    q_cam: Query<&WowCameraRig>,
) {
    let Ok((mut velocity, mut transform)) = q_player.get_single_mut() else { return };
    let Ok(rig) = q_cam.get_single() else { return };

    let right_click_held = mouse_btn.pressed(MouseButton::Right);
    let mut move_input = Vec3::ZERO;

    // Standard WASD mapping
    if keys.pressed(KeyCode::KeyW) { move_input.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { move_input.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { move_input.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { move_input.x += 1.0; }

    if move_input.length_squared() > 0.0 {
        move_input = move_input.normalize();

        // Calculate absolute direction based on Camera Yaw
        let cam_rot = Quat::from_rotation_y(rig.yaw);
        let move_dir = cam_rot * move_input;

        // Apply Velocity
        velocity.linvel.x = move_dir.x * PLAYER_SPEED;
        velocity.linvel.z = move_dir.z * PLAYER_SPEED;

        // Rotation Logic
        if !right_click_held {
            // If NOT steering with mouse, WASD rotates the character body
            // We want the character to look where they are moving
            let target_angle = -move_dir.z.atan2(move_dir.x) + PI / 2.0; // Math to get angle from vector
            let target_rot = Quat::from_rotation_y(target_angle);
            transform.rotation = transform.rotation.slerp(target_rot, time.delta_secs() * 10.0);
        } 
        // If right_click_held, rotation is handled in the camera system (Strafe mode)
    } else {
        // Friction / Stop
        velocity.linvel.x = velocity.linvel.x.lerp(0.0, time.delta_secs() * 10.0);
        velocity.linvel.z = velocity.linvel.z.lerp(0.0, time.delta_secs() * 10.0);
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
) {
    if keys.pressed(KeyCode::AltLeft) { return; }

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
            if t.translation().distance(cursor.pos) < 6.0 {
                if hp.current < hp.max { hp.current += 1.0; }
            }
        }
    }
}

// SCALED DIFFICULTY SPAWNER
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
    
    // Scale Spawn Rate: 1.5s -> 0.3s as waves progress
    let spawn_delay = (1.5 - (phase.wave as f32 * 0.05)).max(0.3);
    
    *timer += time.delta_secs();
    if *timer > spawn_delay {
        *timer = 0.0;
        if let Ok(p_t) = player_q.get_single() {
            let angle = rand::random::<f32>() * PI * 2.0;
            let pos = p_t.translation + Vec3::new(angle.cos() * 80.0, 2.0, angle.sin() * 80.0);
            
            // Scale HP
            let hp_mult = HP_SCALING_PER_WAVE.powi(phase.wave as i32);
            let hp = BASE_ENEMY_HP * hp_mult;
            
            commands.spawn((
                Mesh3d(meshes.add(Capsule3d::new(0.4, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.2, 0.2), ..default() })),
                Transform::from_translation(pos),
                Enemy { is_giant: false },
                Health { current: hp, max: hp },
                Boing { base_scale: Vec3::ONE, intensity: 1.0 },
                RigidBody::Dynamic, Collider::capsule_y(0.5, 0.4), LockedAxes::ROTATION_LOCKED,
                Velocity::default(),
            ));
        }
    }
}

fn enemy_ai(mut enemies: Query<(&mut Velocity, &mut Transform), With<Enemy>>, player: Query<&Transform, (With<Player>, Without<Enemy>)>, structures: Query<&GlobalTransform, With<Structure>>) {
    let Ok(p_t) = player.get_single() else { return };
    for (mut v, mut t) in enemies.iter_mut() {
        let mut target = p_t.translation;
        let mut min_dist = t.translation.distance(target);
        
        for s in structures.iter() {
            let d = t.translation.distance(s.translation());
            if d < 20.0 && d < min_dist { min_dist = d; target = s.translation(); }
        }
        
        let dir = (target - t.translation).normalize_or_zero();
        v.linvel.x = dir.x * 15.0;
        v.linvel.z = dir.z * 15.0;
        
        let y = t.translation.y;
        t.look_at(Vec3::new(target.x, y, target.z), Vec3::Y);
    }
}

fn projectile_logic(mut commands: Commands, time: Res<Time>, mut projs: Query<(Entity, &mut Projectile, &Transform)>, mut enemies: Query<(Entity, &GlobalTransform, &mut Health), With<Enemy>>) {
    for (pe, mut p, pt) in projs.iter_mut() {
        p.lifetime.tick(time.delta());
        if p.lifetime.finished() { commands.entity(pe).despawn(); continue; }
        
        for (ee, et, mut hp) in enemies.iter_mut() {
            if pt.translation.distance(et.translation()) < 2.0 {
                hp.current -= p.damage;
                commands.entity(pe).despawn();
                if hp.current <= 0.0 { commands.entity(ee).despawn_recursive(); }
                break;
            }
        }
    }
}

fn turret_ai(time: Res<Time>, mut turrets: Query<(&GlobalTransform, &mut Turret)>, enemies: Query<&GlobalTransform, With<Enemy>>, mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for (t, mut tur) in turrets.iter_mut() {
        tur.cooldown -= time.delta_secs();
        if tur.cooldown <= 0.0 {
            for e_t in enemies.iter() {
                if t.translation().distance(e_t.translation()) < 40.0 {
                    tur.cooldown = 0.4;
                    let dir = (e_t.translation() - t.translation()).normalize();
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(0.2, 0.2, 0.6))),
                        MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::new(1.0, 0.5, 0.0, 5.0), ..default() })),
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

// --- UTILS & GAME LOOP ---

fn check_game_over(
    mut next_state: ResMut<NextState<GameState>>,
    player_q: Query<&Health, With<Player>>
) {
    if let Ok(hp) = player_q.get_single() {
        if hp.current <= 0.0 {
            next_state.set(GameState::GameOver);
        }
    }
}

fn restart_game_system(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
    enemies: Query<Entity, With<Enemy>>,
    structures: Query<Entity, With<Structure>>,
    units: Query<Entity, With<Unit>>,
    mut player_q: Query<&mut Health, With<Player>>,
    mut phase: ResMut<PhaseManager>,
    mut stats: ResMut<PlayerStats>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        // Reset Stats
        if let Ok(mut hp) = player_q.get_single_mut() { hp.current = hp.max; }
        stats.scrap = 500;
        stats.unit_count = 0;
        stats.unit_cap = 8;
        phase.wave = 1;
        phase.is_combat = false;
        phase.timer = Timer::from_seconds(60.0, TimerMode::Once);

        // Despawn everything except player and ground
        for e in enemies.iter() { commands.entity(e).despawn_recursive(); }
        for e in structures.iter() { commands.entity(e).despawn_recursive(); }
        for e in units.iter() { commands.entity(e).despawn_recursive(); }

        next_state.set(GameState::Playing);
    }
}

fn boing_anim(time: Res<Time>, mut q: Query<(&mut Transform, &Boing)>) {
    for (mut t, b) in q.iter_mut() {
        let s = (time.elapsed_secs() * 10.0).sin() * 0.05 * b.intensity;
        t.scale = b.base_scale + Vec3::splat(s);
    }
}

fn phase_logic(time: Res<Time>, mut pm: ResMut<PhaseManager>, mut lights: Query<&mut DirectionalLight>) {
    pm.timer.tick(time.delta());
    if pm.timer.finished() {
        pm.is_combat = !pm.is_combat;
        pm.timer = Timer::from_seconds(if pm.is_combat { 45.0 } else { 60.0 }, TimerMode::Once);
        if !pm.is_combat { pm.wave += 1; }
        
        if let Ok(mut l) = lights.get_single_mut() {
            l.color = if pm.is_combat { Color::srgb(1.0, 0.5, 0.5) } else { Color::srgb(0.8, 0.9, 1.0) };
        }
    }
}

fn update_hud(
    pm: Res<PhaseManager>, 
    mgr: Res<BuildManager>, 
    stats: Res<PlayerStats>, 
    mut txt: Query<&mut Text, With<HudText>>,
    state: Res<State<GameState>>,
) {
    if let Ok(mut t) = txt.get_single_mut() {
        if *state.get() == GameState::GameOver {
            t.0 = "GAME OVER\n\nPRESS [R] TO RESTART".to_string();
            return;
        }

        let phase = if pm.is_combat { "COMBAT" } else { "BUILD" };
        let tool = match mgr.tool { 
            BuildTool::Drill => "Drill ($50)", 
            BuildTool::Turret => "Turret ($80)", 
            BuildTool::Wall => "Wall ($15)", 
            BuildTool::Barracks => "Barracks ($200)" 
        };
        t.0 = format!(
            "{} - {:.0}s | Wave {}\nScrap: {}\nUnits: {}/{}\nTool: {} [1-4]\n[R] Rotate | [ALT] Build Mode",
            phase, pm.timer.remaining_secs(), pm.wave, stats.scrap, stats.unit_count, stats.unit_cap, tool
        );
    }
}

fn muzzle_flash_logic(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut MuzzleFlash)>) {
    for (e, mut f) in q.iter_mut() {
        f.timer.tick(time.delta());
        if f.timer.finished() { commands.entity(e).despawn(); }
    }
}