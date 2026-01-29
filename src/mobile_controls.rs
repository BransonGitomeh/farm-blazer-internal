use bevy::prelude::*;
use crate::PrimaryWindow;

// Resource to store mobile input state
#[derive(Resource, Default)]
pub struct MobileInput {
    pub move_axis: Vec2, // x, y between -1.0 and 1.0
    pub jump: bool,
    pub fire: bool,
}

// Marker components for UI
#[derive(Component)]
struct MobileControlsRoot;

#[derive(Component)]
struct JoystickBase;

#[derive(Component)]
struct JoystickKnob {
    drag_start: Option<Vec2>,
}

#[derive(Component)]
struct MobileButton {
    action: MobileAction,
}

#[derive(Clone, Copy, PartialEq)]
enum MobileAction {
    Jump,
    Fire,
}

pub struct MobileControlsPlugin;

impl Plugin for MobileControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobileInput>()
            .add_systems(Startup, setup_mobile_ui)
            .add_systems(Update, (
                handle_mobile_visibility,
                handle_joystick_input,
                handle_button_input,
            ));
    }
}

fn setup_mobile_ui(mut commands: Commands) {
    // Root node for all mobile controls
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexEnd,
                padding: UiRect::all(Val::Px(20.0)),
                ..default()
            },
            MobileControlsRoot,
            Visibility::Hidden, // Hidden by default, shown based on screen size
             // Ensure it doesn't block clicks on the world where there are no buttons
            PickingBehavior::IGNORE, 
        ))
        .id();

    // --- LEFT SIDE: JOYSTICK ---
    // Base
    let joystick_base = commands
        .spawn((
            Node {
                width: Val::Px(150.0),
                height: Val::Px(150.0),
                margin: UiRect { left: Val::Px(20.0), bottom: Val::Px(20.0), ..default() },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.2)),
            BorderRadius::all(Val::Percent(50.0)),
            JoystickBase,
            // Interaction needed for drag
            Interaction::default(), 
        ))
        .id();

    // Knob
    let knob = commands
        .spawn((
            Node {
                width: Val::Px(60.0),
                height: Val::Px(60.0),
                position_type: PositionType::Absolute, // Relative to base center if we manage it strictly
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
            BorderRadius::all(Val::Percent(50.0)),
            JoystickKnob { drag_start: None },
            PickingBehavior::IGNORE, // Base handles interaction
        ))
        .id();

    commands.entity(joystick_base).add_child(knob);


    // --- RIGHT SIDE: ACTIONS ---
    let action_container = commands.spawn((
        Node {
             flex_direction: FlexDirection::Column,
             row_gap: Val::Px(20.0),
             margin: UiRect { right: Val::Px(20.0), bottom: Val::Px(20.0), ..default() },
             ..default()
        },
        PickingBehavior::IGNORE, 
    )).id();

    // Fire Button (Big)
    let fire_btn = commands.spawn((
        Node {
            width: Val::Px(100.0),
            height: Val::Px(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 0.2, 0.2, 0.4)),
        BorderRadius::all(Val::Percent(50.0)),
        Interaction::default(),
        MobileButton { action: MobileAction::Fire },
    )).with_child((
        Text::new("FIRE"),
        TextFont { font_size: 20.0, ..default() },
        TextColor(Color::WHITE),
    )).id();

    // Jump Button (Smaller, above or side)
    let jump_btn = commands.spawn((
        Node {
            width: Val::Px(80.0),
            height: Val::Px(80.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            align_self: AlignSelf::FlexEnd,
            ..default()
        },
        BackgroundColor(Color::srgba(0.2, 0.8, 0.2, 0.4)),
        BorderRadius::all(Val::Percent(50.0)),
        Interaction::default(),
        MobileButton { action: MobileAction::Jump },
    )).with_child((
        Text::new("JUMP"),
        TextFont { font_size: 18.0, ..default() },
        TextColor(Color::WHITE),
    )).id();

    commands.entity(action_container).add_child(jump_btn);
    commands.entity(action_container).add_child(fire_btn);

    commands.entity(root).add_child(joystick_base);
    commands.entity(root).add_child(action_container);
}

// Logic to interpret UI interactions into MobileInput
// Note: This is a simplified "virtual joystick" that assumes simple click-and-drag logic relative to center.
// For robust touch handling in Bevy, we might usually look at touch events, but Interaction works for "Mouse-like" touch emulation.

fn handle_joystick_input(
    mut input: ResMut<MobileInput>,
    // 1. Query ComputedNode instead of Node to get actual dimensions
    mut q_base: Query<(&Interaction, &GlobalTransform, &ComputedNode), With<JoystickBase>>,
    mut q_knob: Query<&mut Transform, With<JoystickKnob>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok((interaction, base_gt, computed_node)) = q_base.get_single_mut() else { return };
    let Ok(mut knob_t) = q_knob.get_single_mut() else { return };
    let Ok(window) = primary_window.get_single() else { return };

    // 2. Use .size() from ComputedNode
    let base_radius = computed_node.size().x / 2.0; 
    let center = base_gt.translation().truncate();

    // Reset by default
    input.move_axis = Vec2::ZERO;
    let mut knob_pos = Vec2::ZERO;

    match interaction {
        Interaction::Pressed => {
            if let Some(cursor_pos) = window.cursor_position() {
                let offset = cursor_pos - center;
                let dist = offset.length();
                let clamped_dist = dist.min(base_radius);
                
                if base_radius > 0.0 {
                    let axis = (offset / base_radius).clamp_length_max(1.0);
                     // Invert Y: Screen Y is down (positive), but usually 'Up' on joystick implies -Y in screen space.
                     input.move_axis = Vec2::new(axis.x, -axis.y); 
                }

                let visual_dir = if dist > 0.001 { offset / dist } else { Vec2::ZERO };
                knob_pos = visual_dir * clamped_dist;
            }
        }
        Interaction::None | Interaction::Hovered => {
            knob_pos = Vec2::ZERO;
        }
    }
    
    knob_t.translation = knob_pos.extend(0.0);
}
fn handle_button_input(
    mut input: ResMut<MobileInput>,
    q_btns: Query<(&Interaction, &MobileButton)>,
) {
    input.jump = false;
    input.fire = false;

    for (interaction, btn) in q_btns.iter() {
        if *interaction == Interaction::Pressed {
            match btn.action {
                MobileAction::Jump => input.jump = true,
                MobileAction::Fire => input.fire = true,
            }
        }
    }
}

// System to toggle visibility based on screen width
fn handle_mobile_visibility(
    mut q_root: Query<&mut Visibility, With<MobileControlsRoot>>,
    q_win: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = q_win.get_single() else { return };
    let Ok(mut vis) = q_root.get_single_mut() else { return };

    // Threshold: e.g. 1024px. iPad Pros are wider, but often "mobile controls" are desired on tablets too.
    // User said "hide on LARGER screens". Let's say desktop monitors > 1200? 
    // Or maybe just check if it's a touch device? Bevy doesn't expose "touch device" easily without events.
    // Let's stick to width for now.
    
    // A heuristic: Mobile usually < 800-1000px width (portrait) or < 1200px (landscape).
    // Let's enable it if width < 1000.0 (arbitrary choice, user can tune).
    // Actually, user might test on desktop resizing window.
    
    if window.width() < 1000.0 {
        *vis = Visibility::Visible;
    } else {
        *vis = Visibility::Hidden;
    }
}
