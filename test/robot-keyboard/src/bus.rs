use crate::state::{PendingCommand, RobotSpec};
use iceoryx2::prelude::*;
use rollio_bus::{robot_command_service_name, robot_state_service_name, CONTROL_EVENTS_SERVICE};
use rollio_types::config::RobotMode;
use rollio_types::messages::{ControlEvent, RobotCommand, RobotState};
use std::error::Error;

type StateSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>;
type CommandPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotCommand, ()>;
type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

struct RobotPort {
    name: String,
    state_subscriber: StateSubscriber,
    command_publisher: CommandPublisher,
}

pub struct RobotBus {
    _node: Node<ipc::Service>,
    robots: Vec<RobotPort>,
    control_publisher: ControlPublisher,
}

impl RobotBus {
    pub fn connect(specs: &[RobotSpec]) -> Result<Self, Box<dyn Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let mut robots = Vec::with_capacity(specs.len());
        for spec in specs {
            let state_service_name: ServiceName =
                robot_state_service_name(&spec.name).as_str().try_into()?;
            let state_service = node
                .service_builder(&state_service_name)
                .publish_subscribe::<RobotState>()
                .open_or_create()?;
            let state_subscriber = state_service.subscriber_builder().create()?;

            let command_service_name: ServiceName =
                robot_command_service_name(&spec.name).as_str().try_into()?;
            let command_service = node
                .service_builder(&command_service_name)
                .publish_subscribe::<RobotCommand>()
                .open_or_create()?;
            let command_publisher = command_service.publisher_builder().create()?;

            robots.push(RobotPort {
                name: spec.name.clone(),
                state_subscriber,
                command_publisher,
            });
        }

        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .open_or_create()?;
        let control_publisher = control_service.publisher_builder().create()?;

        Ok(Self {
            _node: node,
            robots,
            control_publisher,
        })
    }

    pub fn drain_states<F>(&self, mut on_state: F) -> Result<(), Box<dyn Error>>
    where
        F: FnMut(&str, RobotState),
    {
        for robot in &self.robots {
            loop {
                let Some(sample) = robot.state_subscriber.receive()? else {
                    break;
                };
                on_state(&robot.name, *sample.payload());
            }
        }

        Ok(())
    }

    pub fn publish_command(&self, pending: &PendingCommand) -> Result<(), Box<dyn Error>> {
        let Some(robot) = self
            .robots
            .iter()
            .find(|robot| robot.name == pending.robot_name)
        else {
            return Err(format!("robot {} is not connected to the bus", pending.robot_name).into());
        };

        robot.command_publisher.send_copy(pending.command)?;
        Ok(())
    }

    pub fn publish_mode_switch(&self, mode: RobotMode) -> Result<(), Box<dyn Error>> {
        self.control_publisher.send_copy(ControlEvent::ModeSwitch {
            target_mode: mode.control_mode_value(),
        })?;
        Ok(())
    }

    pub fn publish_shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.control_publisher.send_copy(ControlEvent::Shutdown)?;
        Ok(())
    }
}
