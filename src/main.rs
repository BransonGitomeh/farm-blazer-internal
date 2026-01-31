use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::pbr::{CascadeShadowConfigBuilder, NotShadowCaster, MaterialPlugin};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use bevy_rapier3d::prelude::*;
use std::f32::consts::PI;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::core_pipeline::bloom::Bloom;
use bevy::render::render_resource::{AddressMode, AsBindGroup, ShaderRef, SamplerDescriptor};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::reflect::TypePath;
use bevy::utils::HashMap;
use rand::prelude::*;
use bevy::asset::AssetMetaCheck;

mod mobile_controls;

// --- CONFIG ---
// Moved to WorldSettings for dynamic adjustment

#[derive(Resource)]
struct WorldSettings {
    pub hex_size: f32,
    pub tile_scale: f32,
    pub render_distance: i32,
    pub island_size: f32,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            // Doubled world scale for larger gameplay area
            // hex_size and tile_scale increased 2x
            hex_size: 50.0, 
            tile_scale: 86.0, 
            render_distance: 16,
            island_size: 12.0, 
        }
    }
}

#[derive(Resource, Default)]
struct HexGridState {
    spawned_tiles: HashMap<(i32, i32), Entity>,
    tile_types: HashMap<(i32, i32), TileType>,
    seed: f32,
}

#[derive(Component)]
struct HexTile;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum TileType {
    Water,
    WaterRock,
    Sand,
    Grass,
    Forest,
    Hill,
    Mountain,
    River,
    Path,
    Castle,
    House,
    Mill,
    Lumber,
    Sheep,
    WatchTower,
    Dock,
}

// --- NEW r#genERATION LOGIC ---
// This attempts to recreate the composition of the image (Island with a river partition)

fn get_tile_type(q: i32, r: i32, seed: f32, island_size: f32) -> TileType {
    // 1. COORDINATE UTILS
    let dist = (q.abs() + (q + r).abs() + r.abs()) as f32 / 2.0;
    
    // Noise for slight organic variation
    let noise = ((q as f32 * 0.5 + seed).sin() * (r as f32 * 0.5 + seed).cos()) * 2.0;
    
    // 2. THE RIVER (A winding cut through the island)
    // Hardcoded shaped flow: starts at (0, -4), flows down to (2, 4)
    let is_river = 
        (q == 0 && r >= -4 && r <= -1) || // Top vertical segment
        (q == 1 && r == -1) ||            // Bend
        (q == 1 && r >= -1 && r <= 2) ||  // Mid segment
        (q == 2 && r == 2) ||             // Bend
        (q == 2 && r >= 3 && r <= 5);     // Mouth

    if is_river { return TileType::River; }

    // 3. THE PATH (Connects Dock -> House -> Castle)
    let is_path = 
        (q == -2 && r == 0) || (q == -1 && r == 0) || // Horizontal lead up
        (q == -3 && r == 1) || (q == -3 && r == 2);   // Path to dock

    if is_path { return TileType::Path; }

    // 4. SPECIFIC POIs (Points of Interest from the image)
    if q == 0 && r == -5 { return TileType::Mountain; } // Source of river
    if q == 0 && r == -1 { return TileType::Castle; }   // Center Castle
    if q == 2 && r == 1  { return TileType::Mill; }     // Mill by the river
    if q == -3 && r == 3 { return TileType::Dock; }     // Dock at bottom left
    if q == -2 && r == 1 { return TileType::House; }    // Village
    if q == 1 && r == -3 { return TileType::WatchTower; } // Guard tower

    // 5. BIOME LAYERS
    // Water
    if dist > island_size - 1.0 + (noise * 0.2) {
        if noise > 0.8 { return TileType::WaterRock; }
        return TileType::Water;
    }
    
    // Beach/Sand (Coastline)
    if dist > island_size - 2.5 + (noise * 0.2) {
        return TileType::Sand;
    }

    // Mountains/Hills (Cluster near top/center)
    let mountain_bias = if r < -2 { 1.5 } else { 0.0 };
    if noise + mountain_bias > 1.8 { return TileType::Mountain; }
    if noise + mountain_bias > 1.2 { return TileType::Hill; }

    // Forests (Clumps of trees)
    if noise < -0.5 { return TileType::Forest; }
    
    // Flavor Tiles (Sheep/Lumber)
    let detail_rng = ((q * 13 + r * 37) as f32).sin();
    if detail_rng > 0.85 { return TileType::Sheep; }
    if detail_rng < -0.85 { return TileType::Lumber; }

    // Default
    TileType::Grass
}

// --- SYSTEMS ---

pub fn setup_hex_resources(mut commands: Commands) {
    commands.insert_resource(HexGridState {
        spawned_tiles: HashMap::new(),
        tile_types: HashMap::new(),
        seed: rand::random::<f32>() * 100.0,
    });
}

fn update_hex_map(
    mut commands: Commands,
    mut grid: ResMut<HexGridState>,
    asset_server: Res<AssetServer>,
    player_q: Query<&Transform, With<crate::Player>>,
    settings: Res<WorldSettings>,
) {
    let Ok(player_t) = player_q.get_single() else { return };

    let q = ((f32::sqrt(3.0) / 3.0 * player_t.translation.x - 1.0 / 3.0 * player_t.translation.z) / settings.hex_size).round() as i32;
    let r = ((2.0 / 3.0 * player_t.translation.z) / settings.hex_size).round() as i32;

    // PRE-PASS: Determine types first for neighbor checking
    for dq in -settings.render_distance..=settings.render_distance {
        for dr in -settings.render_distance..=settings.render_distance {
            if (dq + dr).abs() > settings.render_distance { continue; }
            let nq = q + dq;
            let nr = r + dr;
            if !grid.tile_types.contains_key(&(nq, nr)) {
                let t_type = get_tile_type(nq, nr, grid.seed, settings.island_size);
                grid.tile_types.insert((nq, nr), t_type);
            }
        }
    }

    // SPAWN PASS
    for dq in -settings.render_distance..=settings.render_distance {
        for dr in -settings.render_distance..=settings.render_distance {
            if (dq + dr).abs() > settings.render_distance { continue; }
            let neighbor_q = q + dq;
            let neighbor_r = r + dr;

            if !grid.spawned_tiles.contains_key(&(neighbor_q, neighbor_r)) {
                spawn_hex(
                    &mut commands, 
                    neighbor_q, 
                    neighbor_r, 
                    &asset_server, 
                    &mut grid,
                    &settings,
                );
            }
        }
    }
}

// --- MAIN SPAWN FUNCTION ---

fn spawn_hex(
    commands: &mut Commands,
    q: i32,
    r: i32,
    assets: &AssetServer,
    grid: &mut HexGridState,
    settings: &WorldSettings,
) {
    // 1. Calculate World Position
    let x = settings.hex_size * f32::sqrt(3.0) * (q as f32 + r as f32 / 2.0);
    let z = settings.hex_size * 3.0 / 2.0 * r as f32;
    let pos = Vec3::new(x, 0.0, z);

    let my_type = *grid.tile_types.get(&(q, r)).unwrap_or(&TileType::Water);

    // 2. State Variables for Model Selection
    let mut glb_path: String;
    let mut rotation_y = 0.0;
    let mut y_offset = 0.0;
    let mut collider = Some(Collider::cylinder(0.5, settings.hex_size * 0.9)); // Default ground collider
    let mut spawn_grass_base = false; // Does this tile need a grass tile underneath?
    let mut can_spawn_props = false;  // Can we put random trees/rocks here?

    // 3. Selection Logic
    match my_type {
        TileType::River | TileType::Path => {
            let is_river = my_type == TileType::River;
            let prefix = if is_river { "river" } else { "path" };

            // -- Neighbor Connectivity Logic --
            let neighbors = [
                (q + 1, r), (q, r + 1), (q - 1, r + 1), 
                (q - 1, r), (q, r - 1), (q + 1, r - 1)
            ];
            
            let mut mask = 0u8;
            for (i, (nq, nr)) in neighbors.iter().enumerate() {
                if let Some(nt) = grid.tile_types.get(&(*nq, *nr)) {
                    // What does this tile connect to?
                    let connects = if is_river {
                        // Rivers connect to other Rivers, Water, or the Dock
                        matches!(*nt, TileType::River | TileType::Water | TileType::WaterRock | TileType::Dock)
                    } else {
                        // Paths connect to Paths and all Buildings
                        matches!(*nt, TileType::Path | TileType::Castle | TileType::House | 
                                      TileType::Mill | TileType::Lumber | TileType::Sheep | 
                                      TileType::WatchTower | TileType::Dock)
                    };

                    if connects {
                        mask |= 1 << i;
                    }
                }
            }

            // Get the specific mesh (corner, straight, intersection) based on mask
            let (model, rot_steps) = get_intersection_model(mask);
            glb_path = format!("GLB format/{}-{}.glb", prefix, model);
            
            // Convert hexagonal steps (60 degrees) to radians
            rotation_y = -(rot_steps as f32) * PI / 3.0;

            if is_river {
                y_offset = -0.2; // Rivers sit lower
                collider = None; // Fall into water
            } else {
                y_offset = 0.02; // Paths sit *just* above the grass base
                spawn_grass_base = true; // Paths need grass underneath
            }
        },
        TileType::Water => {
            glb_path = "GLB format/water.glb".into();
            y_offset = -0.2;
            collider = None;
        },
        TileType::WaterRock => {
            glb_path = "GLB format/water-rocks.glb".into();
            y_offset = -0.2;
        },
        TileType::Sand => {
            glb_path = "GLB format/sand.glb".into();
            // Optional: Random rotation for variety
            rotation_y = rand::random::<f32>() * PI * 2.0; 
        },
        TileType::Grass => {
            glb_path = "GLB format/grass.glb".into();
            can_spawn_props = true;
        },
        TileType::Forest => {
            // Randomize forest density visuals
            if rand::random::<f32>() > 0.5 {
                glb_path = "GLB format/grass-forest.glb".into();
            } else {
                glb_path = "GLB format/unit-tree.glb".into(); // Single large tree
                spawn_grass_base = true;
            }
        },
        TileType::Hill => {
            glb_path = "GLB format/grass-hill.glb".into();
        },
        TileType::Mountain => {
            glb_path = "GLB format/stone-mountain.glb".into();
            // Tall cone collider to block movement
            collider = Some(Collider::cone(4.0, settings.hex_size));
        },
        TileType::Castle => {
            glb_path = "GLB format/building-castle.glb".into();
            spawn_grass_base = true;
            collider = Some(Collider::cuboid(3.0, 3.0, 3.0));
        },
        TileType::House => {
            glb_path = "GLB format/building-house.glb".into();
            spawn_grass_base = true;
            rotation_y = -PI / 6.0; // Angled slightly
        },
        TileType::Mill => {
            glb_path = "GLB format/building-mill.glb".into();
            spawn_grass_base = true;
        },
        TileType::Lumber => {
            glb_path = "GLB format/dirt-lumber.glb".into();
        },
        TileType::Sheep => {
            glb_path = "GLB format/building-sheep.glb".into();
            spawn_grass_base = true;
        },
        TileType::WatchTower => {
            glb_path = "GLB format/building-tower.glb".into();
            spawn_grass_base = true;
        },
        TileType::Dock => {
            glb_path = "GLB format/building-dock.glb".into();
            // Docks need to rotate to face the water/path logic, 
            // but for simplicity, we fix rotation or rely on manual placement logic.
            // Here we just rotate it to look okay in the specific map spot.
            rotation_y = PI; 
            y_offset = 0.1;
        },
    }

    // 4. Create Parent Entity
    let parent_id = commands.spawn((
        Transform::from_translation(pos),
        Visibility::default(),
        HexTile,
    )).id();

    let scale_vec = Vec3::splat(settings.tile_scale);

    // 5. Spawn Base Layer (if needed)
    // This prevents "void" gaps under buildings or paths
    if spawn_grass_base {
        let grass_scene = assets.load("GLB format/grass.glb#Scene0");
        commands.spawn((
            SceneRoot(grass_scene),
            Transform::from_translation(Vec3::ZERO).with_scale(scale_vec),
        )).set_parent(parent_id);
    }

    // 6. Spawn Main Model
    let scene = assets.load(format!("{}#Scene0", glb_path));
    commands.spawn((
        SceneRoot(scene),
        Transform::from_translation(Vec3::new(0.0, y_offset, 0.0))
            .with_rotation(Quat::from_rotation_y(rotation_y))
            .with_scale(scale_vec),
    )).set_parent(parent_id);

    // 7. Spawn Physics Collider
    if let Some(col) = collider {
        commands.spawn((
            RigidBody::Fixed,
            col,
            // Offset collider slightly up so it covers the model volume
            Transform::from_xyz(0.0, 1.0, 0.0), 
        )).set_parent(parent_id);
    }

    // 8. Prop System (Decoration)
    // Randomly adds fences, trees, or rocks to empty Grass/Sand tiles
    if can_spawn_props {
        let mut rng = rand::thread_rng();
        let chance: f32 = rng.r#gen();

        // 10% Chance for a loose tree (creates smooth transition to forests)
        if chance > 0.90 {
            let tree_scene = assets.load("GLB format/unit-tree.glb#Scene0");
            // Randomize tree scale/rotation slightly
            let tree_scale = settings.tile_scale * (0.8 + rng.r#gen::<f32>() * 0.4); 
            commands.spawn((
                SceneRoot(tree_scene),
                Transform::from_xyz(0.0, 0.0, 0.0)
                    .with_scale(Vec3::splat(tree_scale))
                    .with_rotation(Quat::from_rotation_y(rng.r#gen::<f32>() * PI * 2.0)),
            )).set_parent(parent_id);
        }
        // 5% Chance for a small wall/fence
        else if chance < 0.05 {
            let wall_scene = assets.load("GLB format/building-wall.glb#Scene0");
            commands.spawn((
                SceneRoot(wall_scene),
                Transform::from_xyz(0.0, 0.0, 0.0)
                    .with_scale(scale_vec)
                    .with_rotation(Quat::from_rotation_y(rng.r#gen::<f32>() * PI * 2.0)),
            )).set_parent(parent_id);
        }
        // 2% Chance for a rock
        else if chance > 0.40 && chance < 0.42 {
            let rock_scene = assets.load("GLB format/stone.glb#Scene0");
             commands.spawn((
                SceneRoot(rock_scene),
                Transform::from_xyz(rng.r#gen::<f32>(), 0.0, rng.r#gen::<f32>())
                    .with_scale(scale_vec * 0.5),
            )).set_parent(parent_id);
        }
    }

    // 9. Register in Grid State
    grid.spawned_tiles.insert((q, r), parent_id);
}

fn apply_mesh_colliders(
    mut commands: Commands,
    q: Query<(Entity, &Mesh3d, &Parent), Added<Mesh3d>>,
    q_parents: Query<&Parent>,
    q_hextile: Query<&HexTile>,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, mesh_handle, parent) in q.iter() {
        // Walk up to find if this is part of a HexTile
        let mut current = parent.get();
        let mut is_tile = false;
        for _ in 0..5 {
            if q_hextile.contains(current) {
                is_tile = true;
                break;
            }
            if let Ok(p) = q_parents.get(current) {
                current = p.get();
            } else {
                break;
            }
        }
        
        if is_tile {
            if let Some(mesh) = meshes.get(mesh_handle) {
                let shape = bevy_rapier3d::geometry::ComputedColliderShape::TriMesh(TriMeshFlags::MERGE_DUPLICATE_VERTICES);
                if let Some(collider) = Collider::from_bevy_mesh(mesh, &shape) {
                    commands.entity(entity).insert(collider);
                }
            }
        }
    }
}

fn reload_scene_system(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut grid: ResMut<HexGridState>,
    tiles: Query<Entity, With<HexTile>>,
) {
    if keys.just_pressed(KeyCode::KeyG) {
        for entity in tiles.iter() {
            commands.entity(entity).despawn_recursive();
        }
        grid.spawned_tiles.clear();
        grid.seed = rand::random::<f32>() * 1000.0;
        info!("World Rer#generated with Seed: {}", grid.seed);
    }
}

fn world_tuner_system(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<WorldSettings>,
    mut grid: ResMut<HexGridState>,
    tiles: Query<Entity, With<HexTile>>,
) {
    let mut changed = false;
    let size_step = 0.5; 
    let scale_step = 0.5;
    
    // Use Shift for rapid changes, otherwise single clicks
    let rapid = keys.pressed(KeyCode::ShiftLeft);

    // Adjust Grid Spacing (Hex Size)
    if (rapid && keys.pressed(KeyCode::ArrowUp)) || keys.just_pressed(KeyCode::ArrowUp) { 
        settings.hex_size += size_step; changed = true; 
    }
    if (rapid && keys.pressed(KeyCode::ArrowDown)) || keys.just_pressed(KeyCode::ArrowDown) { 
        settings.hex_size = (settings.hex_size - size_step).max(0.1); changed = true; 
    }

    // Adjust Model Scale
    if (rapid && keys.pressed(KeyCode::ArrowRight)) || keys.just_pressed(KeyCode::ArrowRight) { 
        settings.tile_scale += scale_step; changed = true; 
    }
    if (rapid && keys.pressed(KeyCode::ArrowLeft)) || keys.just_pressed(KeyCode::ArrowLeft) { 
        settings.tile_scale = (settings.tile_scale - scale_step).max(0.1); changed = true; 
    }

    // Adjust Render Distance
    if keys.just_pressed(KeyCode::PageUp) {
        settings.render_distance += 1; changed = true;
    }
    if keys.just_pressed(KeyCode::PageDown) {
        settings.render_distance = (settings.render_distance - 1).max(1); changed = true;
    }

    // Adjust Island Size (Landmass)
    if (rapid && keys.pressed(KeyCode::Period)) || keys.just_pressed(KeyCode::Period) {
        settings.island_size += 0.5; changed = true;
    }
    if (rapid && keys.pressed(KeyCode::Comma)) || keys.just_pressed(KeyCode::Comma) {
        settings.island_size = (settings.island_size - 0.5).max(1.0); changed = true;
    }

    if changed {
        info!("World Tuner: Grid Size: {:.2}, Model Scale: {:.2}, Render Dist: {}, Island Size: {:.2}", 
            settings.hex_size, settings.tile_scale, settings.render_distance, settings.island_size);
        for entity in tiles.iter() {
            commands.entity(entity).despawn_recursive();
        }
        grid.spawned_tiles.clear();
        grid.tile_types.clear(); // Clear calculated types so they rer#generate with new size
    }
}

/// Helper to map a 6-bit hex neighbor mask to a model name and rotation steps (0-5).
fn get_intersection_model(mask: u8) -> (&'static str, u8) {
    if mask == 0 { return ("straight", 0); } 
    
    for r in 0..6 {
        let m = rotate_mask(mask, r);
        
        if m == 0b000001 { return ("end", r); }
        if m == 0b001000 { return ("end", (r+3)%6); }
        if m == 0b001001 { return ("straight", r); }
        if m == 0b000011 { return ("corner", r); } 
        if m == 0b000101 { return ("corner-sharp", r); }
        if m == 0b000111 { return ("intersectionA", r); } 
        if m == 0b001011 { return ("intersectionB", r); } 
        if m == 0b010011 { return ("intersectionC", r); } 
        if m == 0b010101 { return ("intersectionD", r); } 
        if m == 0b001111 { return ("intersectionE", r); }
        if m == 0b010111 { return ("intersectionF", r); }
        if m == 0b011011 { return ("intersectionG", r); }
        if m == 0b011111 { return ("intersectionH", r); }
    }

    ("crossing", 0)
}

fn rotate_mask(mask: u8, steps: u8) -> u8 {
    let mut m = mask & 0b111111;
    for _ in 0..steps {
        let low = m & 1;
        m = (m >> 1) | (low << 5);
    }
    m
}



// --- TUNING CONSTANTS ---

const PLAYER_SPEED: f32 = 90.0; 
const GRAVITY_SCALE: f32 = 12.0;

// Camera
const CAM_SMOOTH_SPEED: f32 = 10.0;
const GRID_SIZE: f32 = 2.0; 

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

#[derive(Component, Default)]
struct AutoTarget {
    target: Option<Entity>,
}

#[derive(Component)]
struct Targetable; // Tag for things we can lock onto

#[derive(Component)]
struct Health { current: f32, max: f32 }

#[derive(Component)]
struct Structure; 

#[derive(Component)]
struct Wall; 

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
    pub goal_radius: f32,
    pub target_yaw: f32,
    pub target_pitch: f32,
    // Limits
    pub min_pitch: f32,
    pub max_pitch: f32,
    pub min_dist: f32,
    pub max_dist: f32,
    pub zoom_sens: f32,
    pub rot_sens: f32,
}

impl Default for WowCameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0, pitch: PI / 6.0, radius: 150.0, goal_radius: 20.0,
            target_yaw: 0.0, target_pitch: PI / 6.0,
            min_pitch: 0.1, max_pitch: PI / 2.1,
            min_dist: 5.0, max_dist: 150.0,
            zoom_sens: 5.0, rot_sens: 0.003,
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
struct SkySphere;

#[derive(Component)]
struct FancyCursorVisual;

#[derive(Component)]
struct HudText;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub sun_position: Vec3,
    #[uniform(0)]
    pub turbidity: f32,
    #[uniform(0)]
    pub rayleigh: f32,
    #[uniform(0)]
    pub mie_coefficient: f32,
    #[uniform(0)]
    pub mie_directional_g: f32,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/sky.wgsl".into()
    }
}

// --- MAIN ---

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "SWARM DEFENSE: LOGISTICS COMMANDER".into(),
                        // 1. THIS MAKES IT FILL THE PARENT ELEMENT (The Body)
                        fit_canvas_to_parent: true, 
                        // Optional: Stops browser keys (like F5) from refreshing, useful for games
                        prevent_default_event_handling: false, 
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    watch_for_changes_override: Some(true),
                    // 2. THIS STOPS THE .META 404 ERRORS
                    meta_check: AssetMetaCheck::Never, 
                    ..default()
                })
                .set(ImagePlugin {
                    default_sampler: ImageSamplerDescriptor::linear(),
                })
        )
        .add_plugins(MaterialPlugin::<SkyMaterial>::default())
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        .add_plugins(mobile_controls::MobileControlsPlugin)
        // FrameTimeDiagnosticsPlugin, // Uncomment for FPS
        // LogDiagnosticsPlugin::default(),
        .init_state::<GameState>()
        .insert_resource(PlayerStats { scrap: 2000, max_scrap: 3000, unit_count: 0, unit_cap: 5 })
        .insert_resource(BuildManager { tool: BuildTool::Drill, rotation_idx: 0, is_drag_building: false, drag_start: None })
        .init_resource::<WorldCursor>()
        .init_resource::<SelectionState>()
        .init_resource::<WorldSettings>()
        .insert_resource(PhaseManager { 
            timer: Timer::from_seconds(60.0, TimerMode::Once), 
            wave: 1, 
            is_combat: false 
        })
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(PreStartup, setup_assets)
        .add_systems(Startup, setup_hex_resources)
        .add_systems(Startup, (setup_lighting_only, setup_player, setup_starting_village, setup_ui, setup_cursor_visuals))
        .add_systems(Update, (
            apply_mesh_colliders,
            update_hex_map, 
            wow_camera_system,     
            sky_sphere_follow_system,

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
            reload_scene_system,
            world_tuner_system,
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
            enemy_jump_system,
            restart_game_system, // Allow restarting anytime
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

fn setup_lighting_only(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Camera { hdr: true, ..default() },
        Projection::Perspective(PerspectiveProjection {
            far: 10000.0, // Ensure we can see the Sky Sphere at 1800.0
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Bloom { 
            intensity: 0.5, 
            low_frequency_boost: 0.7,
            ..default() 
        },
        WowCameraRig::default(),
        Transform::from_xyz(0.0, 150.0, 150.0),
    ));

    // Sun: Atmospheric Lighting
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0, // Reduced as requested
            shadows_enabled: true,
            color: Color::srgb(1.0, 0.98, 0.9), 
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -PI / 2.5, PI / 4.0, 0.0)),
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 40.0,
            maximum_distance: 400.0,
            ..default()
        }.build(),
    ));

    // Sky Sphere (Procedural Shader) - TEMPORARILY DISABLED DUE TO SHADER ERROR
    // The shader needs a proper vertex shader to match the fragment shader's VertexOutput
    /*
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(5000.0).mesh().uv(64, 32))),
        MeshMaterial3d(sky_materials.add(SkyMaterial {
            sun_position: Vec3::new(0.0, 1.0, 1.0), // Higher sun for daytime
            turbidity: 10.0,
            rayleigh: 2.0,
            mie_coefficient: 0.005,
            mie_directional_g: 0.8,
        })),
        SkySphere,
        NotShadowCaster,
    ));
    */

    commands.insert_resource(AmbientLight { 
        color: Color::srgb(0.8, 0.9, 1.0), 
        brightness: 1500.0 // Reduced ambient slightly
    });
}

fn sky_sphere_follow_system(
    mut q_sky: Query<&mut Transform, With<SkySphere>>,
    q_cam: Query<&Transform, (With<Camera>, Without<SkySphere>)>,
) {
    if let Ok(cam_t) = q_cam.get_single() {
        if let Ok(mut sky_t) = q_sky.get_single_mut() {
            sky_t.translation = cam_t.translation;
        }
    }
}


fn setup_player(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, assets: Res<GameAssets>) {
    commands.spawn((
        // 1. INCREASE MESH SIZE (Radius 1.0, Length 2.5)
        Mesh3d(meshes.add(Capsule3d::new(1.0, 2.5))), 
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 0.8, 1.0),
            base_color_texture: Some(assets.debug_tex.clone()),
            emissive: LinearRgba::new(0.0, 0.8, 1.0, 2.0),
            ..default()
        })),
        // 2. SPAWN HIGHER (20.0 instead of 10.0)
        Transform::from_xyz(0.0, 20.0, 0.0), 
        Player { 
            fire_timer: 0.0,
            jump_count: 0,
            dash_timer: 0.0,
            dash_cooldown: 0.0,
            is_somersaulting: false,
        },
        AutoTarget::default(),
        Health { current: 500.0, max: 500.0 },
        // 3. INCREASE COLLIDER SIZE (Half Height 1.25, Radius 1.0)
        RigidBody::Dynamic, Collider::capsule_y(1.25, 1.0), LockedAxes::ROTATION_LOCKED,
        Velocity::default(), Friction::coefficient(0.0), GravityScale(GRAVITY_SCALE),
    )).with_children(|parent| {
        parent.spawn(PointLight { color: Color::srgb(0.0, 1.0, 1.0), intensity: 1000.0, range: 25.0, ..default() });
    });
}

fn setup_starting_village(
    mut commands: Commands, 
    mut meshes: ResMut<Assets<Mesh>>, 
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<GameAssets>,
) {
    // Village center at origin
    let village_center = Vec3::new(0.0, 0.0, 0.0);
    
    // Wall material
    let wall_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.35, 0.3),
        base_color_texture: Some(assets.debug_tex.clone()),
        ..default()
    });
    
    // Resource structure material
    let resource_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.6, 0.8),
        base_color_texture: Some(assets.debug_tex.clone()),
        emissive: LinearRgba::new(0.2, 0.6, 0.8, 1.0),
        ..default()
    });
    
    // Create perimeter walls (octagonal layout)
    let wall_radius = 30.0;
    let wall_segments = 8;
    for i in 0..wall_segments {
        let angle = (i as f32 / wall_segments as f32) * PI * 2.0;
        let x = village_center.x + angle.cos() * wall_radius;
        let z = village_center.z + angle.sin() * wall_radius;
        let wall_rotation = Quat::from_rotation_y(angle + PI / 2.0);
        
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(8.0, 6.0, 1.0))),
            MeshMaterial3d(wall_mat.clone()),
            Transform::from_xyz(x, 3.0, z).with_rotation(wall_rotation),
            Wall,
            Structure,
            Health { current: 500.0, max: 500.0 },
            RigidBody::Fixed,
            Collider::cuboid(4.0, 3.0, 0.5),
        ));
    }
    
    // Spawn 3 drills inside the village
    let drill_positions = [
        Vec3::new(-10.0, 1.0, -10.0),
        Vec3::new(10.0, 1.0, -10.0),
        Vec3::new(0.0, 1.0, 10.0),
    ];
    
    for pos in drill_positions.iter() {
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(2.0, 3.0))),
            MeshMaterial3d(resource_mat.clone()),
            Transform::from_translation(village_center + *pos),
            Drill { 
                timer: Timer::from_seconds(5.0, TimerMode::Repeating), 
                storage: 0 
            },
            Structure,
            Health { current: 200.0, max: 200.0 },
            RigidBody::Fixed,
            Collider::cylinder(1.5, 2.0),
        )).with_children(|parent| {
            parent.spawn(PointLight { 
                color: Color::srgb(0.2, 0.8, 1.0), 
                intensity: 500.0, 
                range: 15.0, 
                ..default() 
            });
        });
    }
    
    // Spawn 2 storage bins
    let storage_positions = [
        Vec3::new(-5.0, 1.5, 0.0),
        Vec3::new(5.0, 1.5, 0.0),
    ];
    
    for pos in storage_positions.iter() {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(4.0, 3.0, 4.0))),
            MeshMaterial3d(resource_mat.clone()),
            Transform::from_translation(village_center + *pos),
            StorageBin,
            Structure,
            Health { current: 300.0, max: 300.0 },
            RigidBody::Fixed,
            Collider::cuboid(2.0, 1.5, 2.0),
        ));
    }
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

#[derive(Component)]
struct Ghost;

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
    
    let right_click = mouse_btn.pressed(MouseButton::Right);
    let left_click = mouse_btn.pressed(MouseButton::Left);
    
    // Zoom
    for ev in mouse_wheel.read() {
        rig.goal_radius = (rig.goal_radius - ev.y * rig.zoom_sens).clamp(rig.min_dist, rig.max_dist);
    }

    // Orbit / Steer Logic
    if right_click || left_click {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
        
        let delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
        
        rig.target_yaw -= delta.x * rig.rot_sens;
        rig.target_pitch = (rig.target_pitch - delta.y * rig.rot_sens).clamp(rig.min_pitch, rig.max_pitch);

        // If Right Click, turn player immediately (Steer)
        // If Left Click, only camera turns (Orbit/Look)
        if right_click {
             let target_player_rot = Quat::from_rotation_y(rig.target_yaw);
             player_t.rotation = player_t.rotation.slerp(target_player_rot, dt * 15.0);
        }
    } else {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }

    // Wrap Angles
    rig.target_yaw = (rig.target_yaw + PI) % (2.0 * PI) - PI;
    rig.yaw = (rig.yaw + PI) % (2.0 * PI) - PI;

    // Smooth Camera Follow
    let mut diff = rig.target_yaw - rig.yaw;
    if diff > PI { diff -= 2.0 * PI; }
    if diff < -PI { diff += 2.0 * PI; }
    rig.yaw = rig.yaw + diff * (dt * CAM_SMOOTH_SPEED).min(1.0);

    rig.pitch = rig.pitch.lerp(rig.target_pitch, dt * CAM_SMOOTH_SPEED);
    rig.radius = rig.radius.lerp(rig.goal_radius, dt * 5.0);

    // Calc Position
    let head = player_t.translation + Vec3::new(0.0, 4.5, 0.0); // Focus slightly higher
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
    rapier: Single<&RapierContext>,
) {
    let (cam, cam_t) = q_cam.single();
    let win = q_win.single();

    if let Some(screen_pos) = win.cursor_position() {
        if let Ok(ray) = cam.viewport_to_world(cam_t, screen_pos) {
            if let Some((_, dist)) = rapier.cast_ray(ray.origin, *ray.direction, 1000.0, true, QueryFilter::exclude_dynamic()) {
                cursor.pos = ray.origin + *ray.direction * dist;
                cursor.snapped_pos = (cursor.pos / GRID_SIZE).round() * GRID_SIZE;
                // Keep the exact Y from physics hit to sit on terrain
                cursor.snapped_pos.y = cursor.pos.y; 
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
                    Transform::from_translation(t.translation() + Vec3::new(1.0, 40.0, 0.0)),
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
            let spawn_pos = t.translation() + Vec3::new(0.0, 40.0, 2.0);
            
            if b.spawn_drone_next {
                commands.spawn((
                    Mesh3d(drone_mesh.clone()), MeshMaterial3d(drone_mat.clone()),
                    Transform::from_translation(spawn_pos + Vec3::Y * 15.0),
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

            let mut ent = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_translation(pos + Vec3::new(0.0, y_off, 0.0)).with_rotation(rot),
                Structure,
                Health { current: 150.0, max: 150.0 },
                RigidBody::Fixed,
                final_collider,
            ));

            if mgr.tool == BuildTool::Wall {
                ent.insert(Wall);
            }

            let id = ent.id();

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

// --- GAMEPLAY SYSTEMS ---

fn auto_target_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut q_player: Query<(&Transform, &mut AutoTarget), With<Player>>,
    q_enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    let Ok((p_t, mut auto)) = q_player.get_single_mut() else { return };

    if keys.just_pressed(KeyCode::Tab) {
        if auto.target.is_some() {
            auto.target = None;
        } else {
            // Find closest enemy
            let mut closest = None;
            let mut min_dist = 60.0; // Range

            for (e_entity, e_t) in q_enemies.iter() {
                let dist = p_t.translation.distance(e_t.translation);
                if dist < min_dist {
                    min_dist = dist;
                    closest = Some(e_entity);
                }
            }
            auto.target = closest;
        }
    }
    
    // Validate target (if it died/despawned)
    if let Some(target) = auto.target {
        if q_enemies.get(target).is_err() {
            auto.target = None;
        }
    }
}

fn wow_movement_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_btn: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut q_player: Query<(&mut Velocity, &mut Transform, &mut Player, &AutoTarget)>,
    q_cam: Query<&WowCameraRig>,
    q_enemies: Query<&GlobalTransform, With<Enemy>>,
    rapier: Single<&RapierContext>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mobile: Res<mobile_controls::MobileInput>,
    mut last_mobile_jump: Local<bool>,
) {
    let Ok((mut velocity, mut transform, mut player, auto)) = q_player.get_single_mut() else { return };
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
    let mobile_jump_now = mobile.jump && !*last_mobile_jump;
    *last_mobile_jump = mobile.jump;

    if (keys.just_pressed(KeyCode::Space) || mobile_jump_now) && player.jump_count < 2 {
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
    
    // --- ROTATION ---
    if let Some(target_entity) = auto.target {
        // LOCKED MODE: Face enemy
        if let Ok(target_t) = q_enemies.get(target_entity) {
            let mut look_at = target_t.translation();
            look_at.y = transform.translation.y;
            transform.look_at(look_at, Vec3::Y);
        }
    } 
    // Otherwise Right-Click steer is handled in Camera System (modifying Rotation there directly or here? 
    // It is clearer to handle Rotation HERE if we want strict separation, but the Camera System already does it. 
    // We will assume Camera System handles the "Face Camera" logic on Right Click for smoothness).

    // --- MOVEMENT ---
    let mut move_input = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { move_input.z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { move_input.z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { move_input.x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { move_input.x += 1.0; }
    
    // Mobile Move
    move_input.x += mobile.move_axis.x;
    move_input.z -= mobile.move_axis.y; 

    if move_input.length_squared() > 0.0 {
        move_input = move_input.normalize();
        let speed = if player.dash_timer > 0.0 { PLAYER_SPEED * 2.0 } else { PLAYER_SPEED };

        if auto.target.is_some() {
            // STRAFE (Relative to Player Facing, which is locked to target)
            // W = Forward (towards target), S = Back, A = Strafe Left, D = Strafe Right
            // Transform.forward() etc are local but in world space
            let strafe_vec = (transform.forward() * -move_input.z) + (transform.right() * move_input.x);
            velocity.linvel.x = strafe_vec.x * speed;
            velocity.linvel.z = strafe_vec.z * speed;
        } else {
            // STANDARD / STEER
            // Movement is relative to CAMERA Yaw
            let cam_rot = Quat::from_rotation_y(rig.yaw);
            let move_dir = cam_rot * move_input;
            
            velocity.linvel.x = move_dir.x * speed;
            velocity.linvel.z = move_dir.z * speed;

            // If moving and NOT right-clicking, face movement direction (Keyboard Turn / Free Run)
            if !right_click_held {
                let target_angle = move_dir.x.atan2(move_dir.z) + PI;
                let target_rot = Quat::from_rotation_y(target_angle);
                transform.rotation = transform.rotation.slerp(target_rot, dt * 10.0);
            }
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
    mut player_query: Query<(&Transform, &mut Player, &AutoTarget)>,
    q_enemies: Query<&GlobalTransform, With<Enemy>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    keys: Res<ButtonInput<KeyCode>>,
    sel: Res<SelectionState>,
    mobile: Res<mobile_controls::MobileInput>,
) {
    if keys.pressed(KeyCode::AltLeft) || sel.is_selecting { return; }

    if let Ok((t, mut p, auto)) = player_query.get_single_mut() {
        p.fire_timer -= time.delta_secs();
        if (mouse.pressed(MouseButton::Left) || keys.pressed(KeyCode::Space) || mobile.fire) && p.fire_timer <= 0.0 {
            p.fire_timer = 0.1;
            let spawn_pos = t.translation + Vec3::new(0.0, 1.5, 0.0) + *t.forward() * 0.5;
            
            // Aim logic
            let mut dir = *t.forward();
            if let Some(e) = auto.target {
                if let Ok(et) = q_enemies.get(e) {
                    dir = (et.translation() - spawn_pos).normalize();
                }
            }

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
                Projectile { damage: 35.0, lifetime: Timer::from_seconds(2.0, TimerMode::Once), from_player: true },
                RigidBody::Dynamic, Collider::ball(0.15), Sensor,
                Velocity { linvel: dir * 150.0, angvel: Vec3::ZERO },
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
    enemies_q: Query<Entity, With<Enemy>>,
) {
    if !phase.is_combat { return; }
    
    // Limit max enemies to prevent RAM overflow
    const MAX_ENEMIES: usize = 80;
    let current_enemy_count = enemies_q.iter().count();
    
    // If we're at max, remove oldest enemies
    if current_enemy_count >= MAX_ENEMIES {
        if let Some(oldest) = enemies_q.iter().next() {
            commands.entity(oldest).despawn_recursive();
        }
        return;
    }
    
    let spawn_delay = (1.5 - (phase.wave as f32 * 0.05)).max(0.3);
    *timer += time.delta_secs();
    
    if *timer > spawn_delay {
        *timer = 0.0;
        if let Ok(p_t) = player_q.get_single() {
            let angle = rand::random::<f32>() * PI * 2.0;
            // Spawn distance doubled for 2x world scale, and spawn high above ground (100 units)
            let pos = p_t.translation + Vec3::new(angle.cos() * 160.0, 100.0, angle.sin() * 160.0);
            
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

        // --- Jumping Logic ---
        // If the enemy logic is "Steer towards target", we check if we are significantly below the target Y 
        // OR if we are close to an object that isn't a wall.
        // For simplicity: If distance to target is short but there's a height difference, or just periodic jumping.
        // More specific: Check if elevation of the ground at forward pos is higher.
        // Actually, Kenney models are high. Let's just do a simple proximity jump if not a wall.
    }
}

fn enemy_jump_system(
    mut q_enemies: Query<(&mut Velocity, &Transform), With<Enemy>>,
    q_obstacles: Query<(&GlobalTransform, Option<&Wall>), With<Structure>>,
    time: Res<Time>,
) {
    let t = time.elapsed_secs();
    for (mut vel, transform) in q_enemies.iter_mut() {
        // Periodic jump curiosity
        if (t * 2.0 + transform.translation.x).sin() > 0.98 {
            let mut near_wall = false;
            for (obs_t, wall) in q_obstacles.iter() {
                if wall.is_some() && transform.translation.distance(obs_t.translation()) < 10.0 {
                    near_wall = true;
                    break;
                }
            }
            // Jump if not blocked by a wall
            if !near_wall {
                vel.linvel.y = 25.0; 
            }
        }
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
    // Restart if 'T' is pressed
    if keys.just_pressed(KeyCode::KeyT) {
        if let Ok((mut hp, mut t)) = player_q.get_single_mut() { 
            hp.current = hp.max; 
            t.translation = Vec3::new(0.0, 40.0, 0.0);
            t.rotation = Quat::IDENTITY;
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

        info!("Game Reset!");
        // Always ensure we are in Playing state
        next_state.set(GameState::Playing);
    }
}

fn phase_logic(
    time: Res<Time>, 
    mut pm: ResMut<PhaseManager>, 
    mut lights: Query<&mut DirectionalLight>, 
    mut ambient: ResMut<AmbientLight>,
) {
    pm.timer.tick(time.delta());
    if pm.timer.finished() {
        pm.is_combat = !pm.is_combat;
        pm.timer = Timer::from_seconds(if pm.is_combat { 45.0 } else { 60.0 }, TimerMode::Once);
        if !pm.is_combat { pm.wave += 1; }
        
        let (l_col, a_col) = if pm.is_combat {
            // COMBAT: Vibrant Red/Purple, high contrast
            (Color::srgb(2.0, 0.5, 0.5), Color::srgb(0.2, 0.05, 0.1)) 
        } else {
            // BUILD: Cool Cyan/Blue
            (Color::srgb(1.0, 1.0, 1.5), Color::srgb(0.1, 0.1, 0.2))
        };

        if let Ok(mut l) = lights.get_single_mut() { l.color = l_col; }
        ambient.color = a_col;
    }
}

fn update_hud(
    pm: Res<PhaseManager>, 
    mgr: Res<BuildManager>, 
    stats: Res<PlayerStats>, 
    mut txt: Query<&mut Text, With<HudText>>, 
    state: Res<State<GameState>>,
    settings: Res<WorldSettings>,
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
            "{} - {:.0}s | Wave {}\nScrap: {}/{}\nUnits: {}/{}\nTool: {} [1-6]\n[Alt+Drag] Select | [T] Reset\n---\nSize: {:.2} (Up/Down) | Scale: {:.2} (Left/Right)",
            phase, pm.timer.remaining_secs(), pm.wave, stats.scrap, stats.max_scrap, stats.unit_count, stats.unit_cap, tool, settings.hex_size, settings.tile_scale
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