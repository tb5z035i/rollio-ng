use crate::state::{mode_label, ControllerState, RobotSession};
use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use std::io::{self, Write};
use std::time::Instant;

pub fn draw<W: Write>(
    writer: &mut W,
    controller: &ControllerState,
    now: Instant,
) -> io::Result<()> {
    let (width, height) = terminal::size().unwrap_or((120, 40));
    let mut lines = build_lines(controller, now);
    let width = width as usize;
    let height = height as usize;

    if lines.len() > height {
        lines.truncate(height);
    }

    queue!(
        writer,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::All)
    )?;
    for (row, line) in lines.iter().enumerate() {
        queue!(
            writer,
            cursor::MoveTo(0, row as u16),
            Print(truncate_line(line, width))
        )?;
    }
    writer.flush()
}

fn build_lines(controller: &ControllerState, now: Instant) -> Vec<String> {
    let active = controller.active_robot();
    let mut lines = vec![
        format!(
            "rollio-test-robot-keyboard | active={} ({}/{}) | tab/1-9 robot | [ ] joint | arrows jog | +/- large | r reset | m mode | q quit",
            active.spec().name,
            controller.active_robot_index() + 1,
            controller.robots().len()
        ),
        format!("status: {}", controller.status_message()),
        "note: mode switch publishes the shared control/events signal seen by all robot drivers.".into(),
        String::new(),
        "Robots".into(),
    ];

    for (index, robot) in controller.robots().iter().enumerate() {
        lines.extend(render_robot_summary(
            index,
            robot,
            index == controller.active_robot_index(),
            now,
        ));
    }

    lines.push(String::new());
    lines.push(format!(
        "Active target for {} | cfg={} | hint={} | selected_joint={} | samples={}",
        active.spec().name,
        mode_label(active.spec().configured_mode),
        mode_label(active.mode_hint()),
        active.selected_joint(),
        active.state_updates()
    ));
    lines.push(format!(
        "  target {}",
        format_joint_values(active.target_positions(), Some(active.selected_joint()))
    ));

    if let Some(state) = active.latest_state() {
        lines.push(format!(
            "  state  {}",
            format_joint_values(
                &state.positions[..active.spec().dof],
                Some(active.selected_joint())
            )
        ));
    } else {
        lines.push("  state  <no state received yet>".into());
    }

    lines
}

fn render_robot_summary(
    index: usize,
    robot: &RobotSession,
    is_active: bool,
    now: Instant,
) -> Vec<String> {
    let marker = if is_active { ">" } else { " " };
    let age = robot
        .latest_state_age(now)
        .map(format_duration)
        .unwrap_or_else(|| "n/a".into());
    let header = format!(
        "{marker} [{}] {} | dof={} | age={} | cfg={} | hint={}",
        index + 1,
        robot.spec().name,
        robot.spec().dof,
        age,
        mode_label(robot.spec().configured_mode),
        mode_label(robot.mode_hint())
    );

    let state_line = if let Some(state) = robot.latest_state() {
        format!(
            "    state {}",
            format_joint_values(
                &state.positions[..robot.spec().dof],
                if is_active {
                    Some(robot.selected_joint())
                } else {
                    None
                }
            )
        )
    } else {
        "    state <no state received yet>".into()
    };

    vec![header, state_line]
}

fn format_joint_values(values: &[f64], highlighted_joint: Option<usize>) -> String {
    if values.is_empty() {
        return "<none>".into();
    }

    values
        .iter()
        .enumerate()
        .map(|(joint_idx, value)| {
            let entry = format!("j{joint_idx}={value:+.3}");
            if highlighted_joint == Some(joint_idx) {
                format!("[{entry}]")
            } else {
                entry
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn format_duration(duration: std::time::Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn truncate_line(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.to_string();
    }

    let visible_width = width.saturating_sub(1);
    let truncated = line.chars().take(visible_width).collect::<String>();
    format!("{truncated}~")
}

#[cfg(test)]
mod tests {
    use super::build_lines;
    use crate::state::{ControllerState, RobotSpec};
    use rollio_types::config::RobotMode;

    #[test]
    fn builds_dashboard_lines() {
        let controller = ControllerState::new(
            vec![RobotSpec {
                name: "robot_0".into(),
                dof: 3,
                configured_mode: RobotMode::FreeDrive,
            }],
            0.05,
            0.2,
        )
        .unwrap();

        let lines = build_lines(&controller, std::time::Instant::now());
        assert!(lines.iter().any(|line| line.contains("robot_0")));
        assert!(lines
            .iter()
            .any(|line| line.contains("mode switch publishes the shared control/events signal")));
    }
}
