use bevy::color::palettes::css::*;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_rapier3d::prelude::*;
use std::f32::consts::PI;

// --- TUNING CONSTANTS ---
const PLAYER_SPEED: f32 = 45.0;
const TROOP_SPEED: f32 = 25.0;
const DRILL_YIELD: u32 = 15;
const DRILL_INTERVAL: f32 = 3.0;
const TRAINING_INTERVAL: f32 = 5.0;
const GRAVITY_SCALE: f32 = 6.0;

// --- STATES ---
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum GameState {
    #[default]
    Management,
    Invasion,
}

// --- RESOURCES ---
#[derive(Resource)]
struct PlayerEconomy {
    scrap: u32,
    army_pool: u32,
    max_army: u32,
}

#[derive(Resource, Default)]
struct WorldCursor {
    pos: Vec3,
}

#[derive(Resource)]
struct BuildManager {
    selected: BuildTool,
}

#[derive(PartialEq, Clone, Copy)]
enum BuildTool {
    Drill,
    Barracks,
}

// --- COMPONENTS ---
#[derive(Component)]
struct Player {
    fire_timer: f32,
}

#[derive(Component)]
struct Health {
    current: f32,
    max: f32,
}

#[derive(Component)]
struct Boing {
    base_scale: Vec3,
    intensity: f32,
}

#[derive(Component, Default)]
struct SmartCameraRig {
    yaw: f32,
    pitch: f32,
    target_yaw: f32,
    target_pitch: f32,
    target_dist: f32,
    focus_point: Vec3,
}

#[derive(Component)]
struct ScrapDrill {
    timer: Timer,
}

#[derive(Component)]
struct Barracks {
    timer: Timer,
}

#[derive(Component)]
struct TroopAI;

#[derive(Component)]
struct Structure;

#[derive(Component)]
struct HudText;

// --- MAIN ---

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "CLASH OF SCRAPS V2.0".into(),
                    ..default()
                }),
                ..default()
            }),
            RapierPhysicsPlugin::<NoUserData>::default(),
        ))
        .init_state::<GameState>()
        .insert_resource(PlayerEconomy {
            scrap: 300,
            army_pool: 0,
            max_army: 20,
        })
        .insert_resource(BuildManager {
            selected: BuildTool::Drill,
        })
        .init_resource::<WorldCursor>()
        .add_systems(Startup, (setup_world, setup_player))
        .add_systems(
            Update,
            (
                unified_cursor_system,
                smart_camera_controls,
                player_movement,
                economy_training_logic,
                action_input_system,
                troop_ai_system,
                lively_animations,
                ui_management_system,
                phase_toggle_system,
            ),
        )
        .run();
}

// --- SETUP ---

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(1000.0, 1000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::from(GREEN),
            perceptual_roughness: 0.9,
            ..default()
        })),
        RigidBody::Fixed,
        Collider::cuboid(500.0, 0.1, 500.0),
    ));

    // Sun
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_rotation_x(-PI / 3.0)),
    ));
}

fn setup_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The Hero Unit
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(0.5, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::from(AQUA), // Fixed: CYAN is AQUA in CSS palette
            emissive: LinearRgba::new(0.0, 1.0, 2.0, 1.0),
            ..default()
        })),
        Transform::from_xyz(0.0, 5.0, 0.0),
        Player { fire_timer: 0.0 },
        Health { current: 1000.0, max: 1000.0 },
        Boing { base_scale: Vec3::ONE, intensity: 1.0 },
        RigidBody::Dynamic,
        Collider::capsule_y(0.5, 0.5),
        LockedAxes::ROTATION_LOCKED,
        Velocity::default(),
        GravityScale(GRAVITY_SCALE),
    ));

    // Smart Camera
    commands.spawn((
        Camera3d::default(),
        SmartCameraRig {
            target_dist: 50.0,
            target_pitch: PI / 4.0,
            ..default()
        },
    ));
}

// --- PHASE LOGIC ---

fn phase_toggle_system(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        match state.get() {
            GameState::Management => next.set(GameState::Invasion),
            GameState::Invasion => next.set(GameState::Management),
        }
    }
}

fn economy_training_logic(
    time: Res<Time>,
    mut econ: ResMut<PlayerEconomy>,
    mut drills: Query<(&mut ScrapDrill, &mut Boing)>,
    mut barracks: Query<(&mut Barracks, &mut Boing), Without<ScrapDrill>>,
) {
    for (mut drill, mut boing) in drills.iter_mut() {
        drill.timer.tick(time.delta());
        if drill.timer.just_finished() {
            econ.scrap += DRILL_YIELD;
            boing.intensity = 2.0; 
        }
    }

    for (mut bar, mut boing) in barracks.iter_mut() {
        bar.timer.tick(time.delta());
        if bar.timer.just_finished() && econ.army_pool < econ.max_army {
            econ.army_pool += 1;
            boing.intensity = 1.5; 
        }
    }
}

// --- INPUT & ACTION ---

fn action_input_system(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<WorldCursor>,
    state: Res<State<GameState>>,
    mut econ: ResMut<PlayerEconomy>,
    mut manager: ResMut<BuildManager>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    match state.get() {
        GameState::Management => {
            if keys.just_pressed(KeyCode::Digit1) { manager.selected = BuildTool::Drill; }
            if keys.just_pressed(KeyCode::Digit2) { manager.selected = BuildTool::Barracks; }

            if mouse.just_pressed(MouseButton::Left) {
                let cost = match manager.selected {
                    BuildTool::Drill => 50,
                    BuildTool::Barracks => 150,
                };

                if econ.scrap >= cost {
                    econ.scrap -= cost;
                    let color = match manager.selected {
                        BuildTool::Drill => Color::from(ORANGE),
                        BuildTool::Barracks => Color::from(PURPLE),
                    };

                    let b_id = commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(4.0, 4.0, 4.0))),
                        MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                        Transform::from_translation(cursor.pos + Vec3::Y * 2.0),
                        Structure,
                        Health { current: 500.0, max: 500.0 },
                        Boing { base_scale: Vec3::ONE, intensity: 2.0 },
                        RigidBody::Fixed,
                        Collider::cuboid(2.0, 2.0, 2.0),
                    )).id();

                    if manager.selected == BuildTool::Drill {
                        commands.entity(b_id).insert(ScrapDrill { timer: Timer::from_seconds(DRILL_INTERVAL, TimerMode::Repeating) });
                    } else {
                        commands.entity(b_id).insert(Barracks { timer: Timer::from_seconds(TRAINING_INTERVAL, TimerMode::Repeating) });
                    }
                }
            }
        }
        GameState::Invasion => {
            if mouse.just_pressed(MouseButton::Left) && econ.army_pool > 0 {
                econ.army_pool -= 1;
                commands.spawn((
                    Mesh3d(meshes.add(Sphere::new(0.7))),
                    MeshMaterial3d(materials.add(StandardMaterial { 
                        base_color: Color::from(RED), 
                        emissive: LinearRgba::new(2.0, 0.0, 0.0, 1.0),
                        ..default() 
                    })),
                    Transform::from_translation(cursor.pos + Vec3::Y * 10.0),
                    TroopAI,
                    Boing { base_scale: Vec3::ONE, intensity: 1.0 },
                    RigidBody::Dynamic,
                    Collider::ball(0.7),
                    Velocity::default(),
                ));
            }
        }
    }
}

fn troop_ai_system(
    mut troops: Query<(&mut Velocity, &mut Transform), With<TroopAI>>,
    structures: Query<&GlobalTransform, With<Structure>>,
) {
    for (mut vel, mut trans) in troops.iter_mut() {
        let mut closest_pos = None;
        let mut min_dist = 1000.0;

        for s_trans in structures.iter() {
            let dist = trans.translation.distance(s_trans.translation());
            if dist < min_dist {
                min_dist = dist;
                closest_pos = Some(s_trans.translation());
            }
        }

        if let Some(target) = closest_pos {
            let dir = (target - trans.translation).normalize_or_zero();
            vel.linvel.x = dir.x * TROOP_SPEED;
            vel.linvel.z = dir.z * TROOP_SPEED;
            trans.look_at(target, Vec3::Y);
        }
    }
}

// --- CORE UTILS ---

fn player_movement(
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<WorldCursor>,
    mut query: Query<(&mut Velocity, &mut Transform), With<Player>>,
    cam_q: Query<&SmartCameraRig>,
) {
    if let Ok((mut vel, mut trans)) = query.get_single_mut() {
        if let Ok(rig) = cam_q.get_single() {
            let mut move_dir = Vec3::ZERO;
            if keys.pressed(KeyCode::KeyW) { move_dir.z -= 1.0; }
            if keys.pressed(KeyCode::KeyS) { move_dir.z += 1.0; }
            if keys.pressed(KeyCode::KeyA) { move_dir.x -= 1.0; }
            if keys.pressed(KeyCode::KeyD) { move_dir.x += 1.0; }

            if move_dir.length_squared() > 0.0 {
                let rot = Quat::from_rotation_y(rig.yaw);
                let final_dir = rot * move_dir.normalize();
                vel.linvel.x = final_dir.x * PLAYER_SPEED;
                vel.linvel.z = final_dir.z * PLAYER_SPEED;
            }

            let look_t = Vec3::new(cursor.pos.x, trans.translation.y, cursor.pos.z);
            trans.look_at(look_t, Vec3::Y);
        }
    }
}

fn smart_camera_controls(
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut cam_q: Query<(&mut Transform, &mut SmartCameraRig)>,
    player_q: Query<&Transform, (With<Player>, Without<SmartCameraRig>)>,
) {
    if let Ok((mut trans, mut rig)) = cam_q.get_single_mut() {
        if let Ok(p_trans) = player_q.get_single() {
            let dt = time.delta_secs();

            if mouse_btn.pressed(MouseButton::Right) {
                let delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
                rig.target_yaw -= delta.x * 0.003;
                rig.target_pitch = (rig.target_pitch - delta.y * 0.003).clamp(0.1, PI / 2.2);
            }

            for event in mouse_wheel.read() {
                rig.target_dist = (rig.target_dist - event.y * 5.0).clamp(10.0, 150.0);
            }

            rig.yaw = rig.yaw.lerp(rig.target_yaw, dt * 5.0);
            rig.pitch = rig.pitch.lerp(rig.target_pitch, dt * 5.0);
            rig.focus_point = rig.focus_point.lerp(p_trans.translation, dt * 4.0);

            let rot = Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch);
            trans.translation = rig.focus_point + rot * Vec3::new(0.0, 0.0, rig.target_dist);
            trans.look_at(rig.focus_point, Vec3::Y);
        }
    }
}

fn unified_cursor_system(
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<SmartCameraRig>>,
    mut cursor: ResMut<WorldCursor>,
) {
    if let (Ok(window), Ok((camera, cam_t))) = (window_query.get_single(), camera_query.get_single()) {
        if let Some(pos) = window.cursor_position() {
            if let Ok(ray) = camera.viewport_to_world(cam_t, pos) {
                let t = -ray.origin.y / ray.direction.y;
                cursor.pos = ray.origin + ray.direction * t;
            }
        }
    }
}

fn lively_animations(time: Res<Time>, mut query: Query<(&mut Transform, &mut Boing)>) {
    for (mut trans, mut boing) in query.iter_mut() {
        let s = (time.elapsed_secs() * 12.0).sin() * 0.1 * boing.intensity;
        trans.scale = boing.base_scale + Vec3::new(-s, s, -s);
        boing.intensity = (boing.intensity - time.delta_secs()).max(0.2);
    }
}

fn ui_management_system(
    econ: Res<PlayerEconomy>,
    manager: Res<BuildManager>,
    state: Res<State<GameState>>,
    mut query: Query<&mut Text, With<HudText>>,
    mut commands: Commands,
) {
    let tool_name = match manager.selected {
        BuildTool::Drill => "DRILL (50 Scrap)",
        BuildTool::Barracks => "BARRACKS (150 Scrap)",
    };

    let content = format!(
        "PHASE: {:?}\nSCRAP: {}\nARMY POOL: {}/{}\nTOOL: {} [1,2]\n\n[TAB] Switch Phase\n[L-CLICK] Build/Deploy",
        state.get(), econ.scrap, econ.army_pool, econ.max_army, tool_name
    );

    if let Ok(mut text) = query.get_single_mut() {
        text.0 = content;
    } else {
        commands.spawn((
            Text::new(content),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(20.0),
                left: Val::Px(20.0),
                ..default()
            },
            HudText,
        ));
    }
}