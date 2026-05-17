//! FlatBuffer encoding layer.
//!
//! Converts rollio bus message types into serialized FlatBuffer byte vectors
//! suitable for writing as MCAP message payloads.

use flatbuffers::FlatBufferBuilder;

use crate::fb::foxglove::{
    CompressedVideo, CompressedVideoArgs, JointState, JointStateArgs, JointStates,
    JointStatesArgs, Time,
};
use crate::fb::discover::{Imu, ImuArgs, TactileData, TactileDataArgs, TactilePoint};

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Convert a microsecond unix timestamp to a FlatBuffer `Time` struct.
fn time_from_us(timestamp_us: u64) -> Time {
    let sec = (timestamp_us / 1_000_000) as u32;
    let nsec = ((timestamp_us % 1_000_000) * 1_000) as u32;
    Time::new(sec, nsec)
}

// ---------------------------------------------------------------------------
// CompressedVideo (camera frames)
// ---------------------------------------------------------------------------

/// Encode a camera video packet as a `foxglove.CompressedVideo` FlatBuffer.
///
/// `frame_id` is the camera channel_id, `format` is e.g. "h264",
/// `data` is the raw Annex-B NAL unit payload.
pub fn encode_compressed_video(
    timestamp_us: u64,
    frame_id: &str,
    format: &str,
    data: &[u8],
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::with_capacity(data.len() + 256);
    let ts = time_from_us(timestamp_us);
    let frame_id_off = fbb.create_string(frame_id);
    let format_off = fbb.create_string(format);
    let data_off = fbb.create_vector(data);
    let video = CompressedVideo::create(
        &mut fbb,
        &CompressedVideoArgs {
            timestamp: Some(&ts),
            frame_id: Some(frame_id_off),
            format: Some(format_off),
            data: Some(data_off),
        },
    );
    fbb.finish_minimal(video);
    fbb.finished_data().to_vec()
}

// ---------------------------------------------------------------------------
// JointStates (observations & actions with joint vectors)
// ---------------------------------------------------------------------------

/// Encode joint position/velocity/effort values as `foxglove.JointStates`.
///
/// `joint_names` provides per-DOF names (e.g. ["joint_0", "joint_1", ...]).
/// If `None`, names are generated as "j0", "j1", etc.
pub fn encode_joint_states(
    timestamp_us: u64,
    values: &[f64],
    joint_names: Option<&[String]>,
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::with_capacity(512);
    let ts = time_from_us(timestamp_us);

    // Build individual JointState entries
    let joints: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(i, &val)| {
            let name = if let Some(names) = joint_names {
                names.get(i).map(|s| s.as_str()).unwrap_or("?")
            } else {
                ""
            };
            let name_off = if name.is_empty() {
                let gen = format!("j{i}");
                fbb.create_string(&gen)
            } else {
                fbb.create_string(name)
            };
            JointState::create(
                &mut fbb,
                &JointStateArgs {
                    name: Some(name_off),
                    position: Some(val),
                    velocity: None,
                    acceleration: None,
                    effort: None,
                },
            )
        })
        .collect();

    let joints_vec = fbb.create_vector(&joints);
    let js = JointStates::create(
        &mut fbb,
        &JointStatesArgs {
            timestamp: Some(&ts),
            joints: Some(joints_vec),
        },
    );
    fbb.finish_minimal(js);
    fbb.finished_data().to_vec()
}

/// Encode MIT command (position + velocity + effort per joint) as JointStates.
/// Each joint gets position, velocity, and effort fields populated.
pub fn encode_joint_mit_states(
    timestamp_us: u64,
    len: usize,
    position: &[f64],
    velocity: &[f64],
    effort: &[f64],
    joint_names: Option<&[String]>,
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::with_capacity(1024);
    let ts = time_from_us(timestamp_us);

    let joints: Vec<_> = (0..len)
        .map(|i| {
            let name = if let Some(names) = joint_names {
                names.get(i).map(|s| s.as_str()).unwrap_or("")
            } else {
                ""
            };
            let name_off = if name.is_empty() {
                let gen = format!("j{i}");
                fbb.create_string(&gen)
            } else {
                fbb.create_string(name)
            };
            JointState::create(
                &mut fbb,
                &JointStateArgs {
                    name: Some(name_off),
                    position: position.get(i).copied(),
                    velocity: velocity.get(i).copied(),
                    acceleration: None,
                    effort: effort.get(i).copied(),
                },
            )
        })
        .collect();

    let joints_vec = fbb.create_vector(&joints);
    let js = JointStates::create(
        &mut fbb,
        &JointStatesArgs {
            timestamp: Some(&ts),
            joints: Some(joints_vec),
        },
    );
    fbb.finish_minimal(js);
    fbb.finished_data().to_vec()
}

// ---------------------------------------------------------------------------
// IMU (placeholder — no bus IMU type yet, but the schema is ready)
// ---------------------------------------------------------------------------

/// Encode IMU data as `discover.Imu` FlatBuffer.
pub fn encode_imu(
    timestamp_us: u64,
    frame_id: &str,
    angular_velocity: [f64; 3],
    linear_acceleration: [f64; 3],
) -> Vec<u8> {
    use crate::fb::foxglove::{Quaternion, QuaternionArgs, Vector3, Vector3Args};

    let mut fbb = FlatBufferBuilder::with_capacity(256);
    let ts = time_from_us(timestamp_us);
    let frame_id_off = fbb.create_string(frame_id);

    let ang_vel = Vector3::create(
        &mut fbb,
        &Vector3Args {
            x: angular_velocity[0],
            y: angular_velocity[1],
            z: angular_velocity[2],
        },
    );
    let lin_acc = Vector3::create(
        &mut fbb,
        &Vector3Args {
            x: linear_acceleration[0],
            y: linear_acceleration[1],
            z: linear_acceleration[2],
        },
    );
    // Identity orientation (no orientation data from bus)
    let orient = Quaternion::create(
        &mut fbb,
        &QuaternionArgs {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    );

    let imu = Imu::create(
        &mut fbb,
        &ImuArgs {
            timestamp: Some(&ts),
            frame_id: Some(frame_id_off),
            orientation: Some(orient),
            angular_velocity: Some(ang_vel),
            linear_acceleration: Some(lin_acc),
        },
    );
    fbb.finish_minimal(imu);
    fbb.finished_data().to_vec()
}

// ---------------------------------------------------------------------------
// TactileData (placeholder — schema ready for future tactile sensors)
// ---------------------------------------------------------------------------

/// Encode tactile sensor data as `discover.TactileData` FlatBuffer.
pub fn encode_tactile_data(
    timestamp_us: u64,
    frame_id: &str,
    points: &[[f32; 6]], // [x, y, z, fx, fy, fz] per point
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::with_capacity(256 + points.len() * 24);
    let ts = time_from_us(timestamp_us);
    let frame_id_off = fbb.create_string(frame_id);

    let tactile_points: Vec<TactilePoint> = points
        .iter()
        .map(|p| TactilePoint::new(p[0], p[1], p[2], p[3], p[4], p[5]))
        .collect();
    let points_vec = fbb.create_vector(&tactile_points);

    let td = TactileData::create(
        &mut fbb,
        &TactileDataArgs {
            timestamp: Some(&ts),
            frame_id: Some(frame_id_off),
            points: Some(points_vec),
        },
    );
    fbb.finish_minimal(td);
    fbb.finished_data().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_compressed_video_roundtrip() {
        let data = b"fake h264 nal data";
        let buf = encode_compressed_video(1_700_000_000_000_000, "cam_left", "h264", data);
        // Verify we can parse it back
        let video = flatbuffers::root::<CompressedVideo>(&buf).unwrap();
        assert_eq!(video.frame_id(), Some("cam_left"));
        assert_eq!(video.format(), Some("h264"));
        assert_eq!(video.data().unwrap().bytes(), data);
        let ts = video.timestamp().unwrap();
        assert_eq!(ts.sec(), 1_700_000_000);
        assert_eq!(ts.nsec(), 0);
    }

    #[test]
    fn test_encode_joint_states_roundtrip() {
        let values = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let buf = encode_joint_states(1_000_000, &values, None);
        let js = flatbuffers::root::<JointStates>(&buf).unwrap();
        let ts = js.timestamp().unwrap();
        assert_eq!(ts.sec(), 1);
        assert_eq!(ts.nsec(), 0);
        let joints = js.joints().unwrap();
        assert_eq!(joints.len(), 6);
        assert_eq!(joints.get(0).name(), Some("j0"));
        assert!((joints.get(0).position().unwrap() - 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_encode_imu_roundtrip() {
        let buf = encode_imu(2_500_000, "imu_link", [0.1, 0.2, 0.3], [9.8, 0.0, 0.0]);
        let imu = flatbuffers::root::<Imu>(&buf).unwrap();
        assert_eq!(imu.frame_id(), Some("imu_link"));
        let ts = imu.timestamp().unwrap();
        assert_eq!(ts.sec(), 2);
        assert_eq!(ts.nsec(), 500_000_000);
    }
}
