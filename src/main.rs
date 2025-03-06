use std::hash::Hash;

use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use bevy::{prelude::*, render::camera::ScalingMode, tasks::IoTaskPool};
use bevy_ggrs::*;
use bevy_matchbox::matchbox_socket::{PeerId, WebRtcSocket};
use bevy_matchbox::MatchboxSocket;

const MIN_SPEED_TO_STEER: f32 = 10.0;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    // fill the entire browser window
                    fit_canvas_to_parent: true,
                    // don't hijack keyboard shortcuts like F5, F6, F12, Ctrl+R etc.
                    prevent_default_event_handling: false,
                    ..default()
                }),
                ..default()
            }),
            GgrsPlugin::<Config>::default(),
        ))
        .rollback_component_with_clone::<Transform>()
        .add_systems(Startup, (setup, spawn_players, start_matchbox_socket))
        .add_systems(Update, (wait_for_players, car_movement_system))
        .add_systems(ReadInputs, read_local_inputs)
        .add_systems(GgrsSchedule, (car_input_system))
        .run();
}

const INPUT_FORWARD: u8 = 1 << 0;
const INPUT_REVERSE: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;
const INPUT_DRIFT: u8 = 1 << 4;

type Config = bevy_ggrs::GgrsConfig<u8, PeerId>;

fn read_local_inputs(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    local_players: Res<LocalPlayers>,
) {
    let mut local_inputs = HashMap::new();
    for handle in &local_players.0 {
        let mut input = 0u8;
        if keys.any_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
            input |= INPUT_FORWARD;
            println!("forward for player {}", handle);
        }
        if keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyA]) {
            input |= INPUT_REVERSE;
        }
        if keys.any_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) {
            input |= INPUT_LEFT;
        }
        if keys.any_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) {
            input |= INPUT_RIGHT;
        }
        if keys.any_pressed([KeyCode::Space]) {
            input |= INPUT_DRIFT;
        }
        local_inputs.insert(*handle, input);
    }
    commands.insert_resource(LocalInputs::<Config>(local_inputs));
}

#[derive(Component)]
struct Player {
    handle: usize,
}

// Component for our car entity
#[derive(Component)]
struct Car {
    acceleration: f32,
    max_speed: f32,
    normal_friction: f32,
    drift_friction: f32,
    steering_speed: f32,
}

impl Default for Car {
    fn default() -> Self {
        Self {
            acceleration: 200.0,
            max_speed: 300.0,
            normal_friction: 0.8,
            drift_friction: 0.1,
            steering_speed: 3.0,
        }
    }
}

// Component to store velocity
#[derive(Component, Default)]
struct Velocity(Vec2);

fn start_matchbox_socket(mut commands: Commands) {
    let room_url = "ws://127.0.0.1:3536/extreme_bevy?next=2";
    info!("connecting to matchbox server: {room_url}");
    commands.insert_resource(MatchboxSocket::new_unreliable(room_url));
}
fn wait_for_players(mut commands: Commands, mut socket: ResMut<MatchboxSocket>) {
    if socket.get_channel(0).is_err() {
        return; // we've already started
    }

    // Check for new connections
    socket.update_peers();
    let players = socket.players();

    let num_players = 2;
    if players.len() < num_players {
        return; // wait for more players
    }

    info!("All peers have joined, going in-game");

    // create a GGRS P2P session
    let mut session_builder = ggrs::SessionBuilder::<Config>::new()
        .with_num_players(num_players)
        .with_input_delay(2);

    for (i, player) in players.into_iter().enumerate() {
        session_builder = session_builder
            .add_player(player, i)
            .expect("failed to add player");
    }

    // move the channel out of the socket (required because GGRS takes ownership of it)
    let channel = socket.take_channel(0).unwrap();

    // start the GGRS session
    let ggrs_session = session_builder
        .start_p2p_session(channel)
        .expect("failed to start session");

    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
}

// fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>) {
//     commands
//         .spawn(Sprite::from_image(asset_server.load("car.png")))
//         .insert(Player{handle: 0})
//         .insert(Car::default())
//         .insert(Velocity::default())
//         .add_rollback();
// }

fn spawn_players(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands
        .spawn(Sprite::from_image(asset_server.load("car.png")))
        .insert(Player { handle: 0 })
        .insert(Car::default())
        .insert(Velocity::default())
        .add_rollback();

    commands
        .spawn(Sprite::from_image(asset_server.load("car.png")))
        .insert(Player { handle: 1 })
        .insert(Car::default())
        .insert(Velocity::default())
        .add_rollback();
}

// Setup our scene with a camera and car entity
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Camera
    commands.spawn(Camera2d::default());
}

// System to handle input and update car velocity and rotation
fn car_input_system(
    time: Res<Time>,
    mut query: Query<(&Car, &mut Velocity, &mut Transform, &Player)>,
    inputs: Res<PlayerInputs<Config>>,
) {
    for (car, mut velocity, mut transform, player) in query.iter_mut() {
        let (input, _) = inputs[player.handle];
        let dt = time.delta_secs();

        // Calculate forward vector from the car's current rotation
        let forward = transform.rotation.mul_vec3(Vec3::Y).truncate();

        // Forward/backward input
        let mut forward_input = 0.0;
        if input & INPUT_FORWARD != 0 {
            forward_input += 1.0;
        }
        if input & INPUT_REVERSE != 0 {
            forward_input -= 1.0;
        }

        // Steering input (rotate the car)
        let mut steer_input = 0.0;
        if input & INPUT_LEFT != 0 {
            steer_input += 1.0;
        }
        if input & INPUT_RIGHT != 0 {
            steer_input -= 1.0;
        }

        // Only allow steering when the car is moving
        if velocity.0.length() > MIN_SPEED_TO_STEER {
            // Rotate based on steering and forward direction
            // Multiply by sign of forward velocity to reverse steering when going backward
            let forward_sign = forward.dot(velocity.0).signum();
            transform.rotate(Quat::from_rotation_z(
                steer_input * car.steering_speed * dt * forward_sign,
            ));
        }

        // Accelerate in the forward direction
        let acceleration = forward * forward_input * car.acceleration * dt;
        velocity.0 += acceleration;

        // Clamp velocity to max speed
        if velocity.0.length() > car.max_speed {
            velocity.0 = velocity.0.normalize() * car.max_speed;
        }
    }
}

// System to update physics (simulate friction and drifting)
fn car_physics_system(
    time: Res<Time>,
    mut query: Query<(&Car, &mut Velocity, &Transform, &Player)>,
    inputs: Res<PlayerInputs<Config>>,
) {
    for (car, mut velocity, transform, player) in query.iter_mut() {
        let (input, _) = inputs[player.handle];
        let dt = time.delta_secs();

        // Skip physics if the car is barely moving
        if velocity.0.length_squared() < 1.0 {
            velocity.0 = Vec2::ZERO;
            continue;
        }

        // Determine friction coefficient based on drifting (space bar)
        let is_drifting = input & INPUT_DRIFT != 0;
        let friction = if is_drifting {
            car.drift_friction
        } else {
            car.normal_friction
        };

        // Calculate the forward vector for the car
        let forward = transform.rotation.mul_vec3(Vec3::Y).truncate().normalize();

        // Project current velocity onto forward and lateral directions
        let forward_velocity = forward * velocity.0.dot(forward);
        let lateral_velocity = velocity.0 - forward_velocity;

        // Apply lateral friction - the core of the drift mechanic
        let friction_force = lateral_velocity * friction;
        velocity.0 -= friction_force * dt * 10.0; // Increased effect for more noticeable drifting

        // Also apply some forward friction (engine/rolling resistance)
        let forward_friction = 0.1; // Much less than lateral friction
        velocity.0 -= forward_velocity * forward_friction * dt;
    }
}

// New system to actually move the car based on velocity
fn car_movement_system(mut query: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
    for (mut transform, velocity) in query.iter_mut() {
        let dt = time.delta_secs();

        // Update position based on velocity
        transform.translation += Vec3::new(velocity.0.x, velocity.0.y, 0.0) * dt;
    }
}
