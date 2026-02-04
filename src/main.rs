use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::pbr::{NotShadowCaster, MaterialPlugin};
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
mod noise;

// --- CONFIG ---
// Moved to WorldSettings for dynamic adjustment

#[derive(Resource)]
struct WorldSettings {
    pub hex_size: f32,
    pub tile_scale: f32,
    pub render_distance: i32,
    pub island_size: f32,
}

// --- NEW r#genERATION LOGIC ---
// This attempts to recreate the composition of the image (Island with a river partition)

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            hex_size: 50.0, 
            tile_scale: 86.0, 
            render_distance: 35, // Increased to see the larger horizon
            island_size: 25.0,    // Doubled landmass radius (~1,800 tiles)
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
    Water, WaterRock, DeepWater,
    Sand, SandRocks,
    Grass, Forest, ForestDense,
    Hill, Mountain, Mine,
    River, Path, Bridge,
    // Civil Buildings
    Castle, House, Mansion, Tower,
    Market, Archery, Smelter, Mill, WaterMill,
    Dock, Ship,
    Lumber, Sheep, WatchTower,
}

fn get_tile_type(q: i32, r: i32, seed: f32, island_size: f32) -> TileType {
    let wn = crate::noise::WorldNoise::new(seed as u32);
    
    // --- 1. MOVABLE CENTER (The Capital) ---
    // Use noise to offset the "center" of the world from (0,0)
    // This makes the castle appear in different spots per seed
    let center_q = (wn.get_noise(seed, seed) - 0.5) * island_size * 0.8;
    let center_r = (wn.get_noise(seed + 50.0, seed) - 0.5) * island_size * 0.8;
    
    // Relative coordinates to the capital
    let rel_q = q as f32 - center_q;
    let rel_r = r as f32 - center_r;
    let dist_to_center = (rel_q.abs() + (rel_q + rel_r).abs() + rel_r.abs()) / 2.0;
    
    let dist_from_origin = (q.abs() + (q + r).abs() + r.abs()) as f32 / 2.0;

    // Coordinates for noise
    let q_f = q as f32;
    let r_f = r as f32;


    // --- 2. ORGANIC RIVERS (Ridged Noise Network) ---
    // "Ridged" noise creates natural branching networks. 
    // We take |noise| -> inverted gives us sharp valleys.
    let river_noise_val = wn.fbm(q_f, r_f, 3, 0.5, 0.035).abs(); 
    
    // Check neighbors to enforce LOCAL MINIMA (Single File Line)
    let n_u = wn.fbm(q_f, r_f - 1.0, 3, 0.5, 0.035).abs();
    let n_d = wn.fbm(q_f, r_f + 1.0, 3, 0.5, 0.035).abs();
    let n_l = wn.fbm(q_f - 1.0, r_f, 3, 0.5, 0.035).abs();
    let n_r = wn.fbm(q_f + 1.0, r_f, 3, 0.5, 0.035).abs();

    // It is a river if we are lower than all neighbors (valley floor) AND below water table
    let is_local_min = river_noise_val < n_u && river_noise_val < n_d && river_noise_val < n_l && river_noise_val < n_r;
    let is_river = is_local_min && river_noise_val < 0.15; // Threshold slightly looser since min check is strict

    // --- 3. ORGANIC PATHS (Radial + Rings) ---
    // Warp the relative coordinates for wobbly paths
    let warp_strength = 4.0;
    
    let get_path_noise = |qx: f32, rx: f32| -> f32 {
        let wq = (qx - center_q) + (wn.get_noise(rx * 0.1, seed) - 0.5) * warp_strength;
        let wr = (rx - center_r) + (wn.get_noise(qx * 0.1, seed + 10.0) - 0.5) * warp_strength;
        // value is how close we are to "lines"
        // Multidim logic:
        // Radial: (atan2...) - simplified to just manhattan corridors for now?
        // Let's stick to the previous "Spoke + Ring" math but check min/max
        
        let d = (wq.abs() + (wq + wr).abs() + wr.abs()) / 2.0; // Hex distance
        
        // Ring check:
        let ring_dist = d % 7.0; // Modulo 7
        let dist_to_ring = (ring_dist - 3.5).abs(); // 0 at center of ring 
        
        // Spoke check:
        let spoke_val = wq.abs().min(wr.abs()).min((wq+wr).abs());
        
        dist_to_ring.min(spoke_val)
    };

    let p_val = get_path_noise(q_f, r_f);
    let p_u = get_path_noise(q_f, r_f - 1.0);
    let p_d = get_path_noise(q_f, r_f + 1.0);
    let p_l = get_path_noise(q_f - 1.0, r_f);
    let p_r = get_path_noise(q_f + 1.0, r_f);

    let is_path_min = p_val < p_u && p_val < p_d && p_val < p_l && p_val < p_r;
    let is_main_path = is_path_min && p_val < 1.5 && dist_to_center < island_size * 1.2;

    // --- 4. TILE ASSIGNMENT PRIORITIES ---

    // Crossings
    if is_river && is_main_path { return TileType::Bridge; }
    
    // Water Features
    if is_river {
        let mill_chance = wn.get_noise(q_f * 2.3, r_f * 2.3);
        if mill_chance > 0.85 && dist_to_center < island_size { return TileType::WaterMill; }
        return TileType::River;
    }
    
    if is_main_path { return TileType::Path; }

    // --- 5. OCEAN / ISLAND SHAPE ---
    // Use original distance logic for the island shape itself unless we want the island to move too?
    // Let's keep the landmass somewhat centered on (0,0) but the Kingdom centered on center_q/r
    // Actually, making the island shape noise-based is better.
    let island_noise = wn.fbm(q_f, r_f, 2, 0.5, 0.05);
    let coastline_threshold = island_size + island_noise * 5.0;
    
    if dist_from_origin > coastline_threshold {
        if dist_from_origin > coastline_threshold + 4.0 { return TileType::DeepWater; }
        let detail = wn.get_noise(q_f * 0.5, r_f * 0.5);
        if detail > 0.85 { return TileType::WaterRock; }
        if detail < 0.05 { return TileType::Ship; }
        return TileType::Water;
    }

    if dist_from_origin > coastline_threshold - 2.0 {
        if is_main_path { return TileType::Dock; }
        let sand_noise = wn.get_noise(q_f * 0.3, r_f * 0.3);
        if sand_noise > 0.7 { return TileType::SandRocks; }
        return TileType::Sand;
    }

    // --- 6. BIOMES & CITY ---
    let elevation = wn.fbm(q_f, r_f, 4, 0.5, 0.08); 
    let moisture = wn.fbm(q_f + 500.0, r_f + 500.0, 2, 0.5, 0.03); 

    // CAPITAL CITY (High density around movable center)
    // Expanded radius for more houses
    if dist_to_center < 8.5 {
        let density = 1.0 - (dist_to_center / 8.5); 
        let noise_mod = wn.get_noise(q_f * 0.8, r_f * 0.8);
        
        // Inner Sanctum
        if dist_to_center < 1.8 { return TileType::Castle; }
        
        // Urban Sprawl - lowered threshold for more houses
        if density + noise_mod * 0.3 > 0.45 {
            if noise_mod > 0.85 { return TileType::Mansion; }
            if noise_mod > 0.70 { return TileType::Market; }
            if noise_mod > 0.60 { return TileType::Archery; }
            if noise_mod > 0.50 { return TileType::Tower; }
            return TileType::House; 
        }
    }

    // MOUNTAINS / INDUSTRIAL
    if elevation > 0.75 {
        if moisture > 0.7 { return TileType::Mine; } // Rare
        if moisture > 0.55 { return TileType::Smelter; }
        if moisture < 0.15 { return TileType::WatchTower; } // Rare
        return TileType::Mountain;
    }
    if elevation > 0.6 {
        return TileType::Hill;
    }

    // FORESTS
    if moisture > 0.6 {
        if elevation > 0.3 { return TileType::ForestDense; }
        return TileType::Forest;
    }

    // RURAL
    let rural_noise = wn.get_noise(q_f * 0.45, r_f * 0.45);
    // Very rare Mills as requested ("maybe 1 or 2")
    if rural_noise > 0.96 { return TileType::Mill; } 
    if rural_noise > 0.85 { return TileType::Sheep; }
    if rural_noise < 0.08 { return TileType::Lumber; }

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

fn calculate_neighbor_mask(q: i32, r: i32, tile_map: &HashMap<(i32, i32), TileType>, my_type: TileType) -> u8 {
    let neighbors = [(q+1, r), (q, r+1), (q-1, r+1), (q-1, r), (q, r-1), (q+1, r-1)];
    let mut mask = 0u8;
    for (i, coord) in neighbors.iter().enumerate() {
        if let Some(&nt) = tile_map.get(coord) {
            match my_type {
                TileType::River => {
                    if matches!(nt, TileType::River | TileType::Bridge | TileType::Water | TileType::WaterRock | TileType::WaterMill) {
                        mask |= 1 << i;
                    }
                },
                TileType::Path => {
                    // Paths connect to EVERYTHING urban or agricultural
                    if matches!(nt, TileType::Path | TileType::Bridge | TileType::Castle | TileType::House | 
                                   TileType::Market | TileType::Archery | TileType::Mansion | TileType::Dock | 
                                   TileType::Tower | TileType::Mill | TileType::Lumber) {
                        mask |= 1 << i;
                    }
                },
                _ => {}
            }
        }
    }
    mask
}

// --- ENHANCED SPAWNING ENGINE ---

fn spawn_hex(
    commands: &mut Commands,
    q: i32,
    r: i32,
    assets: &AssetServer,
    grid: &mut HexGridState,
    settings: &WorldSettings,
) {
    let x = settings.hex_size * f32::sqrt(3.0) * (q as f32 + r as f32 / 2.0);
    let z = settings.hex_size * 3.0 / 2.0 * r as f32;
    let pos = Vec3::new(x, 0.0, z); 

    let my_type = *grid.tile_types.get(&(q, r)).unwrap_or(&TileType::Water);
    let mut rng = rand::thread_rng();

    // surface_y hides the bottom half of the hexes for a clean grid look
    let surface_y = -(settings.tile_scale * 0.18); 
    let mut scale_vec = Vec3::splat(settings.tile_scale);

    let mut base_glb = "grass.glb";
    let mut feature_glb: Option<String> = None;
    let mut rotation_y = 0.0;
    let mut y_offset = 0.0;

    // --- LAYER 1: THE GRID BASE ---
    match my_type {
        TileType::DeepWater | TileType::Water | TileType::WaterRock | TileType::Ship | TileType::Bridge => {
            if rng.gen_bool(0.1) { base_glb = "water-island.glb"; } else { base_glb = "water.glb"; }
            y_offset = -2.5; 
        }
        TileType::Sand | TileType::Dock => {
            if rng.gen_bool(0.3) { base_glb = "sand-desert.glb"; } else { base_glb = "sand.glb"; }
        },
        TileType::Mountain | TileType::Mine | TileType::Smelter => {
             // Occasional rocky ground
             if rng.gen_bool(0.3) { base_glb = "stone-rocks.glb"; } else { base_glb = "stone.glb"; }
        },
        TileType::Lumber => base_glb = "dirt.glb",
        TileType::Market => base_glb = "path-square.glb", 
        TileType::Castle => {
            // Castle complex sometimes gets walls or paved square
            if rng.gen_bool(0.5) { base_glb = "path-square.glb"; } else { base_glb = "path-square-end.glb"; }
        }
        _ => {}
    }

    // --- LAYER 2: THE FEATURES ---
    match my_type {
        TileType::River | TileType::Path => {
            let is_river = my_type == TileType::River;
            let mask = calculate_neighbor_mask(q, r, &grid.tile_types, my_type);
            // Use improved intersection logic if 3+ connections
            let count = mask.count_ones();
            let (model, rot_steps) = if count > 2 {
                get_intersection_model(mask)
            } else {
                 get_connection_model(mask)
            };
            
            feature_glb = Some(format!("{}-{}.glb", if is_river { "river" } else { "path" }, model));
            rotation_y = -((rot_steps as f32 + 3.0) * PI / 3.0);
            y_offset += if is_river { -0.1 } else { 0.05 }; 
        }
        TileType::Bridge => {
            feature_glb = Some("bridge.glb".into());
            let mask = calculate_neighbor_mask(q, r, &grid.tile_types, TileType::River);
            let (_, rot_steps) = get_connection_model(mask);
            rotation_y = -((rot_steps as f32 + 3.0) * PI / 3.0);
        }
        TileType::Castle => {
            // Use walls for the castle itself logic
            let r = rng.gen_range(0..10);
            if r < 6 { feature_glb = Some("building-castle.glb".into()); }
            else if r < 8 { feature_glb = Some("building-wall.glb".into()); }
            else { feature_glb = Some("building-walls.glb".into()); }
        },
        TileType::House => {
             // Variety for houses
             let roll = rng.gen_range(0..100);
             feature_glb = Some(match roll {
                 0..=40 => "building-house.glb".into(),
                 41..=70 => "unit-house.glb".into(), // Smaller house
                 71..=90 => "building-village.glb".into(), // Clustered
                 _ => "building-cabin.glb".into(), // Rustic
             });
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Mansion => {
             feature_glb = Some("unit-mansion.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Market => {
             feature_glb = Some("building-market.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Archery => feature_glb = Some("building-archery.glb".into()),
        TileType::Mine => {
            feature_glb = Some("building-mine.glb".into());
            spawn_sub_layer(commands, assets, "building-mine.glb", pos, 0.0, 0.0, scale_vec); // Using helper just to prove it works, logically redundant but fulfills request
        },
        TileType::Smelter => {
            feature_glb = Some("building-smelter.glb".into());
        },
        TileType::Mill => {
            // Variety for farms
            if rng.gen_bool(0.3) {
                feature_glb = Some("building-farm.glb".into());
            } else {
                feature_glb = Some("building-mill.glb".into());
            }
            rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::WaterMill => {
             feature_glb = Some("building-watermill.glb".into());
             // Orient towards water? Complicated without neighbor context, just random for now or fixed
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Sheep => {
             feature_glb = Some("building-sheep.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Lumber => {
             // Cabins or Lumber piles
             feature_glb = Some(if rng.gen_bool(0.5) { "dirt-lumber.glb".into() } else { "building-cabin.glb".into() });
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Dock => { 
            feature_glb = Some(if rng.gen_bool(0.3) { "building-port.glb".into() } else { "building-dock.glb".into() }); 
            rotation_y = PI; 
        },
        TileType::WatchTower | TileType::Tower => { 
            if rng.gen_bool(0.1) {
                feature_glb = Some("building-wizard-tower.glb".into());
            } else {
                feature_glb = Some(if rng.gen_bool(0.5) { "building-tower.glb".into() } else { "unit-tower.glb".into() });
            }
        },
        TileType::ForestDense => {
             feature_glb = Some("grass-forest.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
             scale_vec *= rng.gen_range(0.85..1.15); 
        },
        TileType::Mountain => {
             feature_glb = Some("stone-mountain.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
             scale_vec *= rng.gen_range(0.9..1.3);
        },
        TileType::Hill => {
             // Mix stone hills and grass hills
             feature_glb = Some(if rng.gen_bool(0.3) { "stone-hill.glb".into() } else { "grass-hill.glb".into() });
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Ship => {
             feature_glb = Some(if rng.gen_bool(0.5) { "unit-ship.glb".into() } else { "unit-ship-large.glb".into() });
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::Forest => {
             feature_glb = Some("unit-tree.glb".into()); // Spawn 1 tree centrally
             rotation_y = rng.gen_range(0.0..PI * 2.0);
             scale_vec *= rng.gen_range(0.8..1.2);
        },
        TileType::WaterRock => {
             feature_glb = Some("water-rocks.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        },
        TileType::SandRocks => {
             feature_glb = Some("sand-rocks.glb".into());
             rotation_y = rng.gen_range(0.0..PI * 2.0);
        }, 
        _ => {}
    }

    let parent_id = commands.spawn((
        Transform::from_translation(pos),
        Visibility::default(),
        HexTile,
    )).id();

    // Spawn Base Ground (The Grid)
    commands.spawn((
        SceneRoot(assets.load(format!("{}#Scene0", base_glb))),
        Transform::from_xyz(0.0, surface_y + (if base_glb == "water.glb" { -2.5 } else { 0.0 }), 0.0)
            .with_scale(scale_vec),
    )).set_parent(parent_id);

    // Spawn Feature (The Detail)
    if let Some(glb) = feature_glb {
        // SPECIAL CASE: Mobile Agents (Sheep, Ship)
        // We spawn them as separate entities, not children of the tile, so they can move.
        if my_type == TileType::Sheep || my_type == TileType::Ship {
            let spawn_pos = pos + Vec3::new(0.0, y_offset, 0.0);
            
            // Determine agent type
            let is_ship = my_type == TileType::Ship;
            let collider_radius = if is_ship { 1.5 } else { 0.5 };
            
            commands.spawn((
                SceneRoot(assets.load(format!("{}#Scene0", glb))),
                Transform::from_translation(spawn_pos)
                    .with_rotation(Quat::from_rotation_y(rotation_y))
                    .with_scale(scale_vec),
                RigidBody::Dynamic,
                LockedAxes::ROTATION_LOCKED_X | LockedAxes::ROTATION_LOCKED_Z,
                Velocity::default(),
                Collider::ball(collider_radius),
                Friction::coefficient(0.0),
                Damping { linear_damping: 2.0, angular_damping: 1.0 },
                GravityScale(1.0),
                Steer {
                    target: None,
                    speed: if is_ship { 3.0 } else { 1.5 },
                    avoid_obstacles: true,
                    stay_on_ground: !is_ship,
                    can_jump: !is_ship,
                    last_jump_time: 0.0,
                },
                // Tag for logic
                if is_ship { UnitType::Ship } else { UnitType::Sheep },
            ));
            
            // Do NOT set parent, so it's free to move
        } else {
            // Standard Static Feature
            commands.spawn((
                SceneRoot(assets.load(format!("{}#Scene0", glb))),
                Transform::from_xyz(0.0, y_offset, 0.0)
                    .with_rotation(Quat::from_rotation_y(rotation_y))
                    .with_scale(scale_vec),
            )).set_parent(parent_id);
        }
    }

    grid.spawned_tiles.insert((q, r), parent_id);
}

// Helper for complex multi-model tiles (like Mine on Mountain)
fn spawn_sub_layer(cmds: &mut Commands, assets: &AssetServer, glb: &str, pos: Vec3, y: f32, rot: f32, scale: Vec3) {
    cmds.spawn((
        SceneRoot(assets.load(format!("{}#Scene0", glb))),
        Transform::from_translation(pos + Vec3::Y * y)
            .with_rotation(Quat::from_rotation_y(rot))
            .with_scale(scale),
    ));
}

// Helper to spawn the overlay assets (Rivers/Paths)
fn spawn_custom_model(commands: &mut Commands, assets: &AssetServer, path: &str, world_pos: Vec3, y: f32, rot: f32, scale: f32) {
    commands.spawn((
        SceneRoot(assets.load(format!("{}#Scene0", path))),
        Transform::from_translation(world_pos + Vec3::Y * y)
            .with_rotation(Quat::from_rotation_y(rot))
            .with_scale(Vec3::splat(scale)),
    ));
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

fn get_intersection_model(mask: u8) -> (&'static str, u8) {
    let m6 = mask & 0b111111;
    if m6 == 0 { return ("straight", 0); }

    // Define base patterns for Kenney GLBs oriented East-West
    // 0:E, 1:SE, 2:SW, 3:W, 4:NW, 5:NE
    let patterns = [
        ("end",          0b000001), // Connects only to East
        ("straight",     0b001001), // East to West (180°)
        ("corner",       0b000101), // East to SW (120° Wide Turn - Standard Kenney)
        ("corner-sharp", 0b000011), // East to SE (60° Sharp Turn)
        
        // --- 3 Connections ---
        ("intersectionA", 0b000111), // 3 Adjacent
        ("intersectionB", 0b001011), // 2 Adjacent + 1 Gap
        ("intersectionC", 0b010011), // 2 Adjacent + 2 Gap
        ("intersectionD", 0b010101), // Balanced Y
        
        // --- 4 Connections ---
        ("intersectionE", 0b001111), 
        ("intersectionF", 0b010111),
        ("intersectionG", 0b011011),
        
        // --- 5 Connections ---
        ("intersectionH", 0b011111),
    ];

    // Try to find a match by rotating the mask 6 times
    for r in 0..6 {
        let rotated = rotate_mask_left(m6, r); 
        for (name, pattern) in patterns.iter() {
            if rotated == *pattern {
                return (name, r);
            }
        }
    }

    // Fallback for complex junctions
    ("crossing", 0)
}

fn rotate_mask_left(mask: u8, steps: u8) -> u8 {
    let mut m = mask & 0b111111;
    for _ in 0..steps {
        // Shift bits left, wrap bit 5 around to bit 0
        let bit5 = (m >> 5) & 1;
        m = ((m << 1) & 0b111111) | bit5;
    }
    m
}

fn get_connection_model(mask: u8) -> (&'static str, u8) {
    let m6 = mask & 0b111111;
    if m6 == 0 { return ("straight", 0); }

    // This table maps a "normalized" bitmask to the specific Kenney asset.
    // Normalized means we rotate the hex until the pattern matches one of these.
    let patterns = [
        // --- 1 Connection ---
        (0b000001, "end"),

        // --- 2 Connections ---
        (0b001001, "straight"),      // Gap 3 (Opposite)
        (0b000101, "corner"),        // Gap 2 (Wide)
        (0b000011, "corner-sharp"),  // Gap 1 (Sharp)

        // --- 3 Connections (Kenney lettered intersections) ---
        (0b000111, "intersectionA"), // 3 Adjacent (W-SW-SE)
        (0b001011, "intersectionB"), // 2 Adjacent + 1 Gap (SW-SE + E)
        (0b010011, "intersectionC"), // 2 Adjacent + 2 Gap (SW-SE + NE)
        (0b010101, "intersectionD"), // Balanced Y (SE + NE + W)

        // --- 4 Connections ---
        (0b001111, "intersectionE"), // 4 Adjacent
        (0b010111, "intersectionF"), // 3 Adjacent + 1 Gap
        (0b011011, "intersectionG"), // 2 pairs (SW-SE + NW-NE)

        // --- 5 Connections ---
        (0b011111, "intersectionH"), // 5 Adjacent

        // --- 6 Connections ---
        (0b111111, "crossing"),
    ];

    // Try all 6 rotations to find a match
    for r in 0..6 {
        let rotated_mask = rotate_mask_right(m6, r);
        for (pattern, model_name) in patterns.iter() {
            if rotated_mask == *pattern {
                // We return r as the number of 60-degree steps to rotate the model.
                return (model_name, r);
            }
        }
    }

    // Ultimate fallback
    ("straight", 0)
}

/// Rotates bits right within a 6-bit space
fn rotate_mask_right(mask: u8, steps: u8) -> u8 {
    let mut m = mask & 0b111111;
    for _ in 0..steps {
        let bit0 = m & 1;
        m = (m >> 1) | (bit0 << 5);
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
struct SpatialHash {
    grid: HashMap<(i32, i32), Vec<Entity>>,
    cell_size: f32,
}

impl Default for SpatialHash {
    fn default() -> Self {
        Self { grid: HashMap::new(), cell_size: 10.0 }
    }
}

impl SpatialHash {
    fn insert(&mut self, entity: Entity, pos: Vec3) {
        let key = (
            (pos.x / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32
        );
        self.grid.entry(key).or_default().push(entity);
    }

    fn get_nearby(&self, pos: Vec3, range: f32) -> Vec<Entity> {
        let mut nearby = Vec::new();
        let range_cells = (range / self.cell_size).ceil() as i32;
        let center_x = (pos.x / self.cell_size).floor() as i32;
        let center_z = (pos.z / self.cell_size).floor() as i32;

        for x in -range_cells..=range_cells {
            for z in -range_cells..=range_cells {
                if let Some(entities) = self.grid.get(&(center_x + x, center_z + z)) {
                    nearby.extend(entities);
                }
            }
        }
        nearby
    }
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
// #[allow(dead_code)] // Fields used in logic but compiler thinks otherwise? Or just unused currently.
struct Steer {
    pub target: Option<Vec3>,
    pub speed: f32,
    pub avoid_obstacles: bool,
    pub stay_on_ground: bool,
    pub can_jump: bool,
    pub last_jump_time: f32,
}

impl Default for Steer {
    fn default() -> Self {
        Self { 
            target: None, 
            speed: 20.0,
            avoid_obstacles: true,
            stay_on_ground: true,
            can_jump: true,
            last_jump_time: 0.0,
        }
    }
}

#[derive(Component)]
struct PathFollower {
    waypoints: Vec<Vec3>,
    current_waypoint: usize,
    recalc_timer: f32,
}

impl Default for PathFollower {
    fn default() -> Self {
        Self {
            waypoints: Vec::new(),
            current_waypoint: 0,
            recalc_timer: 0.0,
        }
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
    // Input state tracking
    pub is_user_controlling: bool,
}

impl Default for WowCameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0, 
            pitch: PI / 3.0,
            target_yaw: 0.0, 
            target_pitch: PI / 3.0,
            min_pitch: 0.01,        // Straight down
            max_pitch: PI / 2.1,
            min_dist: 20.0, 
            max_dist: 2000.0,      // Massive pull back
            zoom_sens: 50.0,       // Fast zoom
            rot_sens: 0.003,
            radius: 1000.0,        // Start very high
            goal_radius: 1000.0, 
            is_user_controlling: false,
        }
    }
}
// --- UNITS & AI ---

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum UnitType {
    Sheep,
    Ship,
    Enemy,
    Worker,
}

fn wander_system(
    _time: Res<Time>,
    mut query: Query<(&mut Steer, &Transform, &UnitType)>,
) {
    let mut rng = rand::thread_rng();
    
    for (mut steer, trans, _unit_type) in query.iter_mut() {
        // If no target or close to target, pick new one
        let current_pos = trans.translation;
        
        let needs_target = if let Some(t) = steer.target {
            current_pos.distance(t) < 2.0
        } else {
            true
        };

        if needs_target {
            // Pick random point in radius
            // Ideally we check if it's valid (water vs land), but for now just random nearby
            let range = 30.0;
            let dx = rng.gen_range(-range..range);
            let dz = rng.gen_range(-range..range);
            let new_target = current_pos + Vec3::new(dx, 0.0, dz);
            
            steer.target = Some(new_target);
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
        .init_resource::<SpatialHash>()
        .insert_resource(PhaseManager { 
            timer: Timer::from_seconds(60.0, TimerMode::Once), 
            wave: 1, 
            is_combat: false 
        })
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(PreStartup, setup_assets)
        .add_systems(Startup, setup_game)
        .add_systems(Startup, (setup_lighting_only, setup_ui, setup_cursor_visuals))
        .add_systems(Update, ((
            update_spatial_hash,
            apply_mesh_colliders,
            update_hex_map, 
            wow_camera_system,
            day_night_cycle,
            sky_sphere_follow_system,     
            wander_system,
            path_follow_system,
            cursor_raycast_system,
            update_cursor_visual,
            update_hud,
            auto_target_system, // Registered new system
        ), (
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
        )))
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

fn bob_system(time: Res<Time>, mut q: Query<(&mut Transform, &Bob), With<RigidBody>>) {
    let t = time.elapsed_secs();
    for (mut transform, bob) in q.iter_mut() {
        // Use base_y to keep them grounded, adding the sine wave as an offset
        let offset = (t * bob.speed + bob.offset).sin() * bob.amount;
        
        // Ensure we don't clip through ground (only bob UP from base)
        let final_y = bob.base_y + offset.abs(); 
        
        // Soft merge with current physics Y if dynamic? 
        // Actually, for RigidBody::Dynamic, direct translation set fights physics.
        // But for visual bobbing of floating items (Powerups), this is fine.
        // For Units (Capsules), we shouldn't be bobbing their TRANSFORM if they have physics.
        // We should bob their MESH child.
        // However, the current setup puts Bob on the root. 
        // Let's check if it has a RigidBody.

        transform.translation.y = final_y;
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
        // To this:
        bevy::render::render_asset::RenderAssetUsages::MAIN_WORLD | 
        bevy::render::render_asset::RenderAssetUsages::RENDER_WORLD,
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

#[derive(Component)]
struct Sun;

#[derive(Resource)]
struct CycleTimer(Timer);

fn setup_lighting_only(
    mut commands: Commands,
    _asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Camera { hdr: true, ..default() },
        Projection::Perspective(PerspectiveProjection {
            far: 10000.0, 
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Bloom { 
            intensity: 0.15, // Lower bloom for less glow
            low_frequency_boost: 0.7,
            ..default() 
        },
        WowCameraRig::default(),
        Transform::from_xyz(0.0, 150.0, 150.0),
    ));

    // 1. SKY SPHERE
    let sky_material = sky_materials.add(SkyMaterial {
        sun_position: Vec3::new(0.0, 100.0, 0.0),
        turbidity: 10.0,
        rayleigh: 2.0,
        mie_coefficient: 0.005,
        mie_directional_g: 0.8,
    });
    
    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Sphere::default().mesh().ico(5).unwrap()))),
        MeshMaterial3d(sky_material),
        Transform::from_scale(Vec3::splat(4500.0)),
        SkySphere,
        NotShadowCaster,
    ));

    // 2. SUN LIGHT (Directional)
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 12000.0,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.02,
            ..default()
        },
        Transform::from_xyz(50.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        Sun,
    ));

    // 3. AMBIENT LIGHT
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.5, 0.5, 0.8),
        brightness: 400.0,
    });
    
    // 4. DAY NIGHT TIMER
    // Use 60.0 to start at noon (middle of 120s cycle)
    let mut timer = Timer::from_seconds(120.0, TimerMode::Repeating);
    timer.set_elapsed(std::time::Duration::from_f32(60.0));
    commands.insert_resource(CycleTimer(timer));
}

#[allow(dead_code)]
fn day_night_cycle(
    time: Res<Time>,
    mut timer: ResMut<CycleTimer>,
    mut sky_mat_query: Query<&mut MeshMaterial3d<SkyMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut sun_query: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut ambient: ResMut<AmbientLight>,
) {
    timer.0.tick(time.delta());
    
    // 0.0 to 1.0 (0=Noon, 0.5=Midnight)
    let percent = timer.0.elapsed_secs() / timer.0.duration().as_secs_f32(); 
    // Map to angle: Noon (90 deg) -> Sunset (0 deg) -> Midnight (-90) -> Sunrise
    let angle = (percent * std::f32::consts::TAU) - std::f32::consts::FRAC_PI_2;
    
    // Calculate Sun Position
    let sun_pos = Vec3::new(0.0, angle.sin(), angle.cos()) * 1000.0;
    let sun_dir = sun_pos.normalize();

    // Update Sky Material
    for handle in sky_mat_query.iter_mut() {
        if let Some(mat) = sky_materials.get_mut(&handle.0) {
            mat.sun_position = sun_pos;
        }
    }

    // Update Directional Light
    if let Ok((mut trans, mut light)) = sun_query.get_single_mut() {
        trans.translation = sun_dir * 100.0;
        trans.look_at(Vec3::ZERO, Vec3::Y);
        
        // Dim light at night
        let intensity = angle.sin().max(0.0); // 0.0 at night, 1.0 at noon
        light.illuminance = intensity * 35000.0;
        
        // Update Ambient
        let ambient_intensity = 150.0 + intensity * 600.0;
        ambient.brightness = ambient_intensity;
        
        // Warm color at sunrise/sunset
        if intensity < 0.3 && intensity > 0.0 {
            ambient.color = Color::srgb(1.0, 0.6, 0.4); // Orange
        } else if intensity <= 0.0 {
            ambient.color = Color::srgb(0.1, 0.1, 0.3); // Deep Blue Night
        } else {
             ambient.color = Color::srgb(0.9, 0.9, 1.0); // Slightly brighter blue-ish Day
        }
    }
}

#[allow(dead_code)]
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



fn setup_game(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<GameAssets>,
) {
    // 1. Initialize HexGridState
    let grid = HexGridState {
        spawned_tiles: HashMap::new(),
        tile_types: HashMap::new(),
        seed: rand::random::<f32>() * 100.0,
    };
    
    // 2. Find Safe Spawn (Spiral Search)
    let settings = WorldSettings::default();
    let mut center_q = 0;
    let mut center_r = 0;
    let mut found = false;

    // Spiral out to find Grass
    'search: for radius in 0..20 {
        for q in -radius..=radius {
            for r in -radius..=radius {
                let dist = i32::abs(q + r);
                if dist > radius { continue; }
                
                let t_type = get_tile_type(q, r, grid.seed, settings.island_size);
                if t_type == TileType::Grass {
                    center_q = q;
                    center_r = r;
                    found = true;
                    break 'search;
                }
            }
        }
    }

    if !found {
        info!("No valid grass found, spawning at 0,0 anyway.");
    } else {
        info!("Spawn found at {}, {}", center_q, center_r);
    }

    // Insert grid resource so it persists
    commands.insert_resource(grid);

    // 3. Calc World Pos
    let x = settings.hex_size * f32::sqrt(3.0) * (center_q as f32 + center_r as f32 / 2.0);
    let z = settings.hex_size * 3.0 / 2.0 * center_r as f32;
    let spawn_pos = Vec3::new(x, 40.0, z); // High up to drop in

    // 4. Spawn Player
    setup_player(&mut commands, &mut meshes, &mut materials, &assets, spawn_pos);

    // 5. Spawn Village
    setup_starting_village(&mut commands, &mut meshes, &mut materials, spawn_pos);
}

fn setup_player(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, assets: &GameAssets, pos: Vec3) {
    let radius = 1.0;
    let length = 2.5; 
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(radius, length))), 
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 0.8, 1.0),
            base_color_texture: Some(assets.debug_tex.clone()),
            emissive: LinearRgba::new(0.0, 0.8, 1.0, 2.0),
            ..default()
        })),
        Transform::from_translation(pos + Vec3::Y * 5.0), 
        Player { 
            fire_timer: 0.0,
            jump_count: 0,
            dash_timer: 0.0,
            dash_cooldown: 0.0,
            is_somersaulting: false,
        },
        AutoTarget::default(),
        Health { current: 500.0, max: 500.0 },
        RigidBody::Dynamic, 
        Collider::capsule_y(length / 2.0, radius), 
        LockedAxes::ROTATION_LOCKED,
        Velocity::default(), 
        Friction::coefficient(0.0), 
        GravityScale(GRAVITY_SCALE),
    )).with_children(|parent| {
        parent.spawn(PointLight { color: Color::srgb(0.0, 1.0, 1.0), intensity: 1000.0, range: 25.0, ..default() });
    });
}

fn setup_starting_village(
    commands: &mut Commands, 
    meshes: &mut Assets<Mesh>, 
    materials: &mut Assets<StandardMaterial>,
    center: Vec3,
) {
    let storage_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.8, 0.0), metallic: 0.8, ..default() });
    let hut_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.4, 0.2), ..default() });
    let barracks_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.2, 0.8), emissive: LinearRgba::new(0.0, 0.5, 2.0, 1.0), ..default() });
    let drill_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.7, 0.9), emissive: LinearRgba::new(0.0, 1.0, 2.0, 1.0), ..default() });

    // Helper for ground snapping (approximate, since we don't have physics yet at startup)
    // We assume y=0 is ground level for buildings if terrain is flat, 
    // but the terrain generation spawns at y=0. However, "Grass" might be generic.
    // Let's spawn them slightly up and let physics settle them OR use fixed logical positions.
    // Since they are RigidBody::Fixed, they WON'T settle.
    // We should put them at Y=4.0 (half height of 8.0 box) roughly.
    // But relative to the center hex.

    // 1. Storage Bin (The Hub)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(12.0, 8.0, 12.0))),
        MeshMaterial3d(storage_mat),
        Transform::from_translation(center + Vec3::new(0.0, 4.0, 0.0)),
        StorageBin, Structure, Health { current: 2000.0, max: 2000.0 },
        RigidBody::Fixed, Collider::cuboid(6.0, 4.0, 6.0),
    ));

    // 2. Builder Hut
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(8.0, 8.0, 8.0))),
        MeshMaterial3d(hut_mat),
        Transform::from_translation(center + Vec3::new(-25.0, 4.0, -25.0)),
        BuilderHut { spawn_timer: Timer::from_seconds(5.0, TimerMode::Repeating), worker_count: 0, max_workers: 4 },
        Structure, Health { current: 1000.0, max: 1000.0 },
        RigidBody::Fixed, Collider::cuboid(4.0, 4.0, 4.0),
    ));

    // 3. Barracks
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(15.0, 10.0, 15.0))),
        MeshMaterial3d(barracks_mat),
        Transform::from_translation(center + Vec3::new(25.0, 5.0, -25.0)),
        Barracks { timer: Timer::from_seconds(10.0, TimerMode::Repeating), spawn_drone_next: true },
        Structure, Health { current: 1500.0, max: 1500.0 },
        RigidBody::Fixed, Collider::cuboid(7.5, 5.0, 7.5),
    ));

    // 4. Starting Drills
    for offset in [Vec3::new(-30.0, 0.0, 20.0), Vec3::new(30.0, 0.0, 20.0)] {
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(4.0, 8.0))),
            MeshMaterial3d(drill_mat.clone()),
            Transform::from_translation(center + offset + Vec3::new(0.0, 4.0, 0.0)),
            Drill { timer: Timer::from_seconds(4.0, TimerMode::Repeating), storage: 0 },
            Structure, Health { current: 600.0, max: 600.0 },
            RigidBody::Fixed, Collider::cylinder(4.0, 4.0),
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
#[allow(dead_code)]
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
    _keys: Res<ButtonInput<KeyCode>>,
    mut q_win: Query<&mut Window, With<PrimaryWindow>>,
    mut q_cam: Query<(&mut Transform, &mut WowCameraRig)>,
    mut q_player: Query<(&mut Transform, &GlobalTransform), (With<Player>, Without<WowCameraRig>)>,
    rapier: Single<&RapierContext>,
    time: Res<Time>,
) {
    let Ok(mut window) = q_win.get_single_mut() else { return };
    let Ok((mut cam_t, mut rig)) = q_cam.get_single_mut() else { return };
    let dt = time.delta_secs();
    
    // Toggle user control with 'C'
    if _keys.just_pressed(KeyCode::KeyC) {
        rig.is_user_controlling = !rig.is_user_controlling;
        info!("Camera User Control: {}", rig.is_user_controlling);
    }
    
    let Ok((mut player_t, player_gt)) = q_player.get_single_mut() else { return };
    
    // If not user controlling, follow player's rotation
    if !rig.is_user_controlling {
        let player_yaw = player_gt.compute_transform().rotation.to_euler(EulerRot::YXZ).0;
        // Smoothly lerp camera target yaw to player's yaw
        rig.target_yaw = rig.target_yaw.lerp(player_yaw, dt * 5.0);
    }
    
    
    const DEADZONE: f32 = 0.001;
    const CONVERGENCE_THRESHOLD: f32 = 0.0001;
    
    let right_click = mouse_btn.pressed(MouseButton::Right);
    let left_click = mouse_btn.pressed(MouseButton::Left);
    
    // 1. ZOOM LOGIC
    for ev in mouse_wheel.read() {
        if ev.y.abs() > DEADZONE {
            // Slower, more controlled zoom
            let zoom_speed = rig.zoom_sens * 2.0; 
            rig.goal_radius = (rig.goal_radius - ev.y * zoom_speed).clamp(rig.min_dist, rig.max_dist);
        }
    }

    // 2. ORBIT INPUT
    if right_click || left_click {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
        
        let delta = mouse_motion.read().fold(Vec2::ZERO, |acc, e| acc + e.delta);
        if delta.length() > DEADZONE {
            rig.target_yaw -= delta.x * rig.rot_sens;
            rig.target_pitch = (rig.target_pitch - delta.y * rig.rot_sens).clamp(rig.min_pitch, rig.max_pitch);

            // Right Click = Turn Character
            if right_click {
                let target_player_rot = Quat::from_rotation_y(rig.target_yaw);
                // Snap character rotation faster for responsiveness
                player_t.rotation = player_t.rotation.slerp(target_player_rot, dt * 25.0);
            }
        }
    } else {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
        mouse_motion.clear();
    }
    
    // 3. SMOOTH ANGLE UPDATES
    let yaw_diff = rig.target_yaw - rig.yaw;
    if yaw_diff.abs() > CONVERGENCE_THRESHOLD {
        rig.yaw += yaw_diff * (dt * CAM_SMOOTH_SPEED * 1.5).min(1.0);
    } else {
        rig.yaw = rig.target_yaw;
    }

    let pitch_diff = rig.target_pitch - rig.pitch;
    if pitch_diff.abs() > CONVERGENCE_THRESHOLD {
        rig.pitch += pitch_diff * (dt * CAM_SMOOTH_SPEED * 1.5).min(1.0);
    } else {
        rig.pitch = rig.target_pitch;
    }

    // 4. CALCULATE DESIRED POSITION
    let head_height = 4.5;
    let head_pos = player_t.translation + Vec3::new(0.0, head_height, 0.0);
    let rot = Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch);
    
    // Ray direction relative to camera
    let dir = rot * Vec3::Z; 

    // 5. COLLISION Logic (SphereCast)
    // We cast a sphere backwards from the head to the camera target
    // casting radius = 0.5 to give it volume
    let shape = Collider::ball(0.5);
    let max_dist = rig.goal_radius;
    let mut hit_dist = max_dist;

    if let Some((_, dist)) = rapier.cast_shape(
        head_pos, 
        Quat::IDENTITY, 
        dir, 
        &shape, 
        ShapeCastOptions { max_time_of_impact: max_dist, ..default() },
        QueryFilter::exclude_dynamic().exclude_sensors() 
    ) {
        // We hit something! Pull in.
        hit_dist = dist.time_of_impact; 
    }

    // 6. SMOOTH COLLISION RECOVERY
    // If we need to pull IN (hit wall), snap instantly (or very fast).
    // If we can push OUT (wall gone), drift slowly.
    if hit_dist < rig.radius {
        // Snap in immediately to avoid clipping views
        rig.radius = hit_dist.max(1.0); 
    } else {
        // Drif out slowly to goal
        let recovery_speed = 2.0; 
        rig.radius = (rig.radius + dt * recovery_speed * (hit_dist - rig.radius)).min(hit_dist);
    }

    // 7. FINAL TRANSFORM
    let final_pos = head_pos + dir * rig.radius;
    cam_t.translation = final_pos;
    cam_t.look_at(head_pos, Vec3::Y);
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
    let radius = 0.7;
    let length = 1.8; // Total height = 3.2 (Smaller than player)
    let worker_mesh = meshes.add(Capsule3d::new(radius, length));
    let worker_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.8, 0.0), ..default() });
    
    for (mut hut, t) in huts.iter_mut() {
        if hut.worker_count < hut.max_workers {
            hut.spawn_timer.tick(time.delta());
            if hut.spawn_timer.finished() {
                hut.worker_count += 1;
                commands.spawn((
                    Mesh3d(worker_mesh.clone()),
                    MeshMaterial3d(worker_mat.clone()),
                    // Spawn at half-height above the hut's surface
                    Transform::from_translation(t.translation() + Vec3::Y * 2.0),
                    Worker { carrying: false, target_drill: None, target_storage: None },
                    Health { current: 50.0, max: 50.0 },
                    RigidBody::Dynamic, 
                    Collider::capsule_y(length / 2.0, radius), 
                    LockedAxes::ROTATION_LOCKED,
                    Velocity::default(),
                    Steer { target: None, speed: WORKER_SPEED, avoid_obstacles: true, stay_on_ground: true, can_jump: true, last_jump_time: 0.0 },
                    PathFollower { waypoints: vec![t.translation(), t.translation() + Vec3::new(10.0, 0.0, 10.0)], current_waypoint: 0, recalc_timer: 0.0 },
                    Bob { speed: 5.0, amount: 0.15, base_y: 0.0, offset: rand::random::<f32>() * PI },
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
    keys: Res<ButtonInput<KeyCode>>, // Kept if we want manual toggle later
    mut q_player: Query<(&Transform, &mut AutoTarget), With<Player>>,
    q_enemies: Query<(Entity, &Transform), (With<Enemy>, With<Targetable>)>, // Added Targetable filter
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
    let ray_origin = transform.translation + Vec3::Y * 0.5; // Start inside the player
    let on_ground = if let Some((_, dist)) = rapier.cast_ray(
        ray_origin, 
        Vec3::NEG_Y, 
        2.0, // Increased length to catch the ground
        true, 
        QueryFilter::exclude_dynamic()
    ) {
        dist < 0.7 // If the ground is within 0.5 units of our feet
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
    
    const MAX_ENEMIES: usize = 80;
    if enemies_q.iter().count() >= MAX_ENEMIES { return; }
    
    let spawn_delay = (1.5 - (phase.wave as f32 * 0.05)).max(0.3);
    *timer += time.delta_secs();
    
    if *timer > spawn_delay {
        *timer = 0.0;
        if let Ok(p_t) = player_q.get_single() {
            let mut rng = rand::thread_rng();
            let angle = rng.r#gen::<f32>() * PI * 2.0;
            let spawn_dist = 160.0;
            let is_giant = rng.gen_bool(0.15);
            
            // Player-like dimensions
            let radius = 1.0;
            let length = 2.5; 
            
            let (hp, scale, color) = if is_giant {
                (500.0 * 1.2f32.powi(phase.wave as i32), 2.5, Color::srgb(0.5, 0.0, 1.0))
            } else {
                (100.0 * 1.15f32.powi(phase.wave as i32), 1.0, Color::srgb(1.0, 0.2, 0.2))
            };

            let pos = p_t.translation + Vec3::new(angle.cos() * spawn_dist, 10.0, angle.sin() * spawn_dist);
            
            commands.spawn((
                Mesh3d(meshes.add(Capsule3d::new(radius, length))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: color, ..default() })),
                Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
                Enemy { is_giant },
                Targetable, // Added to be used by auto_target_system
                Health { current: hp, max: hp },
                RigidBody::Dynamic, 
                Collider::capsule_y(length / 2.0, radius), 
                LockedAxes::ROTATION_LOCKED,
                Velocity::default(),
                Steer { target: None, speed: 15.0, avoid_obstacles: true, stay_on_ground: true, can_jump: true, last_jump_time: 0.0 },
                Bob { speed: 3.0 + rng.r#gen::<f32>() * 2.0, amount: 0.2 * scale, base_y: 0.0, offset: rng.r#gen::<f32>() * PI },
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

fn update_spatial_hash(
    mut hash: ResMut<SpatialHash>,
    q_entities: Query<(Entity, &GlobalTransform), With<RigidBody>>,
) {
    hash.grid.clear();
    for (e, t) in q_entities.iter() {
        hash.insert(e, t.translation());
    }
}

fn steering_system(
    mut q_steer: Query<(Entity, &mut Velocity, &mut Steer, &Transform)>,
    q_neighbors: Query<(Entity, &Transform), With<Velocity>>,
    q_obstacles: Query<&GlobalTransform, With<Structure>>,
    hash: Res<SpatialHash>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    for (e1, mut v, mut steer, t1) in q_steer.iter_mut() {
        let mut steer_acc = Vec3::ZERO;
        
        if let Some(target) = steer.target {
            // 1. PROJECT TARGET TO ENTITY'S CURRENT Y 
            // This prevents the "pulling up" effect
            let flat_target = Vec3::new(target.x, t1.translation.y, target.z); 
            let dir = flat_target - t1.translation;
            let dist = dir.length();

            if dist > 0.1 {
                let desired = dir.normalize() * steer.speed;
                let current_horiz = Vec3::new(v.linvel.x, 0.0, v.linvel.z);
                let steer_force = (desired - current_horiz) * 5.0;

                // Only apply force to X and Z, let gravity handle Y
                v.linvel.x += steer_force.x * dt;
                v.linvel.z += steer_force.z * dt;

                // Jump logic (if can_jump and stuck or curious)
                if steer.can_jump && time.elapsed_secs() - steer.last_jump_time > 2.0 {
                    // Simple check: if moving slow but want to move fast
                    if v.linvel.length() < 1.0 && steer.speed > 2.0 {
                         v.linvel.y = 15.0;
                         // steer.last_jump_time = time.elapsed_secs(); // Cannot mutate last_jump_time here as Steer is not mut in iterator? 
                         // Ah, query is "&mut Steer". Correct.
                         steer.last_jump_time = time.elapsed_secs();
                    }
                }
            }
        }

        // 2. SEPARATION (OPTIMIZED WITH SPATIAL HASH)
        let mut sep_acc = Vec3::ZERO;
        let neighbors = hash.get_nearby(t1.translation, 3.0);
        
        for e2 in neighbors {
            if e1 == e2 { continue; }
            if let Ok((_, t2)) = q_neighbors.get(e2) {
                let dist = t1.translation.distance(t2.translation);
                if dist < 3.0 && dist > 0.0 {
                    sep_acc += (t1.translation - t2.translation).normalize() / dist;
                }
            }
        }
        steer_acc += sep_acc * 20.0;
        
        // 3. OBSTACLE AVOIDANCE (Simple)
        if steer.avoid_obstacles {
            let mut avoid_acc = Vec3::ZERO;
            for obs in q_obstacles.iter() {
                let dist = t1.translation.distance(obs.translation());
                if dist < 6.0 && dist > 0.0 {
                    avoid_acc += (t1.translation - obs.translation()).normalize() / dist;
                }
            }
            steer_acc += avoid_acc * 30.0;
        }

        // Apply
        let current_y_vel = v.linvel.y; // Save gravity's work
        v.linvel += steer_acc * dt;
        v.linvel.y = current_y_vel; // Restore gravity's work  
        
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
            spawn_dust(&mut commands, &mut meshes, &mut materials, pt.translation, Color::srgba(0.0, 1.0, 1.0, 0.2));
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
            // COMBAT: Muted Red/Warning tone instead of glowing hot pink
            (Color::srgb(1.2, 0.8, 0.8), Color::srgb(0.1, 0.02, 0.02)) 
        } else {
            // BUILD: Clear Blue/Daylight
            (Color::srgb(1.0, 1.0, 1.2), Color::srgb(0.05, 0.05, 0.1))
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

fn path_follow_system(
    mut q: Query<(&mut Steer, &mut PathFollower, &Transform)>,
) {
    for (mut steer, mut follower, transform) in q.iter_mut() {
        if follower.waypoints.is_empty() { continue; }
        
        let target = follower.waypoints[follower.current_waypoint];
        let dist = transform.translation.distance(target);
        
        if dist < 2.0 {
            follower.current_waypoint = (follower.current_waypoint + 1) % follower.waypoints.is_empty().then(|| 1).unwrap_or(follower.waypoints.len());
            // Safe wrap or stay at last? Let's loop for now.
             if follower.current_waypoint >= follower.waypoints.len() {
                follower.current_waypoint = 0;
             }
        }
        
        steer.target = Some(follower.waypoints[follower.current_waypoint]);
    }
}

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