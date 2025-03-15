use std::hash::Hash;

use bevy::prelude::*;
use bevy::utils::hashbrown::HashMap;
use bevy::{prelude::*, render::camera::ScalingMode, tasks::IoTaskPool};
use bevy_ggrs::*;
use bevy_matchbox::matchbox_socket::{PeerId, WebRtcSocket};
use bevy_matchbox::MatchboxSocket;
use bevy_rapier2d::prelude::*;

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
            RapierPhysicsPlugin::<NoUserData>::default(),
            RapierDebugRenderPlugin::default(),
        ))
        .rollback_component_with_clone::<RigidBody>()
        .insert_resource(ClearColor(Color::rgb(0.9, 0.3, 0.6)))
        .add_systems(
            Startup,
            (setup, spawn_players, start_matchbox_socket, spawn_map),
        )
        .add_systems(
            Update,
            (
                wait_for_players,
                // REMOVED: car_movement_system
                border_collision_system,
            ),
        )
        .add_systems(ReadInputs, read_local_inputs)
        .add_systems(GgrsSchedule, car_input_system) // Ensure input system is in GgrsSchedule
        .run();
}

const INPUT_FORWARD: u8 = 1 << 0;
const INPUT_REVERSE: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;
const INPUT_DRIFT: u8 = 1 << 4;

type Config = bevy_ggrs::GgrsConfig<u8, PeerId>;
// TODO: Skapa hjälpfunktioner för att beräkna drift
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
        if keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
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
#[derive(Component)]
struct Border;

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

fn start_matchbox_socket(mut commands: Commands) {
    let room_url = "wss://192.168.10.100:3536/extreme_bevy?next=2";
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

fn spawn_players(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Spawn first car (player 0) on the left side
    // commands
    //     .spawn(Sprite::from_image(asset_server.load("car.png")))
    //     .insert(Player { handle: 0 })
    //     .insert(Car::default())
    //     .insert(Velocity::default())
    //     .insert(Collider::cuboid(25.0, 12.5))
    //     .insert(RigidBody::Dynamic)
    //     // .insert(Transform::from_xyz(-200.0, 0.0, 0.0)) // Position on the left
    //     .insert(ActiveEvents::COLLISION_EVENTS)
    //     .with_children(|children| {
    //         children
    //             .spawn(Collider::cuboid(25.0, 12.5))
    //             .insert(Transform::from_xyz(-200.0, 0.0, 0.0))
    //     })
    //     .add_rollback();

    // Spawn second car (player 1) on the right side
    // commands
    //     .spawn(Sprite::from_image(asset_server.load("car.png")))
    //     .insert(Player { handle: 1 })
    //     .insert(Car::default())
    //     .insert(Velocity::default())
    //     .insert(Collider::cuboid(25.0, 12.5))
    //     .insert(RigidBody::Dynamic)
    //     .insert(Transform::from_xyz(200.0, 0.0, 0.0)) // Position on the right
    //     .insert(ActiveEvents::COLLISION_EVENTS)
    //     .add_rollback();
    commands
        .spawn((
            RigidBody::Dynamic,
            Car::default(),
            Velocity::default(),
            Player { handle: 0 },
            Visibility::Visible,
            Transform::from_xyz(-200.0, 0.0, 0.0),
            GravityScale(0.0),
            ActiveEvents::COLLISION_EVENTS,
        ))
        .with_children(|children| {
            children
                .spawn(Collider::cuboid(25.0, 12.5))
                .insert(Sprite::from_image(asset_server.load("car.png")));
        })
        .add_rollback();

    commands
        .spawn((
            RigidBody::Dynamic,
            Car::default(),
            Velocity::default(),
            Player { handle: 1 },
            Visibility::Visible,
            Transform::from_xyz(200.0, 0.0, 0.0),
            GravityScale(0.0),
            ActiveEvents::COLLISION_EVENTS,
        ))
        .with_children(|children| {
            children
                .spawn(Collider::cuboid(25.0, 12.5))
                .insert(Sprite::from_image(asset_server.load("car.png")));
        })
        .add_rollback();
}
fn border_collision_system(
    mut collision_events: EventReader<CollisionEvent>,
    car_query: Query<(), With<Car>>,
    border_query: Query<(), With<Border>>,
) {
    for event in collision_events.read() {
        if let CollisionEvent::Started(entity1, entity2, _) = event {
            let car_involved = car_query.get(*entity1).is_ok() || car_query.get(*entity2).is_ok();
            let border_involved =
                border_query.get(*entity1).is_ok() || border_query.get(*entity2).is_ok();

            if car_involved && border_involved {
                println!("A car has driven over a border collider!");
            }
        }
    }
}

// Setup our scene with a camera and car entity
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Camera
    commands.spawn(Camera2d::default());
    // commands.spawn(Back)
}

fn spawn_map(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Define map dimensions and border thickness
    let map_width = 300.0;
    let map_height = 300.0;
    let border_thickness = 10.0;

    // Left border
    commands.spawn((
        Collider::cuboid(border_thickness / 2.0, map_height / 2.0),
        Transform::from_xyz(-map_width / 2.0 - border_thickness / 2.0, 0.0, 1.0),
        GlobalTransform::default(),
        RigidBody::Fixed,
        Border,
        ActiveEvents::COLLISION_EVENTS,
    ));

    // Right border
    commands.spawn((
        Collider::cuboid(border_thickness / 2.0, map_height / 2.0),
        Transform::from_xyz(map_width / 2.0 + border_thickness / 2.0, 0.0, 1.0),
        GlobalTransform::default(),
        RigidBody::Fixed,
        Border,
        ActiveEvents::COLLISION_EVENTS,
    ));

    // Top border
    commands.spawn((
        Collider::cuboid(map_width / 2.0, border_thickness / 2.0),
        Transform::from_xyz(0.0, map_height / 2.0 + border_thickness / 2.0, 1.0),
        GlobalTransform::default(),
        RigidBody::Fixed,
        Border,
        ActiveEvents::COLLISION_EVENTS,
    ));

    // Bottom border
    commands.spawn((
        Collider::cuboid(map_width / 2.0, border_thickness / 2.0),
        Transform::from_xyz(0.0, -map_height / 2.0 - border_thickness / 2.0, 1.0),
        GlobalTransform::default(),
        RigidBody::Fixed,
        Border,
        ActiveEvents::COLLISION_EVENTS,
    ));
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

        // Input processing
        // Calculate forward vector from the car's current rotation
        let forward = transform.rotation.mul_vec3(Vec3::Y).truncate().normalize();

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

        // Determine if drifting is active
        let is_drifting = input & INPUT_DRIFT != 0;

        // Physics processing
        let speed = velocity.linvel.length();

        // Skip physics if the car is barely moving
        if speed < 1.0 {
            velocity.linvel = Vec2::ZERO;

            // Still process inputs for a stationary car
            if forward_input != 0.0 {
                // Accelerate in the forward direction
                let acceleration = forward * forward_input * car.acceleration * dt;
                velocity.linvel += acceleration;
            }

            continue;
        }

        // Only allow steering when the car is moving at sufficient speed
        if speed > MIN_SPEED_TO_STEER {
            // Rotate based on steering and forward direction
            // Multiply by sign of forward velocity to reverse steering when going backward
            let forward_sign = forward.dot(velocity.linvel).signum();
            transform.rotate(Quat::from_rotation_z(
                steer_input * car.steering_speed * dt * forward_sign,
            ));
        }

        // Accelerate in the forward direction
        if forward_input != 0.0 {
            let acceleration = forward * forward_input * car.acceleration * dt;
            velocity.linvel += acceleration;
        }

        // Friction and drift physics
        // Determine friction coefficient based on drifting
        let friction = if is_drifting {
            car.drift_friction
        } else {
            car.normal_friction
        };

        // Project current velocity onto forward and lateral directions
        let forward_velocity = forward * velocity.linvel.dot(forward);
        let lateral_velocity = velocity.linvel - forward_velocity;

        // Apply lateral friction - the core of the drift mechanic
        let friction_force = lateral_velocity * friction;
        velocity.linvel -= friction_force * dt * 10.0; // Increased effect for more noticeable drifting

        // Also apply some forward friction (engine/rolling resistance)
        let forward_friction = 0.1; // Much less than lateral friction
        velocity.linvel -= forward_velocity * forward_friction * dt;

        if velocity.linvel.length() > car.max_speed {
            velocity.linvel = velocity.linvel.normalize() * car.max_speed;
        }
    }
}

// New system to actually move the car based on velocity
fn car_movement_system(mut query: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
    for (mut transform, velocity) in query.iter_mut() {
        let dt = time.delta_secs();

        // Update position based on velocity
        transform.translation += Vec3::new(velocity.linvel.x, velocity.linvel.y, 0.0) * dt;
    }
}
