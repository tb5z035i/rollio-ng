use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use rollio_test_robot_keyboard::{
    args::Args,
    bus::RobotBus,
    controls::map_key_event,
    render,
    state::{robot_specs_from_config, ControllerState},
};
use rollio_types::config::Config;
use std::error::Error;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    run()
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    args.validate_steps().map_err(invalid_input)?;
    let command_period = args.command_period().map_err(invalid_input)?;
    let render_period = args.render_period().map_err(invalid_input)?;

    let config = Config::from_file(&args.config)?;
    let specs = robot_specs_from_config(&config).map_err(invalid_input)?;
    let mut controller = ControllerState::new(specs.clone(), args.small_step, args.large_step)
        .map_err(invalid_input)?;
    controller.set_status_message(format!(
        "Loaded {} robot(s) from {}",
        specs.len(),
        args.config.display()
    ));

    let bus = RobotBus::connect(&specs)?;
    let mut terminal = TerminalGuard::enter()?;
    let mut next_command_deadline = Instant::now();
    let mut next_render_deadline = Instant::now();

    loop {
        let receive_time = Instant::now();
        bus.drain_states(|robot_name, state| {
            controller.update_state(robot_name, state, receive_time)
        })?;

        let now = Instant::now();
        if now >= next_command_deadline {
            if let Some(pending) = controller.active_command() {
                bus.publish_command(&pending)?;
            }
            next_command_deadline = now + command_period;
        }

        if now >= next_render_deadline {
            render::draw(terminal.writer(), &controller, now)?;
            next_render_deadline = now + render_period;
        }

        let wait_until = next_command_deadline.min(next_render_deadline);
        let timeout = wait_until
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(25));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key_event) => {
                    if let Some(action) = map_key_event(key_event) {
                        let outcome = controller.apply_action(action);
                        if let Some(mode) = outcome.publish_mode_switch {
                            bus.publish_mode_switch(mode)?;
                        }

                        next_command_deadline = Instant::now();
                        next_render_deadline = Instant::now();

                        if outcome.quit_requested {
                            break;
                        }
                    }
                }
                Event::Resize(_, _) => {
                    next_render_deadline = Instant::now();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

struct TerminalGuard {
    stdout: Stdout,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(stdout, EnterAlternateScreen, cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }

        Ok(Self { stdout })
    }

    fn writer(&mut self) -> &mut Stdout {
        &mut self.stdout
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
